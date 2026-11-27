# activation → mito-engine (tests de engine.py: activación, archivos y almacenamiento estable)

Origen: `core/tests/test_activation.py` (11 tests), `core/tests/test_stable_storage.py` (7 tests). Sujeto de prueba: `core/app/mito/engine.py` (703 líneas), más `archives`, `terminal`, `storage_migration` y `isolation` cuando los tests los atraviesan.

Este contrato no describe un módulo del original, sino el comportamiento observable que exigen 18 tests de integración sobre `Engine`. Para reescribirlos en Rust no hace falta leer el Python: cada escenario indica el manifiesto, los pasos y las aserciones. Las funciones de `engine.py` que se describen son las que cada test necesita entender.

## Responsabilidad

Verificar, de extremo a extremo, las reglas de `Engine`:
- instalar una versión reemplaza la anterior sin activarla en ningún profile;
- activar y desactivar un componente en un profile actualiza `apps` y los shims de línea de comandos;
- la persistencia (datos del usuario) sobrevive a reinstalación y reubicación de la raíz;
- la extracción de archivos (tar, zip, 7z, rar, ejecutables sueltos) rechaza rutas peligrosas y enlaces antes de escribir;
- el plan de instalación se prepara entero antes de reemplazar nada;
- la migración del almacenamiento legado es idempotente, conserva copias por profile y revierte ante errores.

## Fixtures comunes

**Activación** (`test_activation.py:21-30`):
- Raíz temporal (`tempfile.TemporaryDirectory`, resuelta). Se exige estar bajo el directorio temporal en el test de reubicación (`test_activation.py:63`).
- `Engine(root).initialize()` y profile `test` con `{"workspace": "test", "apps": [], "env": {}}`.

**Manifiesto de prueba de activación** (`install()`, `test_activation.py:32-46`):

```json
{
  "version": "1",
  "url": "https://example.org/tool.zip",
  "hash": "0000000000000000000000000000000000000000000000000000000000000000",
  "bin": "tool.exe",
  "env_set": { "JAVA_HOME": "$dir" }
}
```

Se añade `"persist": <valor>` si el test lo pide (lista `["conf","user.ini"]` en el test de persistencia). Se escribe en `core/buckets/main.json` como `{"apps": {"tool": <manifiesto>}}`. La descarga falsa (`patch('mito.engine.download')`) escribe un zip con:
- `tool.exe` = `fixture`,
- `conf/settings.ini` = `default`,
- `user.ini` = `file default`.

`install()` llama a `engine.install('main/tool')` sin perfil y devuelve el id cuya versión coincide con la pedida.

**Almacenamiento estable** (`test_stable_storage.py:13-26`):
- Profile `one` con `{"workspace": "one", "apps": [], "env": {}}`.
- Manifiesto base: `{"version": "1", "url": "https://example.org/tool.zip", "hash": "0000…", "bin": "tool.exe"}`.
- Descarga falsa (`self.download`, `patch('mito.engine.download')`): zip con `tool.exe` cuyo contenido son los bytes de la URL. Así cada versión se identifica por su contenido.
- Catálogo: `write_json(root/core/buckets/main.json, {"apps": {...}})` vía `self.catalog(apps)`.

**Forma de `core/buckets/main.json`** (usada en todos los tests de ambos ficheros):

```json
{ "apps": { "tool": { "version": "1", "url": "https://example.org/tool.zip", "hash": "0000…", "bin": "tool.exe" } } }
```

La clave de la app es `tool`, el bucket es `main` (nombre del fichero). La clave de instalación es `main/tool`.

**Identidad de instalación**: en el código actual `Engine.install_id(key, lock)` devuelve `key` (`engine.py:136-137`). Una reinstalación siempre tiene el mismo id, `main/tool`. Los tests de migración usan un formato legado distinto, `<key>@<sha256[:16]>` (ver más abajo).

## Formatos de disco

**Shim de línea de comandos** `core/state/shims/<profile>/<alias>.cmd`, escrito por `_write_shims` (`engine.py:591-611`). Contenido exacto para el alias `tool`, profile `test` y clave `main/tool`:

```
@echo off
setlocal DisableDelayedExpansion
"%~dp0..\..\..\tools\python\python.exe" -B -I -X utf8 -m mito run --profile test --app main/tool --bin="tool" -- %*
```

