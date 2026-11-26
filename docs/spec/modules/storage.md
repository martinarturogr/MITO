# storage.py → mito-core::storage

Origen: `core/app/mito/storage.py` (114 líneas), `core/tests/test_stable_storage.py` (7 tests).

## Responsabilidad
Validación de nombres e identificadores, validación de rutas relativas portables (estilo Windows), contención de rutas dentro de una raíz, lectura/escritura JSON atómica, lock de exclusión mutua entre procesos (`mutation`) y localización de la raíz del proyecto. Define el error de dominio `YitoError`.

## API pública del original
| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `class YitoError(Exception)` (`storage.py:12`) | mensaje | — | Error de dominio. Los mensajes de usuario se generan con `tr()`. |
| `identifier(value: str) -> str` (`:16-21`) | cadena | cadena en minúsculas | `YitoError` si no cumple `[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}` (`:17`), si termina en `.` o si el primer segmento (antes del primer `.`, sin distinguir mayúsculas) es reservado (`:19`). |
| `portable_path(relative: str) -> PureWindowsPath` (`:24-32`) | ruta relativa | `PureWindowsPath` | `YitoError` (ver reglas abajo). No consulta el sistema de ficheros. |
| `inside(root: Path, relative: str) -> Path` (`:35-40`) | raíz, ruta relativa | `root` + partes | `YitoError` si la ruta no es portable o si `resolve()` queda fuera de `root.resolve()`. |
| `command_name(value: str) -> str` (`:43-52`) | nombre de comando | cadena en minúsculas | `YitoError` (ver reglas abajo). |
| `read_json(path: Path)` (`:55-56`) | ruta | valor JSON | `OSError`, `ValueError` de `json` (no se envuelven en `YitoError`). |
| `write_json(path: Path, value)` (`:59-70`) | ruta, valor | — | `OSError`. |
| `mutation(root: Path, name='mutation.lock', wait=0.0)` (`:73-110`) | raíz, nombre de lock, segundos de espera | context manager | `YitoError` (Windows, ocupado tras `wait`); `OSError` (Unix, ocupado). |
| `root_path() -> Path` (`:113-114`) | — | `Path(__file__).resolve().parents[3]` | — |

### Reglas de `identifier` (`storage.py:16-21`)
- Mensaje de error con valor entre comillas simples tipo `repr`: `Nombre no válido: 'bad name'` (`:18`, `p1=(f'{value!r}')`).
- Reservados: `CON, PRN, AUX, NUL, COM0..COM9, LPT0..LPT9` (`:19`, incluye 0 aunque Windows solo use 1-9). Mensaje `Nombre reservado: con` (`:20`, valor **sin** comillas).
- Devuelve `value.lower()` (`:21`).

