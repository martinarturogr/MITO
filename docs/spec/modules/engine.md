# engine.py → mito-engine

Origen: `core/app/mito/engine.py` (703 líneas), `core/tests/test_engine.py` (47 tests).

Las referencias `engine.py:NN` son al original. Los nombres de funciones de otros módulos (`catalog`, `components`, `hooks`, `isolation`, `terminal`, `jobs`, `transfer`, `bucket_resources`, `inno`, `archives`, `pocketools`, `network`, `open_with`) se citan tal como se usan aquí; sus cuerpos no se han leído para este contrato.

## Responsabilidad

Gestiona profiles (`profiles/<name>.json`), el inventario de componentes instalados (`core/apps/<bucket>/<app>/`), la construcción del entorno de ejecución de un profile (PATH, variables, shims), el pipeline de instalación (plan, descarga, extracción, validación y publicación atómica) y el lanzamiento de aplicaciones, comandos y shells dentro de ese entorno.

## API pública del original

| Función/clase | Entradas | Salida | Errores | Líneas |
|---|---|---|---|---|
| `Engine.__init__(root)` | ruta raíz | Engine (resuelve `root`, crea `Catalog`) | — | `:69-71` |
| `initialize()` | — | — (crea carpetas) | OSError | `:79-83` |
| `profiles()` | — | lista ordenada de nombres (stem de `profiles/*.json`) | — | `:85-86` |
| `profile(name)` | nombre | dict `{workspace, apps, env}` | `YitoError` (ver validaciones §1) | `:88-99` |
| `validate_env(overrides)` (static) | dict | el mismo dict | `YitoError` | `:101-116` |
| `global_env()` | — | dict de `config.properties` | `YitoError` | `:118-133` |
| `install_id(key, lock)` (static) | clave, lock | la propia clave (`key`) | — | `:135-137` |
| `installed()` | — | dict `install_id → lock` | `YitoError` (layout antiguo / incoherente), `read_json` | `:139-150` |
| `selected(data)` | dict de profile | dict `install_id → lock` de sus `apps` | `YitoError` (`Falta la instalación`) | `:152-157` |
| `set_active(profile, install_id, active)` | nombre, id, bool | — | `YitoError` | `:159-172` |
| `save_profile(name, data)` | nombre, dict | — | `OSError` | `:174-175` |
| `create_profile(name, workspace=None)` | nombre, workspace | nombre normalizado | `YitoError` (ya existe) | `:177-188` |
| `ensure_default_profile()` | — | `'default'` o `None` si ya hay profiles | `YitoError` | `:190-201` |
| `delete_profile(name)` | nombre | nombre del primer profile restante (orden alfabético) | `YitoError` (último profile, no existe) | `:203-216` |
| `installation_plan(requested, profile, log, manifests, pins)` | ver firma | dict `{records, current, data, unverified, resource_sources, profiles}` | `YitoError` | `:218-220` |
| `install(requested, profile, log, confirm_unverified, confirm_plan, manifests, pins)` | ver firma | dict `records` | `YitoError` (cancelada, `STATE_CONFLICT`, validaciones) | `:300-316` |
| `bins(manifest)` (static) | manifest | iterador `(executable, alias, args)` | `YitoError` (alias inválido vía `command_name`) | `:442-452` |
| `component_path(payload, relative)` | payload, ruta relativa | `Path` bajo `root` | `YitoError` (ruta no portable) | `:454-458` |
| `component_environment(lock, profile)` | lock, profile | `(vars: dict, paths: list[str])` | `YitoError` | `:460-472` |
| `application_entries(lock)` | lock | lista de `(label, executable, args)` | — | `:474-485` |
| `application_target(lock, shortcut=None)` | lock, etiqueta opcional | `(executable, args)` | `YitoError` | `:487-498` |
| `application_name(lock, shortcut=None)` | lock, etiqueta opcional | nombre visible | `YitoError` | `:500-510` |
| `can_open(lock)` | lock | bool | — (captura errores) | `:512-517` |
| `environment(profile)` | profile | dict de variables (con `mutation`) | `YitoError` | `:519-521` |
| `context_variables(profile)` | profile | dict de variables de contexto (sin efectos) | `YitoError` | `:523-535` |
| `execution_context(profile)` | profile | `(env, workspace)` (crea el workspace) | `YitoError` | `:537-543` |
| `launch(profile, app, binary, arguments, new_terminal, coupled, owned, shortcut)` | ver firma | código de salida (int) | `YitoError` | `:613-690` |
| `shell(profile, arguments, shell_type)` | profile, args, `'cmd'`/`'powershell'` | código de salida | `YitoError` (`Shell no soportada`) | `:692-703` |
| `move_whole(source, target, *, wait, sleep, clock, outcome)` (módulo) | rutas | bool (movido en una operación) | — (reintenta `PermissionError`) | `:19-54` |

Privados relevantes: `_installation_plan` (`:222-298`), `_install_payload` (`:318-440`), `_environment` (`:545-589`), `_write_shims` (`:591-611`).

## Formatos de disco

### 1. `profiles/<name>.json`

Ruta: `<root>/profiles/<name>.json`, donde `<name>` pasa por `identifier()` (minúsculas, `[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}`, sin nombres reservados).

Campos, en este orden (el orden de inserción se conserva al escribir, `storage.md`):

| Campo | Tipo | Obligatorio | Validación | Línea |
|---|---|---|---|---|
| `workspace` | string | sí | `inside(root/workspaces, workspace)` (ruta portable). Si falta, `KeyError`, no `YitoError` | `:98` |
| `apps` | lista de strings | sí | debe ser lista de strings; sin duplicados. Valores = ids de instalación (`key` = `bucket/app`) | `:93-96` |
| `env` | objeto string→string | no (default `{}`) | `validate_env` (ver §3) | `:97`, `:102-116` |

Ejemplo creado por `create_profile('default')` (`:185`, también `ensure_default_profile`, `:196`):

```json
{
  "workspace": "default",
  "apps": [],
  "env": {}
}
```

Ejemplo con una app y una variable de perfil (forma de los fixtures de `test_engine.py`):

```json
{
  "workspace": "test",
  "apps": [
    "test/tool"
  ],
  "env": {
    "MY_OPTION": "two words"
  }
}
```

**Creación** (`create_profile`, `:177-188`): dentro de `mutation(root)`: normaliza nombre y workspace con `identifier` (workspace por defecto = nombre); si ya existe → `El profile {p1} ya existe`; `initialize()`; `mkdir` de `workspaces/<workspace>` con `exist_ok=True` (un workspace puede compartirse entre profiles, test 11); escribe `{'workspace', 'apps': [], 'env': {}}`; llama `terminal.prepare` (F5).

**Default** (`ensure_default_profile`, `:190-201`): si no hay ningún profile crea `default` con el mismo contenido que el ZIP de release (test 4 lo fija en `profiles/default.json`). Si otro proceso lo crea entre la comprobación y el lock, devuelve `None`.

**Borrado** (`delete_profile`, `:203-216`), dentro de `mutation`:
1. `profile(name)` (falla si no existe).
2. Si `len(profiles()) <= 1` → `No se puede eliminar el último profile`. Esta es la regla "nunca borrar el último".
3. Copia de seguridad: `write_json(core/state/deleted-profiles/<name>.json, data)`.
4. Borra `profiles/<name>.json`.
5. `open_with.unregister(root, record.profile == name)`: quita las entradas de «Abrir con» de ese profile.
6. `terminal.prepare` (F5).
7. Devuelve `profiles()[0]` como profile de respaldo.

