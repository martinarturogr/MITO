# msi.py → mito_win::msi

Origen: `core/app/mito/msi.py` (234 líneas), `core/tests/test_msi.py` (201 líneas, 5 tests; uno con dos subcasos y varios fallos inyectados).

Nota de crate: PARITY.md asigna `msi.py` a `mito-win`, el único crate con `unsafe` (ADR 0002). Todo acceso a `msi.dll` va allí.

## Responsabilidad

Leer una base MSI en modo solo lectura y extraer sus archivos como datos. Los CAB incrustados (`_Streams`) se descomprimen con el 7-Zip privado. No se ejecutan acciones de instalación: ni `CustomAction`, ni `InstallExecuteSequence`, ni `MsiOpenPackage`, `MsiDoAction` o `MsiInstallProduct` (comentario de `msi.py:36`). El módulo se llama desde `archives.extract` (`.msi`, `archives.py:42`) y desde `hooks.expand_installer('msi')`.

## API pública del original

| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `Database(path)` (clase, 15) | ruta del `.msi` | objeto con `api` (`msi.dll`), `handle` | `YitoError('MSI: Windows database error {status}')` si `MsiOpenDatabaseW` falla |
| `Database.check(status)` (static, 40) | código `UINT` de Windows Installer | `None` | `YitoError('MSI: Windows database error {status}')` si `status != 0` |
| `Database.view(sql, parameter=None)` (ctx. manager, 51) | SQL, parámetro de cadena opcional | handle de vista | `YitoError` por fallos de API, `'MSI: cannot allocate record'` |
| `Database.fetch(view)` (68) | handle de vista | registro `c_uint` o `None` (fin) | `check` si error distinto de fin |
| `Database.rows(table, columns)` (76) | nombre de tabla, lista de columnas | lista de filas (lista de cadenas) | `'MSI: too many table entries'` si más de 150 000 filas |
| `Database.stream(name, target)` (100) | nombre de `_Streams`, `Path` destino | escribe el fichero | `'MSI: missing embedded cabinet {name}'`, `'MSI: cabinet too large'` |
| `leaf(value, directory=False)` (122) | cadena de nombre | nombre local validado o `''` | `'MSI: invalid name {value}'`; también errores de `portable_path` |
| `plan(database, destination)` (135) | base, destino (no se escribe) | `(cabinets, files)` | ver «Invariantes» |
| `extract_msi(archive, destination, root, *, log=…)` (198) | `.msi`, destino, raíz de MITO | `None` | errores de `plan`, `Database`, `extract_7zip`, y de la verificación de cada CAB |
| `MAX_ENTRIES = 150000` (12) | constante | — | — |

## Consultas SQL y tablas leídas

Todas las consultas se hacen con `rows` (sin `WHERE`, sin `ORDER BY`; el orden lo decide el motor de MSI) salvo la de `_Streams`. Los identificadores van entre acentos graves.

| Tabla | Columnas leídas (en este orden) | Uso |
|---|---|---|
| `Directory` | `Directory`, `Directory_Parent`, `DefaultDir` | Árbol de carpetas (`plan`, 136-137) |
| `Component` | `Component`, `Directory_` | Componente → directorio (158) |
| `Media` | `DiskId`, `LastSequence`, `Cabinet` | Rangos de secuencia por CAB (159-160) |
| `File` | `File`, `Component_`, `FileName`, `FileSize`, `Attributes`, `Sequence` | Archivos (168-169) |
| `_Streams` | `Data` con `WHERE Name = ?` (parámetro `name`) | Bytes del CAB (101) |

SQL exacto generado por `rows`:

```
SELECT `Directory`, `Directory_Parent`, `DefaultDir` FROM `Directory`
SELECT `Component`, `Directory_` FROM `Component`
SELECT `DiskId`, `LastSequence`, `Cabinet` FROM `Media`
SELECT `File`, `Component_`, `FileName`, `FileSize`, `Attributes`, `Sequence` FROM `File`
SELECT `Data` FROM `_Streams` WHERE `Name` = ?
```

Las consultas de `_Streams` usan un registro con un parámetro (`MsiCreateRecord(1)` + `MsiRecordSetStringW`).