### Reglas de `portable_path` (`storage.py:24-32`)
- Rechaza si `win.root` o `win.drive` (ruta absoluta, `\foo`, `C:foo`, UNC) (`:27`).
- Rechaza `..` como segmento y cualquier `:` en un segmento (`:27`).
- Por cada segmento distinto de `.` y vacío (`:30`): rechaza caracteres `<>"|?*`, caracteres de control (código < 32), segmentos que terminan en espacio o punto, y nombres reservados (misma tabla que `identifier`, comparando el primer segmento antes del `.`). Mensaje `Ruta reservada: {p1}` (`:31`).
- Errores: `Ruta no portable: {p1}` (`:28`) con el valor **sin** comillas.
- Acepta `\` y `/` como separador (lo hace `PureWindowsPath`).

### Reglas de `command_name` (`storage.py:43-52`)
- Rechaza no-cadena, cadena vacía tras `strip()`, longitud > 240 (`:45`), o cualquier `/` o `\` (`:45`). Mensaje `Nombre de comando no válido: 'valor'` (`:46`, con comillas `repr`).
- Llama a `portable_path(value + '.cmd')` y a `portable_path(value)` (`:47`, `:49`).
- Rechaza `.` (`:50`). Devuelve `value.lower()` (`:52`).

## Formatos de disco
- **Lock** `<root>/core/state/<name>` (`:79-81`), por defecto `core/state/mutation.lock`. Se crea si falta (`a+b`) y **nunca se borra**. El nombre pasa por `identifier` (`:81`).
- **JSON** escrito por `write_json` (`:59-70`): UTF-8, sin BOM, fin de línea `\n`, `indent=2`, `ensure_ascii=False`, una nueva línea final (`:65`). Orden de claves: el de inserción (no hay `sort_keys`). Ejemplo para `{"a": "ñ"}`:
  ```
  {
    "a": "ñ"
  }
  ```
  (seguido de `\n`).
- **Lectura** `read_json` (`:56`): UTF-8 con BOM tolerado (`utf-8-sig`).
- **Temporal**: `tempfile.mkstemp(dir=path.parent, suffix='.tmp')` (`:61`), mismo directorio que el destino, nombre aleatorio `*.tmp`. Se elimina en `finally` si quedó (`:70`).

## Invariantes
1. `identifier` devuelve siempre minúsculas y nunca una cadena que empiece por carácter no alfanumérico, ni de más de 80 caracteres.
2. Una ruta aceptada por `portable_path` no tiene raíz, unidad, `..`, `:` ni segmentos con caracteres prohibidos de Windows, ni termina en espacio o punto en ningún segmento, ni usa nombres reservados.
3. `inside` nunca devuelve una ruta cuyo `resolve()` esté fuera de la raíz (incluye enlaces simbólicos que apunten fuera) (`:38`).
4. `write_json` es atómico: escribe en un temporal del mismo directorio, hace `fsync` (`:67`) y luego `os.replace` (`:68`). El destino contiene la versión anterior o la nueva, nunca una mezcla.
5. El lock de `mutation` lo libera el sistema operativo si el proceso muere (`:75`).
6. Sin `wait`, `mutation` falla en el acto cuando el lock está ocupado (Windows, `:98-101`).

## Casos límite y errores
- Longitud máxima de identificador: 80 caracteres (1 inicial + 79 `{0,79}`) (`:17`).
- Longitud máxima de nombre de comando: 240 caracteres, sin contar `.cmd` (`:45`).
- `portable_path('')` y `portable_path('.')` no lanzan: `PureWindowsPath` devuelve partes vacías. `inside(root, '')` devuelve la raíz. Duda: ¿deben rechazarse?
- `command_name('..')` lanza `Ruta no portable: ..` (de `portable_path(value)`, `:49`), no `Nombre de comando no válido`, porque `'..' + '.cmd'` no contiene `..` como segmento. Duda: confirmar que el mensaje es aceptable.
- Ejemplos de mensajes exactos:
  - `identifier('bad name')` → `Nombre no válido: 'bad name'`
  - `identifier('con')` → `Nombre reservado: con`
  - `portable_path('../x')` → `Ruta no portable: ../x`
  - `portable_path('a<b')` → `Ruta reservada: a<b`
  - `inside(root, 'C:x')` → `Ruta no portable: C:x`
  - `command_name('a/b')` → `Nombre de comando no válido: 'a/b'`
- `mutation` en Windows: si el fichero está vacío escribe un byte `b'0'` (`:92-93`) y bloquea el byte 0 con `LK_NBLCK` (`:96`). Reintenta cada 0,1 s (`:101`) hasta `wait` segundos. Al agotarse lanza `YitoError('Otra instancia está modificando MITO. Espera a que termine.')` (`:100`) con la causa original.
- `mutation` en Unix: `fcntl.flock(LOCK_EX | LOCK_NB)` (`:104`). Ignora `wait`. Lanza `OSError` (`BlockingIOError`), no `YitoError`. Duda: ¿debe traducirse y respetar `wait` también en Unix?
- En Windows, el error de lock se detecta con `OSError` (`:98`), y el comentario (`:89-90`) indica que `PermissionError` significa «ocupado».
- `read_json` no distingue JSON inválido de otros errores.

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_manifest_change_replaces_same_version_and_unsigned_update_requires_confirmation` | Depende de `Engine`/`catalog`. Un manifiesto cambiado sin `hash` exige confirmación (`'sin hash'`), y si se confirma conserva la ruta de instalación y reemplaza el contenido. No prueba `storage.py` directamente. |
| `test_dependency_replacement_does_not_activate_requesting_app_in_other_profiles` | Depende de `Engine`/`catalog`. Reemplazar una dependencia no activa la aplicación en otros perfiles. |
| `test_entire_plan_is_prepared_before_any_replacement` | Depende de `Engine`. Si un elemento del plan falla (`'Comando ausente'`), ninguna instalación se reemplaza. |
| `test_migration_keeps_profile_copies_data_activation_and_is_idempotent` | Depende de `storage_migration`/`isolation`. Migración legada: copias por perfil, datos de usuario, activación, y segunda ejecución sin efecto. |
| `test_migration_refuses_ambiguous_versions_without_changes` | Depende de `storage_migration`. Dos versiones legadas ambiguas (`'ambigua'`) no cambian nada. |
| `test_failed_copy_preparation_restores_all_old_binaries_and_user_data` | Depende de `isolation`/`Engine`. Si falla la preparación de una copia, se restauran binarios y datos de todos los perfiles. |
| `test_migration_rolls_back_on_profile_write_failure` | Depende de `storage_migration`. Si falla `save_profile`, el `.mito.json` legado y el perfil quedan intactos. |