**Qué NO hace el borrado**: no borra `home/` (es único para toda la instancia, ver §3), ni `workspaces/<workspace>` (aunque sea exclusivo del profile), ni las copias por profile de `core/profile-apps` o `home/persist/<profile>` (no aparecen en `engine.py`). Duda: confirmar con `isolation.py` si esos datos se limpian en otro sitio.

Duda: `core/state/deleted-profiles/` no lo crea `initialize()` (`:80`). Según `storage.md` `write_json` crea el temporal en el directorio padre, así que el primer borrado podría fallar. Verificar si `write_json` crea el padre.

Duda: `write_json` sobrescribe en silencio una copia de seguridad previa del mismo nombre.

### 2. `.mito.json` (inventario de componentes)

Ruta: `<root>/core/apps/<bucket>/<app>/.mito.json`, dentro del propio payload publicado. Lo escribe `_install_payload` en el staging (`:430-431`) antes del `rename` a destino (`:439`), así que se publica con el payload.

Formato exacto (`:430` para `record`, `:431` para el envoltorio):

```json
{
  "key": "test/tool",
  "manifest_sha256": "<fingerprint(manifest), ver catalog.py>",
  "record": {
    "key": "test/tool",
    "version": "1.0",
    "manifest_sha256": "<igual que la línea anterior>",
    "path": "core/apps/test/tool",
    "manifest": {
      "version": "1.0",
      "url": "https://example.org/tool.zip",
      "hash": "<sha256 de 64 caracteres hex>",
      "bin": [["bin/tool.exe", "tool"]],
      "env_set": {"JAVA_HOME": "$dir"},
      "env_add_path": "bin"
    }
  }
}
```

Ejemplo derivado del fixture `manifest()` de `test_engine.py:78-79`. Los digests son marcadores, no valores reales.

Campos:
- `key`: `bucket/app`. Debe coincidir con `record.key` y con el nombre de carpetas (`lock['path'] == 'core/apps/' + key`, `:147`).
- `manifest_sha256`: `fingerprint(manifest)` (`catalog.py`). Se compara en la activación con `fingerprint(lock['manifest'])` (`:568`).
- `record`: copia del registro de `_installation_plan` (`:270`) más `path`. `path` lo calcula `installed()` a partir de la ubicación del marcador (`:146`).
- `record.manifest`: el manifiesto tal como está en el bucket (sin `effective()`; `effective` se aplica al leer, `:464`, `:476`).
- `record.pinned`: aparece en el plan (`:272`) pero NO en el `record` escrito en disco (`:430`). Duda: confirmar si `components.publish_plan` lo reescribe; si no, una versión fijada pierde el pin al instalar.

**Descubrimiento del inventario** (`installed`, `:139-150`):
1. Si existe cualquier `core/apps/*/*/*/.mito.json` (layout de tres niveles antiguo) → `Almacenamiento antiguo: cierra las aplicaciones y ejecuta mito.cmd migrate-storage`.
2. Para cada `core/apps/*/*/.mito.json` (ordenado): lee el marcador, construye `lock = record + path`, verifica `lock.path == 'core/apps/' + lock.key` y que la clave no esté repetida (`Identidad o ruta de componente incoherente`), y guarda `result[key] = lock`.
3. `installed()` NO verifica `key` del envoltorio contra `record.key` ni el digest. Eso lo hace `_environment` al activar (`:565-569`).

Otros ficheros de disco que escribe o lee el motor:
- `core/state/shims/<profile>/<alias>.cmd` y `core/state/shims/<profile>/generated.json` (registro de wrappers, lista ordenada de nombres). Ver §3.
- `core/state/deleted-profiles/<name>.json`: copia del profile borrado.
- `config.properties` en la raíz (ver §3).
- `core/staging/<tmp>/` (temporal, se borra siempre).
- `home/`, `workspaces/<ws>/`: carpetas creadas por `initialize`.

## Construcción del entorno de un profile

### Carpetas creadas por `initialize()` (`:79-83`)

Siempre (en este orden): `home`, `home/AppData/Roaming`, `home/AppData/Local`, `home/tmp`, `home/custom-commands`, `profiles`, `workspaces`, `core/cache`, `core/staging`, `core/state`, `core/apps`.

Dentro de `home/` (`USER_FOLDERS`, `:76-77`, `:82-83`): `Desktop`, `Documents`, `Downloads`, `Music`, `Pictures`, `Videos`, `Favorites`, `Links`, `Contacts`, `Searches`, `Saved Games`, `AppData/LocalLow`. Motivo en `:73-75`: sin esas carpetas `SHGetKnownFolderPath` falla (Electron aborta con `Failed to get 'downloads' path`).

No la crea `initialize`: `core/state/shims/<profile>` (lo crea `_write_shims`, `:592`), `core/tools`, `core/state/deleted-profiles` (ver Duda en §1).

### Home es global, no por profile

`home/` es uno solo para toda la instancia. `context_variables` usa siempre `root/home` (`:527`); el test 11 comprueba que `HOME` es igual en dos profiles. Solo varían `MITO_PROFILE` y `MITO_WORKSPACE`.

### Variables de contexto (`context_variables`, `:523-535`)

| Variable | Valor | Línea |
|---|---|---|
| `MITO_PROFILE` | nombre normalizado | `:529` |
| `MITO_ROOT` | `str(root)` | `:529` |
| `MITO_WORKSPACE` | `root/workspaces/<workspace>` | `:530` |
| `HOME` | `root/home` | `:531` |
| `USERPROFILE` | `root/home` | `:531` |
| `APPDATA` | `root/home/AppData/Roaming` | `:532` |
| `LOCALAPPDATA` | `root/home/AppData/Local` | `:532` |
| `TEMP`, `TMP` | `root/home/tmp` | `:533` |
| `HOMEDRIVE` | unidad de `home` (`home.drive`) | `:534` |
| `HOMEPATH` | `str(home)` sin la unidad | `:534` |

`context_variables` no llama a `initialize`, no crea ficheros y no activa nada (lo documenta el propio código, `:524`). Se usa en la vista previa y en el test 11.

### Orden de construcción en `_environment` (`:545-589`)

Se ejecuta dentro de `mutation(root)` (`environment`, `:519-521`; `execution_context`, `:537-543`).

1. `initialize()` y `data = profile(profile)` (`:546-547`).
2. `env = dict(os.environ)` (`:549`).
3. `inherited_path = env['MITO_HOST_PATH']` o, si no existe, `env['PATH']` (`:552`). Así los profiles no se apilan.
4. Quita del entorno copiado todas las claves listadas en `MITO_ACTIVE_VARS` del proceso padre (`:553-555`).
5. `proxy_environment(root, env)` de `network.py` (`:556`).
6. `env.update(context_variables(profile), MITO_HOST_PATH=inherited_path)` (`:557`). Las variables de contexto sobrescriben lo heredado.
7. `paths = [home/custom-commands, core/commands]` (`:558`), más `pocketools.paths(root)` (F10, `:559-560`).
8. Por cada componente en `selected(data)` (orden de `data['apps']`, `:562`):
   - Exige `payload/.mito.json` (`Falta el componente {p1}; vuelve a instalarlo`, `:565-566`).
   - Exige `marker.key == key`, `marker.manifest_sha256 == lock.manifest_sha256` y `fingerprint(lock.manifest) == lock.manifest_sha256` (`Lock o instalación incoherente`, `:568-569`).
   - `prepare_payload(root, lock, profile)` de `isolation.py` (`:570-571`).
   - `component_environment(lock, profile)` → `(component_vars, component_paths)` (`:572`).
   - `paths.extend(component_paths)`, `env.update(component_vars)`, `variables.extend(component_vars)` (`:573-575`).