Ruta de la API: `ctypes.WinDLL('msi.dll', winmode=0x800)`. Firmas registradas:

| Función | Argumentos | Notas |
|---|---|---|
| `MsiOpenDatabaseW` | `(wchar_p path, void* persist=None, uint* handle)` | `persist` = `None` → `MSIDBOPEN_READONLY` (0). No se usa `CREATE` ni `TRANSACT`. |
| `MsiDatabaseOpenViewW` | `(uint db, wchar_p sql, uint* view)` | |
| `MsiViewExecute` | `(uint view, uint record)` | `record` = 0 o el registro con el parámetro |
| `MsiViewFetch` | `(uint view, uint* record)` | `259` (`ERROR_NO_MORE_ITEMS`) → fin, `fetch` devuelve `None` |
| `MsiRecordGetStringW` | `(uint rec, uint field, wchar_p buf, uint* cch)` | Campos 1..N. Primer buffer de 1024 caracteres. Si devuelve `234` (`ERROR_MORE_DATA`) y `cch < 65536`, se reintenta con `cch + 1`. Un segundo `234` lanza error. |
| `MsiRecordReadStream` | `(uint rec, uint field, void* buf, uint* size)` | Lectura en bloques de 1 MiB |
| `MsiCreateRecord`, `MsiRecordSetStringW`, `MsiCloseHandle` | — | |

## Resolución de rutas de directorio (`plan`, líneas 135-166)

1. Para cada clave de `Directory` se resuelve la cadena hasta llegar a una raíz ya resuelta. Se guarda `(clave, nombre)` por nivel.
2. Si `Directory_Parent` está vacío o es igual a la clave, la clave es raíz. Solo se acepta `TARGETDIR`; cualquier otra raíz lanza `'MSI: unsupported root directory'`. `TARGETDIR` se resuelve a `Path('.')`. Su `DefaultDir` (`SourceDir`) **no se valida ni se usa**.
3. Un padre inexistente o un ciclo lanza `'MSI: cyclic or missing directory'`.
4. Cada segmento se obtiene con `leaf(name, directory=True)`:
   - Si contiene `:`, se toma la parte **después del primer** `:` (formato `destino:origen`, como `TARGET:SHORT|Package`).
   - Después se toma la parte **después del último** `|` (formato `nombre_corto|nombre_largo`). Ej.: `SHORT|Package` → `Package`.
   - Si el resultado es `.`, el segmento vacío (`''`): la carpeta es la misma que el padre.
   - Se valida con `portable_path`, y no puede ser vacío, `.`, `..`, ni contener `/` o `\`.
5. Tras cada segmento se valida el acumulado con `portable_path(path.as_posix())`.

Nombres de archivo: `leaf(name)` (sin `directory=True`), con las mismas reglas de `|` y validación. `FileName` `TOOL.EXE|tool.exe` → `tool.exe`. La clave `File` se valida con `leaf(key)` pero no se usa como ruta.

Ruta final de un archivo: `resolved[Component.Directory_] / leaf(FileName)`. Se valida con `portable_path`.

## Medios y CAB

- `Media` se ordena por `(DiskId, LastSequence, Cabinet)` con `int()` sobre `DiskId` y `LastSequence`.
- Para cada fila: si `LastSequence < previous`, si `Cabinet` no empieza por `#`, o si `len(Cabinet) < 2`, lanza `'MSI: only embedded cabinets are supported (no external media)'`.
- Se guarda `(previous, last, Cabinet[1:])` y `previous = last`. El CAB cubre las secuencias `(previous, last]` (exclusivo por abajo, inclusivo por arriba).
- Cada archivo busca el primer CAB con `low < Sequence <= high`. Si no hay ninguno: `'MSI: file has no embedded cabinet'`.
- El nombre del CAB en `_Streams` es `Cabinet` sin el `#` inicial.

## Validación de archivos (`plan`, 167-195)

Para cada fila de `File`:

1. `size = int(FileSize)`, `sequence = int(Sequence)`. Si `size < 0`, `sequence < 1`, o `int(Attributes or 0) & 0x2000` (archivo no comprimido): `'MSI: unsupported uncompressed file or invalid size/sequence'`.
2. `leaf(key)`.
3. `Component_` debe estar en `Component` y su directorio debe estar en `resolved`; si no: `'MSI: missing file component/directory'`.
4. Duplicados (`folded` de la ruta relativa, clave `File` en casefold, secuencia): `'MSI: duplicate file, sequence or destination'`.
5. Se acumula `total += size`.
6. Tras el bucle: si no hay archivos o `total > MAX_EXPANDED` (12 GiB): `'MSI: empty or oversized payload'`.
7. Colisión archivo/directorio: si algún padre de una ruta de archivo coincide con otra ruta de archivo (casefold): `'MSI: file/directory collision'`.

Resultado: `cabinets = [(low, high, name), …]`, `files = [(cabinet_index, key, relative_path, size), …]`.

## Extracción de CAB (`extract_msi`, 198-234)

1. `destination.parent.mkdir(parents=True, exist_ok=True)`.
2. Se abre la base (`Database(archive)`) y se crea un `TemporaryDirectory(dir=destination.parent)` (`scratch`).
3. `plan(database, destination)`.
4. Por cada CAB `index` (1 de N):
   a. `log(tr('MSI: CAB {p1}/{p2}', p1=index+1, p2=N))`.
   b. `database.stream(name, scratch/'payload.cab')`.
   c. Cabecera: se leen 36 bytes. Deben empezar por `MSCF` y `u16 little-endian` en offset 30 (flags) `& 3 == 0`. Si no: `'MSI: invalid or spanning cabinet is unsupported'`. Los flags `0x1` (prev) y `0x2` (next) identifican CAB que continúan en otro disco.
   d. `expected = {key.casefold(): (relative, size)}` de los archivos de este CAB.
   e. `extract_7zip(cabinet, scratch/'expanded', root, max_expanded=Σ size, max_entries=len(expected))`.
   f. `actual = {nombre.casefold(): path}` de los ficheros de `expanded` (solo primer nivel). Si `set(actual) != set(expected)` o hay subcarpetas: `'MSI: cabinet contents do not match File table'`.
   g. Para cada archivo: si `stat().st_size != size`: `f'MSI: file size mismatch {relative}'` (sin `tr`). Después `inside(destination, relative)`, `mkdir` del padre y `shutil.copyfile`.
   h. `completed += 1`. Si `completed % 1000 == 0` o es el último: `log(tr('Archivos {p1}: {p2}/{p3}', p1='MSI', p2=completed, p3=len(files)))`.
   i. `shutil.rmtree(expanded)` y `cabinet.unlink()`.

Importante: el nombre del fichero dentro del CAB debe coincidir con la **clave** `File` (no con `FileName`), en casefold. Así lo prueba `payload` en el test.

`extract_7zip` ejecuta el 7-Zip privado sobre el CAB extraído. Sus límites son los de la lista de `expected`, no los globales.

## Invariantes

1. Solo lectura: `MsiOpenDatabaseW` con `persist=None` (`MSIDBOPEN_READONLY`). Ninguna API de instalación. La base de prueba con una acción `MustNeverRun` demuestra que no se ejecuta.
2. Solo CAB incrustados (`#nombre` en `Media`). Medios externos, `LastSequence` decreciente y CAB con `prev`/`next` se rechazan antes de escribir.
3. `plan` valida todo (directorios, componentes, archivos, secuencias, tamaños, colisiones) antes de leer ningún CAB ni escribir en `destination`.
4. Rutas: cada segmento pasa por `leaf`/`portable_path`; cada destino final por `inside(destination, …)`.
5. El tamaño declarado de cada archivo debe coincidir con el extraído, y el conjunto de nombres del CAB debe coincidir exactamente con la tabla `File`.
6. `TARGETDIR` es la única raíz. Los segmentos de `Directory` son relativos.
7. Un archivo con el bit `0x2000` (no comprimido) en `Attributes` se rechaza.
8. Total declarado ≤ `MAX_EXPANDED` (12 GiB). Filas de `Directory`/`Component`/`File`/`Media` ≤ 150 000 (en `rows`).

## Casos límite y errores