La línea se genera con cuatro `..` y se reduce a tres (`engine.py:605-607`). Junto a los shims se escribe `core/state/shims/<profile>/generated.json` con la lista ordenada de nombres `<alias>.cmd` generados (`engine.py:611`). Al desactivar, los que no están en la lista de deseados se borran (`:599-601`).

**Ruta de instalación legada** (solo en tests de migración): `core/apps/main/tool/<version>-<sha256[:16]>/` con `.mito.json` dentro. El marcador se escribe como `{"key", "record", "manifest_sha256"}`.

**Detección de almacenamiento antiguo** (`engine.py:142-143`): si existe `core/apps/*/*/*/.mito.json`, `Engine.installed()` lanza `Almacenamiento antiguo: cierra las aplicaciones y ejecuta mito.cmd migrate-storage`. Por eso la migración debe ejecutarse antes de cualquier lectura de inventario.

**Copia legada por profile** (`migrate` debe convertirla): `core/profile-apps/<profile>/main/tool/<version>-<fp16>/`. Tras migrar, `core/profile-apps/<profile>/main/tool/`.

**Profile con id legado**: `apps: ["main/tool@<sha256[:16]>"]` antes de migrar; `apps: ["main/tool"]` después.

**Persistencia del profile aislado**: `home/profiles/<profile>/persist/main/tool/data/…`.

**Persistencia compartida**: `home/persist/main/tool/…`.

## Invariantes

1. **Instalar no activa**. Instalar un componente nuevo o reemplazar su versión no añade su id a ningún profile (test 1). Solo `set_active` o `update`/`install` con `profile` lo añaden.
2. **Un id por componente**. Reinstalar la misma clave deja `installed()` con una única entrada, con la versión nueva y ruta `core/apps/main/tool` (test 1, `install_id` es identidad).
3. **Sin duplicados en `apps`**. Activar dos veces el mismo id deja una sola entrada (`engine.py:166-167`).
4. **Desactivar limpia los shims**. Al desactivar el último componente que expone un alias, su `.cmd` se borra en el siguiente `environment` (`engine.py:599-601`, test 1).
5. **La persistencia compartida sobrevive**. Con `persist` sin `shortcuts`, el contenido de `home/persist/<key>/` se conserva tras renombrar la raíz y se reconstruyen los enlaces (test 2).
6. **Reubicación**. Todas las rutas absolutas de enlaces se reconstruyen al llamar a `environment` desde una raíz nueva (test 2, test de `isolation.md`).
7. **Extracción segura**. Un archivo con entradas de ruta peligrosa, enlaces simbólicos o cifradas se rechaza **antes** de crear el directorio de salida (tests 3, 8, 9). En el caso de zip, ni siquiera se intenta el extractor de respaldo (test 8).
8. **Archivos sueltos**. Un archivo que no es contenedor se copia tal cual al destino con el nombre indicado (test 4b).
9. **JAR y alias zip**. Un `.jar` se conserva tal cual; la extracción solo ocurre si el nombre (`filename`) se presenta como `.zip` (test 5). La comparación de la extensión es insensible a mayúsculas.
10. **Plan completo antes de reemplazar**. Si un elemento del plan falla por `Comando ausente`, ningún componente se publica ni cambia el inventario (test de stable `entire_plan`).
11. **Confirmación para descargas sin hash**. Un manifiesto sin `hash` cambia el resultado de `install` a error `sin hash` salvo que se pase `confirm_unverified` (`engine.py:304-305`).
12. **Dependencias sin activar al solicitante**. Reemplazar una dependencia no activa el componente solicitante en otros profiles, solo añade las dependencias necesarias (test stable `dependency_replacement`).
13. **Migración idempotente**. Ejecutar `migrate` dos veces produce el mismo resultado que una (test `migration_keeps…`).
14. **Migración sin cambios si es ambigua**. Dos versiones legadas de la misma clave no producen cambios (test `migration_refuses…`).
15. **Migración atómica respecto a perfiles**. Si falla escribir un profile, el marcador legado y el profile quedan intactos (test `migration_rolls_back…`).
16. **Rollback de copias**. Si falla la preparación de una copia por profile, todos los binarios y datos de usuario previos se restauran (test `failed_copy…`).

## Casos límite y errores