9. Overrides: `global_env()` y luego `data['env']`, ambos con claves en mayúsculas (`:576-577`). El perfil gana al global. Para cada clave: borra del entorno toda variante con la misma clave en mayúsculas/minúsculas (`:580-582`), la asigna en mayúsculas (`:583`) y la añade a `variables` (`:584`).
10. `env['MITO_ACTIVE_VARS'] = ';'.join(dict.fromkeys(variables))` (`:585`).
11. `shim_dir = core/state/shims/<profile>`; `_write_shims(data, profile, shim_dir)` (`:586-587`).
12. `env['PATH'] = os.pathsep.join([shim_dir, *dict.fromkeys(paths), inherited_path])` (`:588`).

Duda: el orden de PATH que se pidió en la tarea (activación, `env_add_path`, shims, custom-commands, core/commands, pocketools) no coincide con el código. El orden real es: **shims del profile → `home/custom-commands` → `core/commands` → pocketools → por componente (en orden de `apps`): `env_add_path` y luego el directorio de cada `bin` → `PATH` heredado (o `MITO_HOST_PATH`)**. Se quita cualquier duplicado conservando el primero.

### `env_add_path` y `bin` en `component_environment` (`:460-472`)

- Cada valor de `array(manifest.env_add_path)` pasa por `component_path(payload, value)` (`:465`). `array()` acepta string o lista (test 7 y fixtures).
- Después, el directorio padre de cada ejecutable de `bins()` (`:466-467`).
- `env_set` (`:469-471`): sustituye `${dir}` y `$dir` por el payload y `${persist_dir}` y `$persist_dir` por `persist_root(root, lock, profile)` (de `isolation.py`). El orden de sustitución no colisiona: `$persist_dir` no contiene `$dir`.
- Las claves de `env_set` NO se pasan a mayúsculas ni se validan aquí. La compatibilidad (`HOME` en `env_set` no es `Compatible`, test 8) la decide `catalog.compatibility`.

### Variables de profile y globales (`:118-133`, `:102-116`)

`config.properties` de la raíz: solo líneas que empiezan por `env.` tras `strip()`. Ejemplo de los fixtures (test 12):

```
# env.IGNORED=no
env.JAVA_HOME=global-java
env.COMMON=a=b
env.EMPTY=
```

- `key` = texto entre `env.` y el primer `=`, sin espacios. Sin `=` → `Variable global sin =: {key}`.
- `value` = resto, con `strip()`. `a=b` queda como `a=b`; `EMPTY` queda como `""`.
- Claves repetidas (sensible a mayúsculas) → `Variable de entorno repetida`.

