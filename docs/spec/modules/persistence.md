# persistence.py → mito-core::persistence

Origen: `core/app/mito/persistence.py` (101 líneas), `core/tests/test_persistence_locations.py` (2 tests).

## Responsabilidad
Gestiona los directorios y ficheros de datos persistentes de cada aplicación (`persist` de su manifiesto). Los datos viven en el hogar de MITO, fuera del directorio de la aplicación, y se enlazan a la carpeta de la aplicación mediante un *junction* (directorios) o un *hard link* (ficheros). Permite calcular dónde están los datos sin crear nada, y abrir la carpeta en el explorador.

## API pública del original
| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `definitions(manifest) -> Iterator[(origen, destino)]` (`:11-18`) | manifiesto con `persist` | pares | `YitoError` si un elemento de lista no tiene 2 valores. |
| `linked(source: Path, target: Path) -> bool` (`:21-33`) | origen y destino | ¿ya es el enlace correcto? | Nunca lanza (`OSError` se traga). |
| `prepare(root, payload, key, manifest, destination=None)` (`:36-86`) | raíz, carpeta de la aplicación (`payload`), clave, manifiesto, destino opcional | `None` (efectos en disco) | `YitoError` (ver casos) y `OSError` del sistema. |
| `locations(root, lock, profile=None) -> list[(relativo, Path)]` (`:89-94`) | raíz, bloqueo de la aplicación, perfil opcional | lista de rutas de datos | `YitoError` si la sintaxis de un alias no es válida. **No crea nada.** |
| `open_location(path: Path)` (`:97-101`) | ruta | — (abre el explorador) | `YitoError` si no existe. |

Dependencias de `locations`: `catalog.effective(lock['manifest'])` y `isolation.persist_root(root, lock, profile)` (`:91-93`). Ver Dependencias.

## Formatos de disco

### Rutas de datos
- Sin perfil: `<root>/home/persist/<clave>/<alias>` (`:47`, `:17` del test: `root/'home/persist/main/editor/config.ini'`).
- Con perfil: `<root>/home/profiles/<perfil>/persist/<clave>/<alias>` (test línea 14: `root/'home/profiles/one/persist/main/editor/saved'`).
- La clave de aplicación contiene `/` (`main/editor`), así que genera carpetas anidadas.
- El destino se construye con `storage.inside(destino, alias)` (`:47`), por lo que el alias se valida como ruta portable.

### Fichero de registro en la aplicación: `<payload>/.mito-persist.json`
Diccionario `relativo → alias`, escrito con `write_json` (indentado, UTF-8, `:39`, `:79-80`). Ejemplo derivado de `[['data','saved'], 'config.ini']`:
```
{
  "data": "saved",
  "config.ini": "config.ini"
}
```
Este registro es el **diario** del motor: se escribe antes de crear cada enlace (`:78-80`).

### Valores originales: `<payload>/.mito-defaults/<relativo>`
Si la ruta `source` ya existe en la aplicación y no es un enlace del motor, se mueve (`rename`) a `.mito-defaults/<relativo>` (`:64-68`). Luego se copia a `target` si `target` no existe: `copytree` para directorio, `copy2` para fichero (`:69-74`).

### Enlaces
- Directorio: *junction* de Windows creado con `powershell.exe -NoProfile -NonInteractive -Command "New-Item -ItemType Junction -Path '<source>' -Value '<target>' -ErrorAction Stop | Out-Null"` (`:82-84`, con `CREATE_NO_WINDOW`).
- Fichero: *hard link* con `os.link(target, source)` (`:86`).

Comillas simples de la ruta duplicadas (`literal`, `:82-83`).

## Invariantes
1. `source` siempre queda dentro de `payload`: se comprueba `source.parent.resolve()` (`:45`).
2. El diario `.mito-persist.json` se escribe antes de crear cualquier enlace (`:78-80`). Una creación interrumpida es reintentable.
3. Solo se eliminan enlaces que el propio motor registró en `.mito-persist.json`: `rmdir` del junction (`:58`, elimina el punto de reparse, no el contenido) o `unlink` de un fichero enlazado (`:59`). Nunca se borra un directorio real (`:60-61`).
4. Si `relative` ya está registrado, el alias debe coincidir; si no, error (`:52-54`).
5. Un enlace ajeno (no registrado) no se reemplaza (`:62-63`).
6. `prepare` no crea nada si el manifiesto no tiene `persist` (`:37-38`).
7. `locations` no crea carpetas ni ficheros (test: `assertFalse((root/'home').exists())`).
8. Un enlace ya correcto (`linked`) se omite sin ejecutar PowerShell (`:49-51`).

## Casos límite y errores
| Caso | Comportamiento | Mensaje (`tr`) |
|---|---|---|
| Elemento de `persist` que es lista de longitud ≠ 2 (`:14`) | `YitoError` | `persist debe ser una ruta o [origen, destino]` |
| Sintaxis inválida (`inside(C:/__mito_validation__, relative)`, `:43`) | `YitoError` de `storage.portable_path`/`inside` | (ver `storage.md`) |
| `source` dentro de un enlace (padre resuelto fuera de `payload`, `:45-46`) | `YitoError` | `persist anidado dentro de otro enlace` |
| `relative` ya registrado con otro alias (`:52-54`) | `YitoError` | `Definición persist modificada inesperadamente` |
| `relative` registrado pero `source` es un directorio real (`:60-61`) | `YitoError` | `Se esperaba un enlace persistente` |
| `source` es un enlace ajeno al motor (junction o symlink) (`:62-63`) | `YitoError` | `No se reemplaza un enlace ajeno al motor` |
| `defaults` o `source` fuera de la raíz MITO (`:66-67`) | `YitoError` | `Ruta persist fuera de MITO` |
| `open_location` con ruta inexistente (`:98-99`) | `YitoError` | `Esta persistencia todavía no se ha creado` |