- Archivo que no es una base MSI (bytes `b'invalid MSI'`): `MsiOpenDatabaseW` devuelve error → `YitoError('MSI: Windows database error <n>')`. No se publica nada.
- Media con `Cabinet` externo (`external.cab`): `only embedded cabinets…`.
- Media con `Cabinet = ''`: `only embedded cabinets…` (la longitud < 2).
- Archivo sin CAB que lo cubra (secuencia 2 con `LastSequence` 1): `file has no embedded cabinet`.
- CAB con bit de spanning en el offset 30: `invalid or spanning cabinet is unsupported`.
- CAB cuyo contenido no coincide con `File` (archivo de más, de menos, o subcarpeta): `cabinet contents do not match File table`.
- Tamaño distinto del declarado: `file size mismatch <ruta>`.
- Ruta `..\escape.exe`, `aux.exe` (nombre reservado), `C:\escape`: error de `portable_path`/`leaf`. El test acepta `YitoError` o `ValueError`, porque `int()` lanza `ValueError` si `FileSize` no es número.
- `Attributes = 8192` (0x2000): `unsupported uncompressed file…`.
- Dos archivos con mismo destino (`TOOL.EXE` en dos filas) o misma secuencia: `duplicate file, sequence or destination`.
- Límite de tamaño con `MAX_EXPANDED` parchado a 1: `empty or oversized payload`.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_source_names_and_embedded_media_plan` | `test_msi.py:88-91` | `plan` sobre tablas simuladas: `TARGETDIR` → `.`; `TARGET:SHORT|Package` → `Package`; `bin` → `Package/bin`; cabinets `[(0,1,'cab')]`; archivo `Package/bin/tool.exe` con tamaño 7. |
| `test_invalid_paths_references_duplicates_and_media` | `test_msi.py:93-113` | 12 cambios de una celda (rutas `../escape`, `C:\escape`, `BIN` como padre de `APP` (ciclo), `missing`, `..\escape.exe`, `aux.exe`, `-1`, `8192`, secuencia 2, `external.cab`, `''`) fallan. Duplicado de `TOOL.EXE` → `duplicate`. `MAX_EXPANDED=1` → error. |
| `test_real_msi_lifecycle_and_failed_update_preserves_installation` | `test_msi.py:115-163` | Ciclo completo con MSI real generado (`makecab /D CompressionType=MSZIP`, `MsiOpenDatabaseW(…, 3)`): la instalación publica `core/apps/test/tool/app/bin/tool.exe` con bytes `fixture`; el log contiene en orden `Extrayendo tool.MSI`, `MSI: CAB 1/1`, `Archivos MSI: 1/1`; `CustomAction` no se ejecuta; un update roto deja intacta la instalación previa. |
| `test_corrupt_msi_is_not_copied_as_payload` | `test_msi.py:165-172` | `extract(archive, out, root, 'tool.msi')` con `b'invalid MSI'` → `YitoError`; `out/` no existe. |
| `test_cabinet_corruption_and_external_media_fail_before_publication` | `test_msi.py:174-201` | Cuatro fallos inyectados por parches a `Database.rows`/`Database.stream`: `external` (Media externa), `missing` (archivo ausente de `File`), `size` (tamaño declarado distinto), `spanning` (bit 0 de flags). Todos → `YitoError`, y el destino no existe. |

Notas sobre los tests:

- `Tables` (`test_msi.py:18-29`) simula `Database.rows`; `plan` solo llama a `rows`.
- `create_fixture` (`test_msi.py:32-84`) genera el MSI con las APIs de escritura de `msi.dll` (`MsiRecordSetStreamW`, `MsiDatabaseCommit`) y tablas `CustomAction` e `InstallExecuteSequence` como trampa.
- Los tests que hacen extracción real copian `tests/tools/7zip` a `core/tools/7zip`.

## Dependencias

- `storage`: `YitoError`, `inside`, `portable_path`.
- `i18n.tr`: `'MSI: CAB {p1}/{p2}'`, `'Archivos {p1}: {p2}/{p3}'` (con `p1='MSI'`).
- `transfer.MAX_EXPANDED`.
- `archives.extract_7zip` (dentro de `extract_msi`, import diferido).
- Llamadores: `archives.extract` (`.msi`), `hooks.expand_installer('msi')`.

## Mensajes

Traducidos con `tr`: `MSI: CAB {p1}/{p2}` y `Archivos {p1}: {p2}/{p3}`.

No traducidos (cadenas literales o f-strings): todos los `YitoError` de `msi.py`, por ejemplo `MSI: Windows database error {status}`, `MSI: cannot allocate record`, `MSI: too many table entries`, `MSI: missing embedded cabinet {name}`, `MSI: cabinet too large`, `MSI: invalid name {value}`, `MSI: cyclic or missing directory`, `MSI: unsupported root directory`, `MSI: only embedded cabinets are supported (no external media)`, `MSI: file has no embedded cabinet`, `MSI: missing file component/directory`, `MSI: duplicate file, sequence or destination`, `MSI: empty or oversized payload`, `MSI: file/directory collision`, `MSI: invalid or spanning cabinet is unsupported`, `MSI: cabinet contents do not match File table`, `MSI: file size mismatch {relative}`.

Para Rust hay que decidir si estos mensajes entran en el catálogo de i18n (ver Dudas).

## Notas para Rust

- Crate `windows` (o `windows-sys`) con `Win32::System::ApplicationInstallation` (`MsiOpenDatabaseW`, `MsiDatabaseOpenViewW`, `MsiViewExecute`, `MsiViewFetch`, `MsiRecordGetStringW`, `MsiRecordReadStream`, `MsiCloseHandle`). Cargar `msi.dll` desde `System32` (equivalente a `winmode=0x800`). Cada bloque `unsafe` lleva `// SAFETY:`.
- Constantes: `ERROR_NO_MORE_ITEMS = 259`, `ERROR_MORE_DATA = 234`, `MSIDBOPEN_READONLY = 0`, `msidbFileAttributesNoncompressed = 0x2000`, `MAX_ENTRIES = 150000`.
- La validación de rutas (`leaf`, `portable_path`, `inside`) se implementa una vez en `mito-storage`. No duplicar.
- Evitar `Database.rows` con `Vec<Vec<String>>` sin límite: el límite de 150 000 se aplica en la lectura.
- `extract_msi` copia cada archivo a `destination` mientras procesa CAB. Si el destino es el staging temporal, un fallo a mitad deja restos, pero `engine` no publica. Ver Dudas.