- `install()` con `persist` como lista (`['conf','user.ini']`): el directorio `conf` y el fichero `user.ini` se enlazan individualmente desde el payload hacia `home/persist/main/tool/`.
- `test_manifest_change…`: el mensaje `sin hash` proviene de `engine.py:305` («Instalación cancelada: descargas sin hash no confirmadas»), no de un mensaje específico del catálogo.
- `test_entire_plan…`: el mensaje `Comando ausente: missing.exe` viene de `engine.py:429`, tras `_install_payload` del componente bueno. El componente bueno se prepara en staging pero no se publica.
- `test_dependency_replacement…`: la dependencia se declara como cadena `depends: 'helper'`. `Catalog.resolve('helper')` la convierte en `main/helper`. Duda: la resolución del nombre corto depende de `catalog.resolve`.
- `test_migration_keeps…`: el id legado en `profile.apps` es `key + '@' + sha[:16]`. La migración debe reescribirlo a `main/tool`. El perfil `inactive` **no tiene** fichero `profiles/<p>.json`, pero sí copia en `core/profile-apps/inactive/`, y esa copia también debe migrarse.
- `test_migration_refuses…`: dos registros legados con la misma clave `main/tool` y versiones distintas (`1` y `2`) son ambiguos. El mensaje debe contener `ambigua`.
- `test_tar_rejects_symlinks`: el enlace `link -> ../outside` es un `SYMTYPE`. Se rechaza aunque el destino no exista.
- `test_rar_rejects…`: los listados simulados son salida de `7z l`. La clave `Symbolic Link` y `Encrypted = +` indican error. El listado `Path = ../escape` es ruta peligrosa.
- `test_deflate64…`, `test_extensionless…`: requieren el 7-Zip privado en `core/tools/7zip/7z.exe`. Sin él, el test se salta (`skipTest`) o falla (`test_extensionless…` no salta).
- `test_terminal…`: el ejecutable se busca en `core/tools/windows-terminal/WindowsTerminal.exe`. El último argumento es `shell --profile test`.
- `test_bin_fixed…`: `bin` en forma de lista `[executable, alias, "args"]`. La cadena de argumentos se parte como un shell mínimo (comillas dobles). Duda: confirmar el algoritmo de `arguments.split` (comillas simples, escapes).

## Casos de test del original

Notación de fases (igual que `components.md`):
- **F5** terminal (`terminal.prepare`, `terminal.command`). Lo llama `set_active` (`engine.py:171-172`) y `refresh_launchers` (en `components.py:76`), que `publish_plan` invoca en cada instalación con éxito.
- **F7** hooks / MSI / Inno. `_install_payload` llama `run_pre_install` y `run_installer` (`engine.py:413-422`) en toda instalación. Duda: confirmar que son no-op sin hooks.
- **F8** accesos directos. `publish_plan` llama `migrate_application_links` para cada componente cambiado (`components.py:307-308`).
- **F10** pocketools. `_environment` llama `pocketools.paths` (`engine.py:559-560`) en cada `environment`.
- **F11** migración / traslado. Tests de `storage_migration` y reubicación.
- **Otros**: `archives` (extracción), `arguments` (partido de argumentos), `persistence` (enlaces), `catalog`. No se asignan a una fase de la hoja de ruta; Duda.

### test_activation.py (11 tests)