Duda: ninguno de los 7 tests ejercita directamente `identifier`, `portable_path`, `inside`, `command_name`, `read_json`, `write_json` o `mutation`. Son tests de integración de `engine`, `catalog`, `storage_migration` e `isolation`. ¿Dónde deben vivir en Rust (`mito-core::storage` o en los módulos de dominio)? Hay que escribir tests unitarios propios para las reglas de este contrato.

## Dependencias
- `i18n.tr(message: &str, values...) -> String` (`mito-i18n`). Interpola `{pN}` con los valores (`i18n.py:97-109`). Solo se usa para mensajes de error.
- Biblioteca estándar de Python (`json`, `os`, `tempfile`, `re`, `pathlib`, `contextlib`, `msvcrt`/`fcntl`).
- Dependientes en el original: `configuration.py`, `persistence.py`, y prácticamente todos los módulos que escriben disco.

## Notas para Rust
- **Atomicidad**: `tempfile::NamedTempFile::new_in(parent)`, escribir, `as_file().sync_all()` (equivale a `fsync`), y `persist(path)` (rename que reemplaza). `persist` no hace `fsync` por sí solo.
- **JSON**: `serde_json` con `PrettyFormatter` de 2 espacios. Para conservar el orden de claves, activar `preserve_order`. Duda: `ensure_ascii=False` escribe UTF-8 directo, así que serde_json debe coincidir. Duda: los floats se formatean distinto (repr de Python frente a ryu); los manifiestos ¿contienen floats?
- **Lock**: en Windows, `LockFileEx` sobre el byte 0 (con `windows-sys`) para ser compatible con `FileStream.Lock` de .NET usado en el bootstrap PowerShell. `fd-lock` o `fs4` bloquean todo el fichero y no son exactamente equivalentes; confirmar. En Unix, `flock` con `LOCK_EX | LOCK_NB` (`fs4`, `fd-lock` o `rustix`). Duda: ¿se aplica `wait` también en Unix?
- **Rutas portables**: no usar `std::path::Path` para validar (depende del SO). Parsear a mano separando por `\` y `/`, o usar `typed-path`. Mantener las mismas reglas.
- **`inside`**: `std::fs::canonicalize` exige que la ruta exista y en Windows devuelve el prefijo `\\?\`; usar `dunce::canonicalize` y resolver a mano los componentes para rutas que aún no existen. Duda: el `resolve()` de Python no exige existencia.
- **Mensajes**: `tr` de Python formatea el valor con `repr` en algunos casos y sin comillas en otros (ver arriba). Replicar la misma convención en Rust (Duda: `{:?}` no es idéntico a `repr`).
- **`root_path`**: devuelve tres niveles por encima de `core/app/mito/storage.py`. Duda: en Rust no hay fichero fuente, ¿se usará el directorio del ejecutable o una variable de entorno?
- **Errores**: `thiserror` con una variante `YitoError(String)` o similar. Duda: ¿mantener `YitoError` como tipo único del dominio o uno por módulo?