## Dudas

1. `msi.py:36` comenta «Do not use MsiOpenPackage/DoAction/InstallProduct», y los tests comprueban que `CustomAction` no se ejecuta. ¿Debe Rust además rechazar bases con `CustomAction` o `InstallExecuteSequence` no vacías, o basta con no ejecutarlas?
2. El límite de expansión de cada CAB es `Σ size` de sus archivos y `max_entries = len(expected)`. Un CAB sin archivos (`expected` vacío) da `max_expanded=0`. ¿Es intencional? (Tests no lo cubren.)
3. `extract_msi` publica archivos en `destination` **por CAB**. Si el CAB 2 falla tras copiar archivos del CAB 1, `destination` queda parcial. Los tests cubren un solo CAB. ¿Debe Rust validar todos los CAB antes de copiar algo, como hacen `burn` y `ZIP`?
4. `Attributes` con `int(attributes or 0)` admite `Attributes` vacío. ¿Es correcto que `2 & 0x2000` solo rechace el bit no comprimido, sin validar otros bits?
5. Los mensajes `MSI: …` no se traducen (salvo dos). Decidir si pasan a i18n. El original no lo hace.
6. `MsiRecordGetStringW` con `234` solo reintenta una vez y rechaza valores ≥ 65 536 caracteres. Validar con un valor de nombre largo.
7. `plan` usa `int()` sin capturar `ValueError` (los tests lo aceptan). Rust debe devolver un error de tipo `YitoError` (ver `transfer.md` sobre errores).
8. `TARGETDIR` ignora `DefaultDir`. Si una base usa `DefaultDir` distinto de `SourceDir`, el original lo ignora sin aviso.
9. `shutil.rmtree(expanded)` y `cabinet.unlink()` no se ejecutan si hay excepción; el `TemporaryDirectory` externo sí limpia.