Validación común (`validate_env`, `:102-116`), aplicada a global y profile:
- Clave vacía, con `=`, con `;`, con NUL, o valor no string o con NUL → `Variable de entorno no válida: {key}`.
- Reservadas (comparación en mayúsculas): `PATH`, `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `HOMEDRIVE`, `HOMEPATH`, y cualquiera que empiece por `MITO_` → `Variable reservada por MITO: {key}`.
- Duplicadas sin distinguir mayúsculas (`A` y `a`) → `Variable de entorno repetida: {key}`.

### Variables `MITO_*` del motor

`MITO_PROFILE`, `MITO_ROOT`, `MITO_WORKSPACE` (de `context_variables`); `MITO_HOST_PATH` (`:557`); `MITO_ACTIVE_VARS` (`:585`). `MITO_COMMAND_ARG_<n>` lo genera `jobs.batch_command` para `.bat`/`.cmd` (§6).

### Limpieza al cambiar de profile

El motor no escribe en `os.environ` (test 9). El entorno nuevo sale de una copia. Lo que heredó de un profile anterior (variables listadas en `MITO_ACTIVE_VARS` del proceso padre) se quita (`:553-555`, test 24 y test 27). Un profile nunca apila PATH sobre el de otro profile (test 24).

### Shims (`_write_shims`, `:591-611`)

Para cada `bin` de cada componente seleccionado se escribe `core/state/shims/<profile>/<alias en minúsculas>.cmd`. Contenido exacto (`:605`, con la sustitución de `:607`):

```
@echo off
setlocal DisableDelayedExpansion
"%~dp0..\..\..\tools\python\python.exe" -B -I -X utf8 -m mito run --profile test --app test/tool --bin="tool" -- %*
```

- El intérprete es `core/tools/python/python.exe`: desde `core/state/shims/<profile>/` hay que subir tres niveles para llegar a `core/`. El comentario de `:606` lo dice; la cadena `..\..\..\..` original se reduce a tres en `:607`.
- `--app` es la clave `bucket/app`; `--bin` es el alias ya pasado por `command_name` (minúsculas, `%` escapado a `%%`, `:604`).
- `generated.json` (`:611`): lista ordenada de los `.cmd` escritos. En cada activación se borran los `.cmd` anteriores que ya no están en la lista (`:599-601`). El registro evita que un comando desactivado quede en PATH.
- Un fichero solo se reescribe si cambió su contenido (`:609-610`).
- `_write_shims` llama a `self.selected(data)` y a `bins()` sobre todos los componentes: un alias inválido en cualquier componente instalado rompe la activación de todos los profiles que lo usan.

## Pipeline de instalación

### Entradas y salida de `install`

`install(requested, profile=None, log=print, confirm_unverified=None, confirm_plan=None, manifests=None, pins=None)` (`:300-316`).
- `requested`: un nombre o lista de nombres (`bucket/app` o `app` si es único en catálogo).
- `manifests`: `{key: manifest}` para sustituir el manifiesto del catálogo (versión elegida).
- `pins`: `{key: pin}` para retener esas claves. Una versión ya fijada conserva su manifiesto instalado aunque el catálogo avance, también como dependencia (`:222-225`, `:241-243`).
- Devuelve `records` (dict `install_id → record`).

### Fases (en orden)

1. **Plan** (`installation_plan`, `:218-220`, bajo `mutation`; `_installation_plan`, `:222-298`):
   a. `initialize()`; `data = profile(profile)` o `{'apps': []}` si no hay profile (`:226-227`).
   b. `current = installed()` (`:231`).
   c. `resolve(name)` recursivo (`:235-259`): `catalog.resolve(name)` → `(key, manifest)`. Si `key` está en `manifests` usa ese manifiesto; si no y está instalado con pin, reutiliza el manifiesto instalado (`:237-243`).
   d. Hook `post_install`: si el manifiesto efectivo lo declara, se ignora y se registra `{key}: post_install omitido por la política general de MITO` (`:248-250`). Es el único hook que se revisa aquí.
   e. Dependencias circulares → `Dependencia circular: {key}` (`:244-245`). Compatibilidad distinta de `Compatible` → `{key}: {status}` (`:251-253`, sin traducir).
   f. Las dependencias se visitan antes que su dependiente (post-orden, `:255-259`).
   g. Para cada item del plan: `digest = fingerprint(manifest)`, ruta `core/apps/<bucket>/<app>`, registro, `records[key]`, y se añade a `data['apps']` en memoria (`:265-277`). No se guarda aún.
   h. `validate_update_layout(current[key], manifest)` para componentes ya instalados (`:278-281`, `components.py`).
   i. Para cada item cuyo digest no coincide con el instalado (`:285-286`): `resource_sources` (`bucket_resources.sources`, `:289`); fuentes HTTP(S) de recursos → entradas en `unverified` (`:290-292`); `download_hashes(array(url), hash)` → cada hash `None` añade `{key} {version} · descarga {n}` a `unverified` (`:293-295`).
   j. Devuelve `{records, current, data, unverified, resource_sources, profiles}` (`:296-298`).

   Duda: `download_hashes` se llama en la fase de plan (`:293`). Verificar si hace E/S de red para obtener hashes; si es así, el plan no es solo local.

2. **Confirmación del plan**: si `confirm_plan` devuelve falso → `Importación cancelada` (`:302-303`).

3. **Confirmación de descargas sin hash**: si `unverified` no está vacío y `confirm_unverified` falta o devuelve falso → `Instalación cancelada: descargas sin hash no confirmadas` (`:304-305`). Ocurre antes de cualquier descarga (test 18).

4. **Re-plan bajo lock**: `_installation_plan` se repite dentro de `mutation(root)` (`:306-307`). Si `current != prepared` → `STATE_CONFLICT: installation plan changed; retry and review again` (`:308-309`, sin traducir).

5. **Publicación del plan**: `components.publish_plan(self, records, current_installed, profile, data, log)` (`:310-311`). Según el nombre y el test 31, es quien llama a `_install_payload` por componente y guarda el profile. `_install_payload` no se llama desde `install` directamente.

   Duda: el cuerpo de `publish_plan` no se ha leído. Confirmar el orden (reemplazo de versiones anteriores, guardado del profile y rollback). Los tests 37-39 y el test 'entire_plan_is_prepared_before_any_replacement' de `test_engine.py` (ver §Casos de test) apuntan a que todos los payloads se preparan antes de reemplazar nada.

6. **Mensaje final**: con profile → `Profile {p1} preparado`; sin profile → `Componente instalado. Puedes activarlo en Inicio.` (`:312-315`).

### `_install_payload` (una instalación, `:318-440`)

Todo ocurre en `core/staging/<tmp>` creado con `TemporaryDirectory`. Si algo falla antes del `rename` (línea `:439`), el directorio temporal se borra con todo su contenido (descargas, `expanded-N`, `payload`). Test 19 comprueba que `core/staging` queda vacío tras un fallo. Ninguna carpeta de `core/apps` cambia hasta el `rename`.

Por cada URL `i` (`urls = array(m.url)`, `:324-325`; `hashes = download_hashes(urls, m.hash)`):

| Paso | Qué hace | Fase/log literal | Línea |
|---|---|---|---|
| Aviso sin hash | si `hash` es `None` | log `{key}: descarga sin hash declarado ({i}/{n})` | `:327-328` |
| Descarga | `download(url, stage/download-i, expected, root=root)` dentro de `step('install','download-and-hash')` | phase `DESCARGANDO` · `Descargando {key} {version} ({i}/{n})` | `:329-333` |
| Checkpoint | `operation.checkpoint()` | — | `:334` |
| Nombre | `download_filename(url)` (`transfer.py`) | — | `:339` |
| Posicionales | `positional(field)`: si el campo es lista debe tener una entrada por URL (`{field}: cantidad distinta de URL`) y usa la `i`-ésima o `.` ; si es escalar, lo usa en todas | — | `:340-346` |
| Inno | `selected = inno_root(extract_dir)` solo si `innosetup` (F7) | — | `:350` |
| Extracción | `extract(archive, expanded-i, root, filename, log, innosetup, inno_root)` dentro de `step('install','extract')` | phase `PREPARANDO ARCHIVOS` · `Extrayendo {key}…` | `:351-354` |
| Origen | `source = inside(expanded, extract_dir)`; si no es directorio → `extract_dir no encontrado: {nombre}` | — | `:358-361` |
| Destino | `target = inside(payload, extract_to)` | — | `:359` |
| Log | `Preparando {key}` | — | `:362` |
| Colisión | si un archivo del origen ya existe en el destino → `Dos artefactos intentan escribir el mismo archivo` (también entre URLs) | — | `:363-368` |
| Copia | `move_whole(source, target, wait=min(30, 0.15*total))`; si no se puede, `copytree(..., copy_function=copy_file)` con `os.replace` y fallback a `copy2` | phase `PREPARANDO ARCHIVOS` · `Moviendo {total} archivos de {key} a su carpeta. Con muchos archivos este paso puede tardar varios minutos; MITO sigue trabajando.` y log `Archivos {key}: {copied}/{total}` | `:387-407` |

Después del bucle de URLs:

| Paso | Qué hace | Literal | Línea |
|---|---|---|---|
| Restaurar objetivos Inno | `restore_declared_targets(payload, shortcuts + bins)` solo si `innosetup` (F7) | — | `:409-412` |
| Recursos del catálogo | `prepare_resources(root, hook_sources(...), stage/hook-buckets, log)` | phase `PREPARANDO ARCHIVOS` · `Preparando {key}` | `:414-417` |
| pre_install | `run_pre_install(root, key, m, payload, log)` en `step('install','pre-install')` (F7) | — (lo registra `hooks.py`) | `:418-420` |
| installer | `run_installer(root, key, m, payload, log)` en `step('install','installer')` (F7) | — (lo registra `hooks.py`) | `:421-423` |
| Validación | log `Validando {key}`; cada `bin.executable` debe ser fichero dentro del payload, si no → `Comando ausente: {executable}` | log `Validando {key}` | `:424-429` |
| Marcador | escribe `payload/.mito.json` | — | `:430-431` |
| Destino | `mkdir` del padre; si `destination` ya existe → `Instalación incompleta existente: {destination}` | — | `:432-434` |
| Publicación | log `Publicando {key}; el antivirus puede retrasar este paso con muchos archivos`; `payload.rename(destination)` en `step('install','publish')` | — | `:435-439` |
| Final | log `Instalado {key} {version}` | — | `:440` |

Notas:
- Los puntos de cancelación de `operation.checkpoint()` están en `:334`, `:355`, `:385` (cada 50 archivos), `:408`, `:417`, `:420`, `:423`.
- Los pasos de diagnóstico `step(...)` tienen nombres: `download-and-hash`, `extract`, `copy-payload`, `pre-install`, `installer`, `validate`, `publish`.
- `move_whole` (`:19-54`): un solo `os.replace` del directorio si el destino no existe o está vacío (si es junction, no vacío o no directorio, devuelve `False` con `target-not-empty`). Reintenta `PermissionError` (antivirus) durante `wait` segundos con espera que se duplica hasta 2 s. Si falla, crea el destino y devuelve `False`. Los datos del intento van a `diagnostics.write('install.move-whole', ...)` (`:399-401`).
- Dependencias de `move_whole`: el test 1 comprueba que no se copia (`copy2` lanza `AssertionError`).

### Rollback y staging

- Lo que se conserva hasta publicar: solo el directorio temporal de `core/staging/<tmp>` (descargas, extracciones, payload, `hook-buckets`). Se borra en cualquier salida, incluida la excepción.
- El estado de `core/apps` y del profile no cambia hasta `publish_plan` (ver paso 5 del plan).
- Un payload ya publicado solo se borra o reemplaza fuera de este módulo (`components.py`).

### Sustitución de un manifiesto distinto del mismo bucket/app

- La clave es `bucket/app`; la ruta es `core/apps/<bucket>/<app>`. Un manifiesto nuevo para la misma clave se detecta en el plan porque `fingerprint` cambia (`:285-286`).
- Si ya está instalado, `validate_update_layout(current, manifest)` se ejecuta antes de cualquier descarga (`:278-281`).
- Si cambió el manifiesto sin `hash`, la descarga se marca como `unverified` (`:293-295`). El test equivalente del original (`test_manifest_change_replaces_same_version_and_unsigned_update_requires_confirmation`) está en `test_stable_storage.py`, no en `test_engine.py`.
- Si el payload nuevo ya tiene destino ocupado, falla con `Instalación incompleta existente` (`:433-434`). La sustitución limpia el destino en `publish_plan`, no aquí (Duda).

## Activar, desactivar, installed, resolve, eliminación

- **`installed()`**: ver §2. Es la única fuente de verdad del inventario.
- **`selected(data)`** (`:152-157`): devuelve un dict en el orden de `data['apps']`. Ese orden es el orden de activación y, por tanto, de PATH (ver §3).
- **`set_active(profile, install_id, active)`** (`:159-172`), bajo `mutation`:
  - Activar: el componente debe existir en `installed()` (`El componente no está instalado`); se añade al final de `apps` si no está.
  - Desactivar: se quitan todas sus apariciones. No exige que esté instalado, así que el test 15 puede desactivar una entrada rota (`missing/app@123`).
  - Guarda el profile y llama `terminal.prepare(self)` (F5, `:171-172`).
  - No llama a `environment()`: la activación real se valida al lanzar.
- **`resolve`**: no existe como método público. La resolución nombre → `(key, manifest)` la hace `catalog.resolve` (`catalog.py`) y la envuelve la función anidada `resolve` de `_installation_plan` (`:235`). Ambigüedad entre buckets → `ambiguo` (test 26).
- **Eliminación de componentes**: `engine.py` no tiene función de borrado. Está en `components.remove(engine, installation, log)` (`components.py:80`), con `check_unused(folder, removing)` (`components.py:17`) y `users(...)` (`components.py:69`). Duda: no se ha leído `components.py`. Documentar en su contrato.

## Lanzamiento

Firma de `launch` (`:613-614`): `launch(profile, app=None, binary=None, arguments=(), new_terminal=False, coupled=False, owned=False, shortcut=None)`.

### Validaciones iniciales
- `shortcut` y `binary` a la vez → `Modo de lanzamiento no válido` (`:615-616`).
- `env = self.environment(profile)` (bajo lock, activa y escribe shims) y `workspace.mkdir` (`:617-620`).

Duda: `launch` lee `profile(profile)` y `environment(profile)` por separado, no a partir de un mismo `execution_context`. Un cambio de profile entre las dos lecturas puede mezclar datos.

### Sin `app`
- Se delega en `terminal.launch(self, profile)` (F5, `:658-659`).

### Con `app`
1. Selección (`:622-626`): busca en `selected(data)` coincidencias por `key`, por `lock.key` o por la segunda parte (`app`). Debe haber exactamente una, si no → `{app} no está seleccionado en {profile}`.
2. Con `binary` (`:630-635`): busca entre `bins()` por alias sin distinguir mayúsculas. Exactamente una coincidencia, si no → `Comando no encontrado: {binary}`. Toma `executable` y `fixed_args` del `bin`.
3. Sin `binary` (`:637`): `application_target(lock, shortcut)` → `(executable, fixed_args)`.
4. `target = component_path(payload, executable)`; debe ser fichero (`No existe {executable}`, `:640-642`).
5. `fixed_args` sustituye `$dir` y `$persist_dir` (`:643`).
6. `command = [target, *fixed_args, *arguments]` (`:644`), con excepciones por extensión (minúsculas):
   - `.bat`/`.cmd`: `jobs.batch_command(target, [*fixed, *args], env)` (`:645-647`). Los argumentos viajan en variables `MITO_COMMAND_ARG_<n>` (test 28: índice 0 = script, después los argumentos ya partidos).
   - `.ps1`: `powershell.exe -NoLogo -NoProfile -File <target> ...` (`:648-649`).
   - `.chm`: `hh.exe` de `%SystemRoot%` (por defecto `C:\Windows`) con el `.chm` (`:650-651`).
   - `.jar`: `java.exe` localizado con `shutil.which('java.exe', path=env.PATH)`; si no existe → `Comando no encontrado: java.exe` (`:652-656`). Comando: `java -jar <target> ...`.

### Modos (`:660-690`)
- `coupled` (`:660-664`): solo sin `binary`, `new_terminal` ni `owned` (si no → `Modo de lanzamiento no válido`); delega en `terminal.launch_component(self, profile, key, application_name(lock, shortcut), arguments, shortcut=shortcut)` (F5).
- Directorio de trabajo (`:668-673`): por defecto `workspace`. Con `binary` (consola/shim) es `os.getcwd()` y, si falla, el workspace. Test 32 lo fija: `git init` en la raíz actúa en la raíz.
- `owned` (`:674-683`): `processes.ProcessJob().spawn(command, cwd, env)`, `wait()`, `wait_empty()`, `close()`; devuelve el código de salida.
- `new_terminal` (`:684-686`): `Popen(..., creationflags=CREATE_NEW_CONSOLE)`; devuelve `0` sin esperar.
- `app` sin `binary` (`:687-689`): `Popen(command, cwd, env)`; devuelve `0` sin esperar.
- Resto (`:690`): `subprocess.call(command, cwd, env)`; devuelve el código de salida.

### Selección de la aplicación (`application_entries` / `application_target`)

- `candidates` (`:477-479`): primero los `shortcuts` del manifiesto (`[executable, label, args, icon]`, `:477`), después los `bin` cuyo `(path, args)` no esté ya en shortcuts (`:478-479`), con etiqueta = alias.
- Una entrada solo cuenta si su extensión está en `.exe .com .cmd .bat .ps1 .jar .chm` y el fichero existe (`:483`).
- `application_target` (`:487-498`):
  - Con `shortcut`: exactamente una entrada con esa etiqueta, si no → `Shortcut unavailable or ambiguous: {shortcut}` (sin traducir).
  - Sin `shortcut`: ordena para que los `.chm` vayan los últimos (`:495`).
  - Sin entradas → `La app no declara un ejecutable disponible; úsala desde la terminal`.
- `application_name` (`:500-510`): con `shortcut` devuelve la etiqueta; si no, la etiqueta del shortcut cuyo ejecutable coincide, o la última parte de la clave.
- Regla de la tarea "nunca escanear runtimes": `:488` lo confirma; solo se usan entradas declaradas.

Duda: `bin` con `[executable, alias, args]` declara `args` como cadena partida por `arguments.split`. Los shortcuts también. Si el alias `tool` del `bin` aparece también en `shortcuts` con otros args, se listan ambos.

### Consola y shell (`shell`, `:692-703`)
- `shell_type = 'powershell'`: `System32\WindowsPowerShell\v1.0\powershell.exe -NoLogo -NoProfile -NoExit`.
- `shell_type = 'cmd'`: `%COMSPEC%` (por defecto `cmd.exe`) `/d /k`.
- Otro valor → `Shell no soportada`.
- Con `arguments`, esos argumentos sustituyen al shell (`:703`).
- `cwd = workspace` (creado si falta). Síncrono (`subprocess.call`).

## Puntos donde engine llama a otros módulos

| Módulo / función | Llamada (línea) | Momento | Fase |
|---|---|---|---|
| `hooks.run_pre_install(root, key, manifest, payload, log)` | `:419` | Tras el pipeline de extracción y antes del installer | F7 |
| `hooks.run_installer(root, key, manifest, payload, log)` | `:422` | Tras `pre_install` y antes de validar `bin` | F7 |
| `bucket_resources.sources(catalog, key, manifest)` | `:289`, `:414` | Plan (`:289`) y antes de preparar | — (catálogo) |
| `bucket_resources.prepare(root, origins, destination, log)` | `:416` | Tras la extracción, antes de `pre_install` | — |
| `inno.inno_root(extract_dir)` | `:350` | Antes de extraer, solo si `innosetup` | F7 |
| `archives.extract(archive, expanded, root, filename, log, innosetup, inno_root)` | `:353` | Extracción de cada URL. Aquí se decide si entra MSI/Inno/Burn (no leído) | F7 (Duda) |
| `inno.restore_declared_targets(payload, targets)` | `:410-412` | Tras copiar, solo `innosetup` | F7 |
| `msi`, `burn` | — (no se llaman desde `engine.py`) | Duda: si se invocan desde `archives.extract` o `hooks` | F7 |
| `pocketools.paths(root)` | `:560` | Construcción del PATH (tras `core/commands`) | F10 |
| `terminal.prepare(engine)` | `:172`, `:187`, `:215` | Tras `set_active`, `create_profile`, `delete_profile` | F5 |
| `terminal.launch(engine, profile)` | `:658-659` | `launch` sin `app` | F5 |
| `terminal.launch_component(engine, profile, key, title, args, shortcut=)` | `:663-664` | `launch(coupled=True)` | F5 |
| `isolation.prepare_payload(root, lock, profile)` | `:571` | Dentro de `_environment`, por componente | Duda (aislamiento por profile) |
| `isolation.payload_path(root, lock, profile)` | `:463`, `:639` | Entorno por componente y lanzamiento | Duda (`core/profile-apps`) |
| `isolation.persist_root(root, lock, profile)` | `:468`, `:643` | `$persist_dir` | Duda |
| `open_with.unregister(root, predicate)` | `:211-213` | `delete_profile` | — |
| `network.proxy_environment(root, env)` | `:556` | Dentro de `_environment`, antes del contexto | — |
| `jobs.batch_command(target, args, env)` | `:647` | Lanzamiento de `.bat`/`.cmd` | — |
| `processes.ProcessJob` | `:675-683` | `launch(owned=True)` | — |
| `components.publish_plan(engine, records, current, profile, data, log)` | `:310-311` | Último paso de `install` | — (Duda) |
| `components.validate_update_layout(old, manifest)` | `:278-281` | Plan | — |
| `catalog.resolve`, `compatibility`, `fingerprint`, `effective`, `array` | `:236-293`, `:325` | Plan, descargas, activación | — |
| `transfer.download`, `download_hashes`, `download_filename` | `:293`, `:325`, `:333`, `:339` | Plan y descarga | — |
| `diagnostics.step`, `diagnostics.write` | `:332`, `:399` y otros | Todos los pasos | — |
| `operation.phase`, `transfer`, `checkpoint` | ver tablas | Todos los pasos | — |

**Sin llamadas desde `engine.py`:**
- F8 (shortcuts/iconos): `engine.py` solo lee `shortcuts` (`:477`, `:505-507`) para elegir entradas. No crea `.lnk` ni copia iconos. El test 32 comprueba que el `.ico` declarado llega intacto al payload.
- F11 (relocation): `relocation.py` existe, pero `engine.py` no lo importa. El test 23 solo copia el árbol y construye `Engine(moved)`.

## Invariantes

1. Un profile referencia solo instalaciones presentes en `core/apps` con `.mito.json`; si falta alguna, `selected` falla (`:152-157`).
2. `apps` no tiene duplicados y su orden define el orden de activación y de PATH (`:95-96`, `:157`, `:588`).
3. Nunca se borra el último profile (`:207-208`).
4. Ningún payload parcial llega a `core/apps`: el único paso que toca el destino es `rename` (`:439`), y solo después de validar `bin` y escribir `.mito.json`.
5. Un componente activo cumple `marker.key == key` y `fingerprint(manifest) == manifest_sha256` (`:568-569`). Si no, la activación falla (test 47).
6. `environment()` no modifica `os.environ` (test 9 y test 10).
7. El PATH no se apila entre profiles: el padre se identifica por `MITO_HOST_PATH` y `MITO_ACTIVE_VARS` (`:552-555`, test 24).
8. Las variables de profile y globales no pueden usar nombres reservados ni claves repetidas sin distinguir mayúsculas (`:102-116`).
9. Un lanzamiento solo usa entradas declaradas en `bin` o `shortcuts`, con extensión de la lista blanca (`:477-483`, `:498`).
10. `context_variables` no crea ficheros ni activa nada (`:524`, test 11).
11. El plan completo (incluidas dependencias y validaciones) se calcula y valida antes de descargar nada (`:263-298`, test 20-21, 18).
12. Un `bin` con alias inválido se rechaza en `bins()` (`:451`); un alias ya instalado que se vuelve inválido rompe la activación.

## Casos límite y errores

- Profile sin `workspace` → `KeyError` (`:98`). Duda: debería ser `YitoError`.
- `apps` con un id desconocido → `Falta la instalación: <ids>` (`:154-156`).
- `set_active(..., True)` con id no instalado → `El componente no está instalado` (`:164-165`). Con `False` siempre funciona (test 15).
- `install` con `profile=None` no modifica ningún profile; devuelve `records`.
- `install` sin confirmación → `cancelada` antes de cualquier descarga (test 18).
- Plan cambiado entre el plan y el lock → `STATE_CONFLICT` (`:308-309`).
- `post_install` nunca se ejecuta; solo se registra en el log (`:248-250`, test 31).
- Colisión de ficheros entre URLs → `Dos artefactos intentan escribir el mismo archivo` (`:363-368`).
- Archivo `bin` ausente tras instalar → `Comando ausente: {executable}` (`:428-429`).
- Destino existente → `Instalación incompleta existente` (`:433-434`).
- `launch(shortcut=..., binary=...)` → `Modo de lanzamiento no válido` (`:615-616`).
- Atajo ambiguo → `Shortcut unavailable or ambiguous` (`:492-493`, sin traducir).
- `.jar` sin `java.exe` en el PATH del profile → `Comando no encontrado: java.exe` (`:653-655`, test 32).
- `binary` con `os.getcwd()` que falla → workspace (`:671-673`).
- `shell(arguments=...)` sustituye el shell por el comando (`:703`).
- Borrado de profile con copia de seguridad previa → sobrescrita (Duda, §1).
- `ensure_default_profile` concurrente → `None` si otro proceso lo creó (`:197-201`).

## Casos de test del original

Tests de `test_engine.py` (47). "Fase" indica la dependencia: "ninguna" si solo usa `engine`/`catalog`/`storage`/`transfer`; "F5 (mock)" si el test parchea `mito.terminal.prepare` pero no depende de la funcionalidad F5; "F7" si depende de hooks/installer/pre_install/Inno/MSI/7-zip; "F8" si depende de shortcuts/iconos; "F10" pocketools; "F11" relocation.

| # | Test | Qué comprueba | Fase |
|---|---|---|---|
| 1 | `test_install_moves_extracted_files_instead_of_copying_them` | Instalar mueve (no copia) los ficheros extraídos: `copy2` lanza si se llama y `bin/tool.exe` existe. | F5 (mock) |
| 2 | `test_mutation_wait_retries_a_busy_lock_and_default_fails_cleanly` | Sin espera, lock ocupado → `YitoError`; con `wait=5` espera a que se libere. | ninguna |
| 3 | `test_portable_home_has_windows_known_user_folders` | Tras borrar `home/`, `environment()` la recrea con `Desktop`, `Documents`, `Downloads`, etc. | ninguna |
| 4 | `test_default_profile_only_when_instance_has_none` | `ensure_default_profile` solo crea `default` si no hay profiles; contenido `{workspace: default, apps: [], env: {}}`. | F5 (mock) |
| 5 | `test_paths_reject_escape_and_windows_special_names` | `inside()` rechaza `..`, rutas absolutas, `:`, nombres reservados y punto/espacio final. | ninguna |
| 6 | `test_zip_traversal_rejected_before_any_extraction` | Un ZIP con `../escape` se rechaza sin extraer nada. | ninguna |
| 7 | `test_architecture_overrides_common_fields` | `effective()` sustituye `url` por la de `architecture.64bit` y conserva `env_set`. | ninguna |
| 8 | `test_scripts_and_persist_not_silently_ignored` | `installer.file` y `env_set.HOME` no son `Compatible` en el catálogo. | F7 |
| 9 | `test_verified_install_and_relative_lock` | Instalación verificada: `lock.path` es relativo; `JAVA_HOME` apunta al payload; `environment()` no toca `os.environ`. | F5 (mock) |
| 10 | `test_profile_env_overrides_components_without_leaking_to_next_profile` | `env` del perfil gana a `JAVA_HOME` del componente (`java_home` → `JAVA_HOME`); no se filtra a otro profile. | F5 (mock) |
| 11 | `test_workspace_variable_follows_profile_context_and_cannot_be_overridden` | `MITO_WORKSPACE` sigue al profile; `context_variables` no activa ni crea carpeta; `mito_workspace` como `env` es inválido. | ninguna |
| 12 | `test_global_env_precedence_reload_and_cleanup` | `config.properties`: comentarios ignorados, `a=b` se conserva, vacío válido, perfil gana a global, cambios se recargan, limpieza entre profiles. | F5 (mock) |
| 13 | `test_global_env_validation` | `env.PATH`, `env.MITO_ROOT`, claves repetidas, `env.MISSING` y `env.=x` → error. | ninguna |
| 14 | `test_profile_env_validation` | `HOME`, `Path`, `MITO_ROOT`, valor no string, claves duplicadas sin distinguir mayúsculas y `BAD=NAME` → error. | ninguna |
| 15 | `test_missing_selection_is_reported_and_can_be_deactivated` | App inexistente en `apps` → `Falta la instalación`; `set_active(False)` la quita igualmente. | F5 (mock) |
| 16 | `test_command_names_and_generated_batch_preserve_literal_aliases` | Alias con espacios, `&`, `%`, `!`: shim se ejecuta con `cmd /v:on` y pasa `--bin=<alias en minúsculas>`; alias inválidos rechazados. | F5 (mock) |
| 17 | `test_multiple_downloads_without_hash_are_all_extracted` | Dos URLs sin hash (con confirmación) se extraen ambas al payload. | ninguna |
| 18 | `test_unverified_dependency_cancelled_before_any_download` | Dependencia sin hash sin confirmar (`None` o `False`) cancela antes de cualquier descarga; ni profile ni instalaciones cambian. | ninguna |
| 19 | `test_failure_preserves_profile` | Error de descarga no cambia el profile y deja `core/staging` vacío. | ninguna |
| 20 | `test_dependency_cycle_preflight` | Dependencia circular → `circular` antes de descargar. | ninguna |
| 21 | `test_dependency_incompatibility_preflight` | Dependencia con `installer.file` → error antes de descargar. | F7 |
| 22 | `test_multiple_runtimes_can_be_active` | Dos componentes activos a la vez en el mismo profile. | F5 (mock) |
| 23 | `test_moving_root_rebuilds_environment` | Copiar la raíz a otra carpeta y reconstruir el entorno da rutas bajo la nueva raíz. No usa `relocation.py`. | F11 (no invocado) |
| 24 | `test_nested_profiles_do_not_stack_path` | Con `MITO_HOST_PATH`, PATH del padre y `MITO_ACTIVE_VARS` previos, el nuevo PATH no contiene el del profile anterior y `JAVA_HOME` antiguo desaparece. | ninguna |
| 25 | `test_hash_mismatch_discards_download` | Descarga con hash incorrecto → `Hash incorrecto` y el destino no existe. | ninguna |
| 26 | `test_ambiguous_bucket_requires_namespace` | Mismo nombre en dos buckets → `resolve('tool')` falla con `ambiguo`. | ninguna |
| 27 | `test_vscode_uses_generic_persistence_and_launch` | Componente con `persist`: `HOME` igual en dos profiles, `VSCODE_PORTABLE` antiguo se quita, la persistencia apunta a `home/persist/...`, `launch` ejecuta `bin/tool.exe`. | ninguna (persistencia) |
| 28 | `test_batch_launch_appends_user_arguments_after_manifest_arguments` | `.cmd` con `fixed "two words"` y args de usuario `user`, `a & b`: `MITO_COMMAND_ARG_0..4` en ese orden. | ninguna |
| 29 | `test_batch_simple_subcommand_matches_raw_positional_argument` | `batch_command` pasa `run` a un `.cmd` que lo compara con `%1` y devuelve 0. | ninguna |
| 30 | `test_no_component_exemptions_for_hooks` | Ningún componente del catálogo queda exento de `installer.file` (`Pasos no soportados: installer.file`). | F7 |
| 31 | `test_pre_install_runs_and_post_install_is_ignored_for_every_component` | `pre_install` se ejecuta (crea `prepared.txt`), `post_install` no y deja aviso en el log. | F7 |
| 32 | `test_jar_and_icon_install_preserves_bytes_and_launches_profile_java` | `.jar` e `.ico` se conservan byte a byte; `.cmd` del shortcut; `java.exe` del profile; cwd del caller; error sin `java.exe`. | F7, F8 |
| 33 | `test_declared_shortcut_icons_are_relative_supported_files` | Iconos fuera del payload, con ruta absoluta, `.dll,0`, `.png` o con formato de dict → no `Compatible`. | F8 |
| 34 | `test_installer_script_nested_archive_order_context_and_architecture` | `installer.script` con `architecture.64bit`: se ejecuta después de `pre_install`, antes de `Validando`; `$fname`, `$architecture`, `$global`; extrae 7z anidado y borra `$PLUGINSDIR`. | F7 |
| 35 | `test_installer_runner_exposes_generic_inno_helpers` | El script de `hooks.SCRIPT` define `Expand-InnoArchive`, `Get-HelperPath`, `Invoke-ExternalCommand`, y llama `python -m mito.hooks`. | F7 |
| 36 | `test_installer_unsupported_fields_and_malformed_scripts_fail_before_download` | `installer` no soportado o malformado falla antes de descargar; `{}` y `{script: ''}` sí son compatibles. | F7 |
| 37 | `test_installer_failure_preserves_installed_version_and_profile` | Un `installer.script` que modifica, borra o crea junction en el payload falla sin tocar la versión instalada ni el profile. | F7 |
| 38 | `test_pre_install_failure_prevents_installer_script` | Si `pre_install` falla, `run_installer` no se llama (`pre_install falló`). | F7 |
| 39 | `test_pre_install_failure_does_not_publish_or_change_profile` | Fallo de `pre_install` (lista con `throw`) → error con el mensaje, sin publicar ni cambiar profile. | F7 |
| 40 | `test_extensionless_download_renamed_by_real_pre_install` | URL sin extensión: `$fname` nombra la descarga; `pre_install` la renombra a `tool.exe`; el original no queda. | F7 |
| 41 | `test_pre_install_multiple_download_names_and_extract_to` | Dos URLs con `extract_to` por posición y `$fname` como lista; `pre_install` copia cada una. | F7 |
| 42 | `test_extensionless_zip_is_extracted_and_traversal_still_rejected` | Un ZIP sin extensión se extrae; un ZIP con `../escape` falla y no deja `core/apps/test/bad`. | ninguna |
| 43 | `test_download_names_are_shared_and_confined` | `download_filename` decodifica `%2E` y rechaza nombres con `..`, `NUL`, `\`, `?`, `:` o punto final; `.iso` no es `Compatible`. | ninguna |
| 44 | `test_pre_install_nested_archive_helper` | `Expand-7zipArchive` desde `pre_install` extrae el 7z anidado; `expand` fuera del payload falla con `dentro del payload`. | F7 |
| 45 | `test_installer_script_pipes_archives_into_expand_helper` | `Get-Item ... | Expand-7zipArchive` desde `installer.script`; el `$PLUGINSDIR` y `$R0` no quedan. | F7 |
| 46 | `test_pre_install_persist_scope_and_architecture_override` | `$persist_dir`: `shared` (sin shortcuts) copia `saved.txt`; `isolated` (con shortcuts) no; `$env:USERPROFILE` es `home`. `architecture` sustituye `pre_install`. | F7 (persistencia en `isolation`) |
| 47 | `test_altered_lock_is_rejected_on_activation` | Cambiar `env_set` en el `.mito.json` → `environment()` falla con `incoherente`. | ninguna |

Dependencia de fase, resumen: F5 (mock) en 1, 4, 9, 10, 12, 15, 16, 22. F7 en 8, 21, 30-32, 34-41, 44-46. F8 en 32, 33. F10 ninguno. F11 solo en 23 (sin invocación).

## Dependencias

- Internas (otros módulos del original): `catalog` (`Catalog`, `array`, `effective`, `fingerprint`), `storage` (`YitoError`, `identifier`, `command_name`, `inside`, `mutation`, `read_json`, `write_json`), `transfer` (`download`, `extract_zip`, `download_hashes`, `download_filename`), `diagnostics` (`step`, `write`), `operation` (`phase`, `transfer`, `checkpoint`), `i18n` (`tr`), `components` (`validate_update_layout`, `publish_plan`, `remove`), `bucket_resources`, `hooks` (`run_pre_install`, `run_installer`, `SCRIPT`, `expand`), `inno` (`APP`, `inno_root`, `restore_declared_targets`), `archives` (`extract`), `isolation` (`payload_path`, `persist_root`, `prepare_payload`, `isolated`), `terminal` (`prepare`, `launch`, `launch_component`), `pocketools` (`paths`), `open_with` (`unregister`), `network` (`proxy_environment`), `jobs` (`batch_command`), `processes` (`ProcessJob`), `arguments` (`split`).
- Biblioteca estándar: `os`, `shutil`, `subprocess`, `tempfile`, `time`, `zipfile`, `pathlib`.
- Dependientes del original: `mito/__main__.py`, la UI, el MCP y `terminal`, entre otros. Duda: no se han inventariado.

## Notas para Rust

### División propuesta de la API en submódulos

| Submódulo | Responsabilidad | Funciones principales (entradas → salida) |
|---|---|---|
| `profiles` | Lectura/escritura y validación de `profiles/<name>.json`, creación, default y borrado | `list(root)`; `load(root, name) → Profile`; `save(root, name, &Profile)`; `create(root, name, Option<workspace>) → name`; `ensure_default(root) → Option<name>`; `delete(root, name) → name_restante`; `validate_env(&env)` |
| `inventory` | Descubrimiento de `core/apps/*/*/.mito.json`, validación de layout, selección por profile | `installed(root) → Map<InstallId, Installation>`; `selected(&Profile, &inventory) → Map`; `set_active(root, profile, id, active)` |
| `environment` | Construcción del entorno, contexto, shims y PATH | `context_variables(root, profile) → Map`; `build(root, profile) → Environment{vars, path}`; `component_environment(&lock, profile) → (vars, paths)`; `write_shims(root, &Profile, dir)` |
| `install` | Plan, confirmaciones, descarga, extracción, validación y publicación | `plan(root, requested, Option<profile>, &Overrides, &Pins, log) → Plan`; `install(root, requested, Option<profile>, confirm_unverified, confirm_plan, &Overrides, &Pins, log) → Map`; `install_payload(root, key, &manifest, destination, log) → Result<()>` |
| `launch` | Lanzamiento de app, binario, `.bat`/`.cmd`/`.ps1`/`.chm`/`.jar`, shell y terminal | `launch(root, profile, LaunchOptions) → exit_code`; `shell(root, profile, args, ShellKind) → exit_code`; `application_target(&lock, Option<shortcut>) → (exe, args)` |

`Engine` puede quedar como fachada con estado `root` y `catalog`, con los mismos nombres de método que el original.

### Firmas principales a respetar (pseudo-firmas)

- `Engine::new(root: Path) -> Engine`, resuelve la ruta.
- `Engine::profile(name) -> Result<Profile, YitoError>`, con `workspace: String`, `apps: Vec<String>` (sin repetidos) y `env: Map<String, String>` ordenado (`IndexMap`), porque el orden se escribe a disco.
- `Engine::installed() -> Result<Map<String, Lock>, YitoError>`. `Lock` = `record` + `path` (ruta relativa con `/`).
- `Engine::environment(profile) -> Result<Map<String,String>, YitoError>` (con lock de `mutation`).
- `Engine::context_variables(profile) -> Result<Map<String,String>, YitoError>` (sin efectos).
- `Engine::execution_context(profile) -> Result<(Map, PathBuf), YitoError>`.
- `Engine::install(requested, profile?, log, confirm_unverified?, confirm_plan?, manifests, pins) -> Result<Map<String, Record>, YitoError>`. En Rust: callbacks `Fn(&str)` para log y `Fn(&[Unverified]) -> bool` para confirmación.
- `Engine::launch(profile, app?, binary?, args, new_terminal, coupled, owned, shortcut?) -> Result<i32, YitoError>`.
- `Engine::shell(profile, args, ShellKind::{Cmd, Powershell}) -> Result<i32, YitoError>`.
- `move_whole(source, target, wait: Duration, clock, sleep) -> Result<bool, io::Error>`.

### Diferencias inevitables

- Python `repr` frente a `{:?}` en mensajes (`storage.md`). Aquí se usan mensajes literales en el mismo idioma y formato.
- `os.replace` y `payload.rename` sobre Windows fallan si el destino existe. En Rust, `std::fs::rename` tiene el mismo comportamiento. Ojo al reemplazo de directorios.
- Los `subprocess.call`/`Popen` con `CREATE_NEW_CONSOLE` requieren `std::os::windows::process::CommandExt::creation_flags`.
- Los errores untranslated (`STATE_CONFLICT`, `Shortcut unavailable...`, `{key}: {status}`) deben conservarse tal cual. No pasan por `tr()`.
- El PATH del entorno debe construirse con `;` (`std::env::join_paths` o equivalente).
- La variable `MITO_ACTIVE_VARS` usa `;` como separador; si una clave tuviera `;`, `validate_env` la rechaza.

### Dudas abiertas

1. ¿`write_json` crea el directorio padre? `core/state/deleted-profiles/` no lo crea `initialize` (`:80`, `:209`).
2. ¿`records[...].pinned` se reescribe en `.mito.json` durante `publish_plan`? En disco no aparece (`:430`).
3. ¿`download_hashes` hace E/S de red en el plan (`:293`)?
4. ¿Qué hace `components.publish_plan` (orden, reemplazo, rollback y guardado del profile)?
5. ¿Dónde entran MSI, Burn e Inno (`archives.extract`, `hooks`, `msi.py`, `burn.py`)?
6. ¿`isolation` limpia `core/profile-apps` o `home/persist/<profile>` al borrar un profile? Y ¿qué regla decide "shortcuts + persist = copia por profile" (test 46)?
7. `launch` lee profile y environment en dos llamadas (`:617-618`). ¿Usar `execution_context` evitaría la mezcla?
8. `profile()` con `workspace` ausente produce `KeyError`, no `YitoError` (`:98`).
9. `set_active` no llama `environment()`: un componente roto se activa sin error (`:159-172`).
10. ¿Los tests con `patch('mito.terminal.prepare')` ocultan algo de F5 que deba documentarse, como el estado del terminal tras activar?