| Test | Escenario (manifiesto, pasos, aserciones) | Fase posterior |
|---|---|---|
| `test_install_replaces_version_without_activating_component` | Manifiesto de activación. `install` versión `1` → id A; `install` versión `2` → id B. Aserciones: A == B == `main/tool`; `installed()` tiene una entrada, versión `2`, ruta `core/apps/main/tool`; `profile('test').apps == []`. `set_active(test, A, True)` y `set_active(test, B, True)` → `apps == [A]`. `environment('test')`. `set_active(test, A, False)`, `environment('test')`. Aserciones: `installed()` tiene 1 entrada y `core/state/shims/test/tool.cmd` no existe. | F5 (`set_active`), F7 (install), F8 (publish), F10 (`environment`). |
| `test_persist_file_directory_and_relocation` | Manifiesto con `persist: ['conf','user.ini']` y **sin** `shortcuts` (persistencia compartida). `set_active` con `terminal.prepare` parcheado a no-op. `environment('test')` crea enlaces. Se escriben `home/persist/main/tool/conf/settings.ini = "custom"` y `home/persist/main/tool/user.ini = "custom file"`. Se renombra `root` a `root-moved`. Con `Engine(moved).environment('test')`: `moved/core/apps/main/tool/conf/settings.ini == "custom"`, `user.ini == "custom file"`, y `(payload/conf).resolve()` está bajo `moved/home`. Al final se renombra de vuelta. | F11 (traslado), F10 (`environment`). F5 neutralizado en este test. |
| `test_tar_rejects_symlinks` | Archivo `bad.tar` con entrada `link` de tipo `SYMTYPE` y destino `../outside`. `extract(bad.tar, root/out, root, 'bad.tar')` → `YitoError`; `out` no existe. | Ninguna (`archives`). |
| `test_tar_and_standalone_file` | (a) `good.tar.gz` con `folder/file.txt` de 4 bytes `test`. `extract` → `out/folder/file.txt == test`. (b) Fichero `download` con bytes `MZ fixture`; `extract(download, root/exe, root, 'tool.exe')` → `exe/tool.exe` con los mismos bytes. | Ninguna (`archives`). |
| `test_jar_is_preserved_unless_url_alias_requests_zip_extraction` | Zip `download` con `Main.class`. (a) `extract(download, jar, root, 'Tool.JAR')` → `jar/Tool.JAR` idéntico al archivo (no se extrae). (b) `extract(download, zip, root, 'tool.zip')` → `zip/Main.class == fixture`. | Ninguna (`archives`). |
| `test_deflate64_zip_uses_private_extractor` | Raíz del proyecto `parents[2]`. Si no existe `core/tools/7zip/7z.exe`, se salta. Crea `fixture.txt` con `Deflate64 fixture ` repetido 1000 veces, lo comprime con `7z a -tzip -mm=Deflate64`. Aserción: `infolist()[0].compress_type == 9`. `extract(archive, out, runtime_root, 'fixture.zip')` → `out/fixture.txt` igual al original. | Ninguna (requiere 7-Zip privado del bootstrap). |
| `test_extensionless_7zip_detected_by_signature` | Crea `fixture.txt` (`extensionless archive fixture`), lo empaqueta con `7z a -t7z` como `download.7z`, lo renombra a `download` sin extensión. `extract(download, out, runtime_root, 'download')` → `out/fixture.txt` igual. | Ninguna (requiere 7-Zip privado). |
| `test_zip_fallback_never_bypasses_path_validation` | Zip con entrada `../escape.txt`. Se parchea `mito.archives.extract_7zip`. `extract(bad.zip, out, root, 'bad.zip')` → `YitoError`; `extract_7zip` no se llama; `out` no existe. | Ninguna (`archives`). |
| `test_rar_rejects_unsafe_entries_before_extraction` | Fichero `download` con bytes `RAR fixture listing is mocked`. Se crea `core/tools/7zip/7z.exe` vacío. Se parchea `mito.archives.subprocess.run` para devolver tres listados: (1) `Path = ../escape` con `Size = 1`; (2) `Path = settings` con `Symbolic Link = outside`; (3) `Path = settings` con `Encrypted = +`. Por cada uno: `extract(download, out, root, 'download.rar')` → `YitoError`; `run.call_count == 1`; `run.call_args.args[0][1] == 'l'`; `out` no existe. | Ninguna (`archives`). |
| `test_terminal_uses_bundled_executable_and_reactivates_inside_tab` | Crea `core/tools/windows-terminal/WindowsTerminal.exe`. `command(engine, 'test')` (de `mito.terminal`) → `argv[0] == ese exe`; `argv[-3:] == ['shell','--profile','test']`; `core/tools/python/python.exe` está en `argv`. | F5 (`terminal.command`). |
| `test_bin_fixed_arguments_are_preserved` | `Engine.bins({'bin': [['tool.exe','alias','--name "two words" --flag']]})` → primer elemento. Aserción: `[2] == ['--name','two words','--flag']`. | Ninguna (`arguments.split`; Duda). |

### test_stable_storage.py (7 tests)