Casos adicionales derivados del código (duda):
- **Fichero persistente sin origen previo**: si `source` no existe y `defaults` no existe, `target` se crea con `target.mkdir(parents=True)` (`:75-76`). Después `target.is_dir()` es verdadero y se crea un *junction*, no un hard link. Es decir, un `persist: ["config.ini"]` sin fichero previo produce un **directorio** llamado `config.ini`. Duda: ¿es el comportamiento deseado? No hay test que lo cubra.
- `prepare` no deshace cambios parciales si falla a mitad (no hay rollback local). La recuperación depende del diario `.mito-persist.json` y de que la operación sea reintentable. El rollback del test `test_failed_copy_preparation...` está en `storage`/`isolation`.
- Enlaces de fichero (hard link) exigen que `source` y `target` estén en el mismo volumen. Duda: ¿qué ocurre si no lo están? El original no lo controla (`os.link` lanza `OSError`).
- `definitions` acepta una cadena sola (`item, item`, `:18`). Un elemento `None` o número se trata como cadena en `inside`. Duda: confirmar comportamiento de `catalog.array`.
- `open_location`: si `path` es un fichero, abre su carpeta padre (`:100`). Si es un directorio, lo abre directamente.
- En Windows, un `rename` de `source` a `defaults` no funciona si `source` está abierto por otro proceso (no controlado en el original).

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_paths_follow_manifest_scope_and_aliases` | Con `persist: [['data','saved'], 'config.ini']`, `locations(root, lock, 'one')[0][1]` es `root/home/profiles/one/persist/main/editor/saved`; las rutas de perfiles `one` y `two` son distintas; sin perfil, la segunda ruta es `root/home/persist/main/editor/config.ini` (el alias `config.ini` se conserva); y `locations` no crea `home/`. |
| `test_files_open_parent_folder_and_missing_paths_are_rejected` | `open_location(root/'config.ini')` (fichero existente) llama a `subprocess.Popen(['explorer.exe', str(root)])` con la carpeta padre; `open_location(root/'missing')` lanza `YitoError`. |

Nota: el test de `locations` depende de `catalog.effective` e `isolation.persist_root` (ver Dependencias), que no se han leído. El test de `open_location` depende de `explorer.exe` (solo Windows).

## Dependencias
- `i18n.tr` (`mito-i18n`): mensajes listados en la tabla de casos.
- `catalog.array(value) -> list` (`:12`, no leído). Normaliza `persist` a lista (ausente → vacía). Duda: ¿acepta `None`, cadena o lista?
- `catalog.effective(manifest) -> manifest` (`:91`, no leído). Devuelve el manifiesto efectivo que usa `locations`. Duda: ¿se aplica a `persist` algún cambio?
- `isolation.persist_root(root, lock, profile=None) -> Path` (`:93`, no leído). Devuelve la carpeta base (`<root>/home/persist/<clave>` o `<root>/home/profiles/<perfil>/persist/<clave>`). Se infiere de los tests.
- `storage`: `YitoError`, `inside(root, relative) -> Path`, `read_json(path)`, `write_json(path, value)` (`storage.py:35`, `:55`, `:59`).
- Biblioteca estándar: `os`, `pathlib`, `shutil` (`copytree`, `copy2`), `subprocess` (PowerShell y `explorer.exe`).

## Notas para Rust
- **Junction**: no hay API en la biblioteca estándar. Usar el crate `junction` (crea, lee y elimina) o `windows-sys` con `DeviceIoControl` y `FSCTL_SET_REPARSE_POINT`. Duda: ¿se mantiene la llamada a PowerShell? Crear un junction directamente evita un proceso por cada entrada.
- **`is_junction()`** (Python 3.12) equivale a `junction::exists`/lectura de reparse point. Para el destino del enlace, `junction::get_target`. Quitar el prefijo `\\?\` con `dunce`.
- **Hard link**: `std::fs::hard_link(target, source)`.
- **Eliminar enlace sin tocar contenido**: `junction::delete` (no `remove_dir_all`, que seguiría el enlace). `std::fs::remove_file` para hard links (elimina solo el nombre).
- **Copia recursiva** (`copytree`): `walkdir` + `std::fs::copy`, o `fs_extra::dir::copy`. Duda: ¿se preservan atributos y enlaces como en `copytree` por defecto (`symlinks=False`)?
- **`rename`**: `std::fs::rename` (reemplaza en Windows). Mismo volumen requerido.
- **Diario**: `serde_json` con mapa ordenado (`preserve_order`) para que el orden de `.mito-persist.json` sea estable.
- **Ruta de `target` que no existe**: el comportamiento de crear un directorio para un fichero sin origen (ver casos límite) debe decidirse explícitamente. Duda abierta.
- **`open_location`**: `std::process::Command::new("explorer.exe").arg(folder)`. `Popen` no espera, así que `spawn()` sin `wait()`.
- **Errores**: `YitoError` con mensajes traducidos vía `mito-i18n`.
- **Duda abierta**: los tests de `isolation` (`prepare_payload`, `persist_root`) están en otro contrato. Confirmar quién llama a `prepare` en el original (no visto en esta tarea).