| Test | Escenario (manifiesto, pasos, aserciones) | Fase posterior |
|---|---|---|
| `test_manifest_change_replaces_same_version_and_unsigned_update_requires_confirmation` | Catálogo con `tool` (`url` original, `hash` `0…`). `install('main/tool','one')` → `old` (ruta `core/apps/main/tool`). Catálogo cambia `url` a `new.zip` y `hash` a `null`. `install('main/tool')` sin confirmación → `YitoError` con `sin hash` (`engine.py:305`); `installed()['main/tool'] == old`. Con `confirm_unverified=lambda _: True` → `install` tiene éxito; `installed` tiene una entrada con la misma `path`; `profile('one').apps == ['main/tool']`; `tool.exe` contiene la nueva URL. | F5, F7, F8 (setup y publicación). |
| `test_dependency_replacement_does_not_activate_requesting_app_in_other_profiles` | Catálogo `runtime` (v1), luego `install('main/runtime','one')`. Profile `two` vacío. Catálogo: `runtime` v2 con `depends: 'helper'`, `helper` (v1), `editor` con `depends: 'runtime'`. `install('main/editor','two')`. Aserciones: `one.apps == ['main/runtime','main/helper']`; `set(two.apps) == {main/helper, main/runtime, main/editor}`; `installed()['main/runtime'].version == '2'`. | F5, F7, F8. |
| `test_entire_plan_is_prepared_before_any_replacement` | `install('main/tool','one')` (v1). Catálogo: `tool` v2 y `broken` con `bin: 'missing.exe'`. `install(['main/tool','main/broken'])` → `YitoError` con `Comando ausente` (`engine.py:429`). `installed() == old` y `profile('one').apps == ['main/tool']`. | F7 (`_install_payload` en staging). F5/F8 solo en el setup. |
| `test_migration_keeps_profile_copies_data_activation_and_is_idempotent` | Legado con `persist: 'data'` y `shortcuts: [['tool.exe','Tool']]` (aislado), versión `1`, ruta `core/apps/main/tool/1-<fp16>`. Profile `one` con `apps: ['main/tool@<fp16 completo>']` y `env: {CUSTOM: value}`. `prepare_payload(root, old, 'one')` y `prepare_payload(root, old, 'inactive')` (sin fichero de profile); se escribe `data/user.txt` = `keep` y `inactive` = `inactive`. `migrate` dos veces. Aserciones: `one.apps == ['main/tool']`; `one.env == {CUSTOM: value}`; `installed()['main/tool'].path == 'core/apps/main/tool'`; para cada `(one, keep)` y `(inactive, inactive)`: `core/profile-apps/<p>/main/tool/tool.exe == b'old'`, `data/user.txt` correcto, y `data` resuelve a `persist_root(root, old, p)/data`. El legado `core/apps/main/tool/1-<fp16>` ya no existe. `environment('one')` no falla. | F11 (`storage_migration`); F10 (`environment` final). |
| `test_migration_refuses_ambiguous_versions_without_changes` | Dos legados `main/tool` versiones `1` y `2`, ambos con `tool.exe`. `migrate(engine)` → `YitoError` que contiene `ambigua`. Ambos `tool.exe` existen; no existe `core/apps/main/tool/.mito.json`. | F11. |
| `test_failed_copy_preparation_restores_all_old_binaries_and_user_data` | Manifiesto aislado (`persist: 'data'`, `shortcuts`). `install('main/tool','one')` (v1). Profile `two` con `apps: ['main/tool']`. Para `one` y `two`: `prepare_payload`, se escribe `tool.exe = <nombre>` y `data/user.txt = <nombre>`. Catálogo con v2. Se parchea `mito.isolation.prepare_payload` para fallar con `OSError('copy failed')` solo cuando `name == 'two'`. `install('main/tool')` → `OSError` esperado. Aserciones: `installed()['main/tool'] == old`; para `one` y `two`: `tool.exe` y `user.txt` sin cambios; `profile(name).apps == ['main/tool']`. | F5, F7, F8 (setup). Rollback sin fase propia. |
| `test_migration_rolls_back_on_profile_write_failure` | Legado con id `main/tool@<fp16>` en profile `one`. Se guarda los bytes de `.mito.json`. Se parchea `Engine.save_profile` para lanzar `OSError('write failed')`. `migrate` → `OSError` con `write failed`. Aserciones: los bytes de `.mito.json` son idénticos; `profile('one').apps == [old_id]`; no existe `core/apps/main/tool/.mito.json`. | F11. |

## Dependencias

- `engine.py`: `Engine` (`initialize`, `profile`, `profiles`, `save_profile`, `installed`, `install`, `set_active`, `environment`, `_environment`, `_write_shims`, `bins`, `install_id`, `component_environment`, `selected`). Solo estas funciones se describen en este contrato.
- `components.py`: `publish_plan`, `refresh_launchers`, `update`, `remove` (ver `components.md`).
- `isolation.py`: `prepare_payload`, `payload_path`, `persist_root` (ver `isolation.md`).
- `catalog.py`: `effective`, `fingerprint`, `array`, `Catalog.resolve`, `Catalog.all`, `Catalog.compatibility`. No documentado aquí.
- `transfer.py`: `download(url, target, expected, root=…)` y `download_hashes`. Los tests lo parchean en `mito.engine.download`.
- `archives.py`: `extract(archive, dest, runtime_root, filename, log=…)` y `extract_7zip`. No documentado aquí. Duda: confirmar la firma, la detección por firma y la lista de extensiones.
- `terminal.py`: `command(engine, profile)` y `prepare(engine)`. No documentado aquí (F5).
- `storage_migration.py`: `migrate(engine, log=…)`. No documentado aquí (F11). **Imprescindible** para los tests de migración.
- `arguments.py`: `split`. No documentado aquí.
- `pocketools.py`: `paths(root)`. (F10).
- `persistence.py`: enlaces `junction`, siembra de `defaults`. No documentado aquí.
- Biblioteca estándar: `zipfile`, `tarfile`, `subprocess` (7-Zip y `7z l`), `unittest.mock`.

## Notas para Rust

Firmas sugeridas (sin código):

- `Engine::install(&self, requested: &[&str], profile: Option<&str>, opts: InstallOptions) -> Result<Vec<Record>, Error>` con `InstallOptions { confirm_unverified, confirm_plan, manifests, pins }`. El test de plan fallido llama con una lista.
- `Engine::set_active(&self, profile: &str, id: &str, active: bool) -> Result<(), Error>`.
- `Engine::environment(&self, profile: &str) -> Result<Env, Error>`.
- `extract(archive: &Path, dest: &Path, runtime_root: &Path, filename: &str) -> Result<(), ArchiveError>`.
- `migrate(engine: &Engine, log: &mut dyn FnMut(&str)) -> Result<(), Error>`.

Dependencias de test:
- **Parches**. Los tests parchean `download`, `extract_7zip`, `subprocess.run`, `save_profile` y `prepare_payload`. En Rust, usar traits (`Downloader`, `Extractor`, `ProcessRunner`, `ProfileStore`) inyectados en `Engine`. Es el único modo práctico de reproducir `test_failed_copy…` y `test_migration_rolls_back…`.
- **Reubicación**. Los tests renombran la raíz temporal y crean un `Engine` nuevo. Usar `tempfile::tempdir()` y `fs::rename` con reintentos (`rename_retrying`, 10 intentos y 0,5 s) por el antivirus.
- **7-Zip privado**. Los tests que lo necesitan deben saltarse con `#[ignore]` o con una comprobación de presencia, igual que `skipTest`. Duda: ¿se mantiene la dependencia de 7-Zip en Rust o se usa la crate `sevenz-rust` o `zip` con Deflate64?

Diferencias:
- **Zip con Deflate64**. Duda: confirmar con la crate elegida si se puede leer Deflate64 (método 9); si no, mantener el respaldo con 7-Zip.
- **Tar**. Rechazar explícitamente las entradas de tipo enlace simbólico y enlace duro antes de extraer (el test exige que `out` no exista).
- **Shims**. El original escribe `\n` con `encoding='utf-8'` (`engine.py:610`) y el test solo comprueba existencia. Duda: confirmar que `cmd` acepta `\n` en este contenido, o usar CRLF si hace falta; el cambio no afecta a los tests.
- **Enlaces (junctions)**. Crear con `junction` crate o con `mklink /J` y `std::process::Command`. El test de "no reconstruir junctions intactos" espera que no se ejecute ningún proceso externo (`persistence.py`).

Dudas abiertas:
1. Duda: `test_persist_file_directory_and_relocation` depende de que `persistence.prepare` siembre los defaults desde el payload al primer uso. Confirmar en el contrato de `persistence`.
2. Duda: el legado `main/tool@<sha16>` y la ruta `core/apps/main/tool/<v>-<fp16>` salen de `storage_migration.py`, que no está documentado. Antes de implementar la migración hay que documentarlo.
3. Duda: `test_migration_keeps…` exige migrar también copias de profiles sin fichero `profiles/<p>.json`. ¿Es intencionado para perfiles eliminados (`core/state/deleted-profiles`)?
4. Duda: `Engine::install` con `Comando ausente` debe dejar staging limpio. ¿Se borra al fallar, o queda para el siguiente arranque?
5. Duda: `test_bin_fixed…` usa `arguments.split`. Confirmar comillas simples, escapes y argumentos vacíos.
6. Duda: `test_extensionless…` no se salta sin 7-Zip, a diferencia de `test_deflate64…`. ¿Es un fallo de cobertura o un requisito fuerte?
7. Duda: en `test_terminal…` el comando incluye `shell --profile test`. Confirmar el formato exacto de la pestaña de Windows Terminal (contrato F5).
