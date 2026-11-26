# archives.py → mito_transfer::archives

Origen: `core/app/mito/archives.py` (104 líneas). Sin archivo de test propio: se cubre desde `core/tests/test_activation.py`, `test_msi.py` y `test_engine.py` (ver tabla de casos más abajo). Otros tests lo parchean con `unittest.mock` y no lo ejercitan (ver final).

Nota de nombre: el crate es `mito-transfer`; el módulo Rust es `mito_transfer::archives`.

## Responsabilidad

Preparar una descarga para su uso: extraer contenedores (ZIP, TAR, 7z, RAR, MSI, Inno Setup) o copiar el archivo suelto. No ejecuta nunca el archivo descargado (docstring de la línea 1). El extractor no ejecuta `.exe` sueltos ni instaladores.

## API pública del original

| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `extract(archive, destination, root, filename, *, log=lambda _: None, innosetup=False, inno_root='{app}')` (12) | `archive: Path` (descarga), `destination: Path`, `root: Path` (raíz de la instalación), `filename` (nombre local, de `download_filename`) | `True` si se extrajo un contenedor; `False` si se copió un archivo suelto | `YitoError` por formato no soportado, límites, rutas, enlaces; errores de los módulos delegados (`inno`, `msi`, 7-Zip) |
| `extract_7zip(archive, destination, root, *, max_expanded=MAX_EXPANDED, max_entries=150000)` (70) | archivo 7z o RAR, destino, raíz | `None` | `YitoError` si falta `core/tools/7zip/7z.exe`, listado no reconocido, entrada con enlace, duplicada o cifrada, o tamaño/entradas excedidos; `subprocess.CalledProcessError` si 7-Zip falla (no se convierte; ver Duda 4) |

Constantes importadas de `transfer.py`: `MAX_EXPANDED` (12 GiB, transfer.py:20) y `extract_zip` (transfer.py:380, ver `transfer.md`).

## Formatos de disco

- **Entrada**: la descarga ya guardada en `archive` (bytes crudos).
- **Salida, contenedor**: árbol de archivos bajo `destination` con las rutas internas del archivo.
- **Salida, suelto**: `destination/<filename>` con el contenido idéntico (`shutil.copy2`, línea 23, 65). `destination` se crea si no existe.
- **Listado 7-Zip**: comando `7z l -slt -ba -sccUTF-8 <archive>` (línea 75). Salida de texto con registros separados por líneas vacías, con formato `clave = valor`. Ejemplo literal del test (test_activation.py:156-160):

```
Path = settings
Size = 1
Symbolic Link = outside
```

  El registro `Path = ../escape` debe rechazarse (test línea 156, `YitoError`).
- **Extracción 7-Zip**: `7z x <archive> -o<destination> -y -bd` (línea 100). Sin `-p` (no se piden contraseñas).

## Orden de decisión de `extract` (Invariantes de formato)

Los formatos se evalúan en este orden. El primero que aplica gana:

1. `innosetup=True` y nombre con sufijo `.exe` (case-insensitive) → `inno.extract_inno(archive, destination, root, inno_root)`; devuelve `True`. Delegado a `innounp` privado (no 7-Zip).
2. Sufijo `.jar` o `.ico` → copia íntegra a `destination/<filename>`; devuelve `False`. Aunque el archivo sea un ZIP. Un alias `#/archivo.zip` cambia el sufijo y sí extrae (ver test línea 105-112).
3. Sufijo `.msi` → `msi.extract_msi(archive, destination, root, log=log)`; devuelve `True`. Las tablas MSI se leen con APIs de Windows Installer en modo lectura. Los CAB internos se descomprimen con 7-Zip (ARCHITECTURE.md, sección "Alcance de portabilidad").
4. Contenido ZIP (`zipfile.is_zipfile`, línea 29) → `extract_zip`. Si lanza `NotImplementedError` (método no soportado, p. ej. Deflate64), se reintenta con `extract_7zip` (líneas 30-35). Devuelve `True`.
5. Contenido TAR (`tarfile.is_tarfile`, línea 37; detecta también gzip/xz/bz2 por compresión) → extracción nativa con validación previa (líneas 38-56). Devuelve `True`.
6. Sufijo `.7z` o `.rar` (case-insensitive), o sin sufijo y firma `7z\xBC\xAF\x27\x1C` o `Rar!\x1A\x07` (líneas 57-62) → `extract_7zip`. Devuelve `True`.
7. Sufijo vacío, `.exe`, `.dll`, `.jar`, `.ps1`, `.cmd`, `.bat` o `.com` (línea 63) → copia suelta. Devuelve `False`.
8. Cualquier otro → `YitoError('Formato de archivo todavía no soportado: {p1}')` (línea 67).

Nota: en el paso 6 la firma solo se mira si el sufijo está vacío. Un `.exe` con firma 7z se copia como suelto (paso 7).

## Formatos soportados: qué hace Python y qué delega

| Formato | Detección | Quién extrae | Notas |
|---|---|---|---|
| ZIP y `.nupkg` | contenido (`is_zipfile`) | Python (`transfer.extract_zip`) | Deflate64 y otros métodos no soportados por `zipfile` → 7-Zip |
| TAR, `.tar.gz`, `.tgz`, `.tar.xz`, `.tar.bz2` | contenido (`tarfile.is_tarfile`) | Python (`tarfile`) | Solo `isfile` y `isdir` (línea 45) |
| gzip, xz, bzip2 sueltos (sin TAR) | — | **no soportado** | Ver Duda 1 |
| 7z | sufijo `.7z` o firma sin sufijo | 7-Zip privado (`core/tools/7zip/7z.exe`) | Listado previo obligatorio |
| RAR | sufijo `.rar` o firma `Rar!` sin sufijo | 7-Zip privado | Igual que 7z |
| MSI | sufijo `.msi` | Python (`msi.py`, APIs de Windows Installer) + 7-Zip para CAB internos | Si no es MSI válido: error sin publicar |
| Inno Setup `.exe` (con `innosetup: true`) | sufijo `.exe` y opción del manifiesto | innounp 2.71.1 privado (no 7-Zip) | Nunca ejecuta el instalador |
| CAB suelto | — | **no soportado** en `extract` | Ver Duda 6 |
| `.jar`, `.ico`, `.exe`, `.dll`, `.ps1`, `.cmd`, `.bat`, `.com`, sin sufijo | sufijo | copia suelta | Sin extracción |

## Validación de ZIP (en `transfer.extract_zip`, ver `transfer.md`)

- Límite de entradas: más de 150 000 → `YitoError('Archivo demasiado grande para extraer')`.
- Suma de `file_size` declarados > `MAX_EXPANDED` → mismo error.
- Por entrada, en el primer bucle: ruta validada con `inside` (traversal, rutas reservadas, ADS con `:`); duplicados sin distinguir mayúsculas → `YitoError('Entrada duplicada: ...')`; enlace (`S_ISLNK` en `external_attr >> 16`) → `YitoError('Los enlaces del archivo no están permitidos')`.
- Ninguna entrada se escribe antes de pasar la validación completa.

## Validación de TAR (archives.py:38-56)

- Más de 150 000 miembros o suma de `size` > `MAX_EXPANDED` → `YitoError('TAR demasiado grande')`.
- Cada miembro debe ser `isfile()` o `isdir()` y su ruta debe pasar `inside`; si no, `YitoError('TAR con enlaces, dispositivos o entradas duplicadas')`. Esto incluye enlaces simbólicos, enlaces duros, dispositivos y duplicados sin distinguir mayúsculas.
- Luego se extrae (sin límite de bytes reales; ver Duda 3).

## Validación de 7z y RAR (archives.py:70-104)

1. Ejecutable `root/core/tools/7zip/7z.exe`; si no existe: `YitoError('Falta el extractor privado 7-Zip; ejecuta bootstrap')` (línea 71-73).
2. Listado `-slt`. Cada registro sin `Path` → `YitoError('Listado 7z no reconocido')`.
3. Por registro: `inside(destination, Path)`; duplicado sin mayúsculas, o `Symbolic Link`/`Hard Link` distinto de vacío o `-`, o `Attributes` con la letra `l` → `YitoError('7z con enlaces o entradas duplicadas')`; `Encrypted = +` → `YitoError('No se admiten archivos cifrados')`.
4. Suma de `Size` y número de registros: más de `max_entries` (150 000) o total > `max_expanded` → `YitoError('7z demasiado grande')`.
5. Extracción `7z x ... -y -bd`.
6. Post-comprobación: cualquier ruta extraída que sea enlace, junction o que resuelva fuera de `destination` → `YitoError('Enlace inesperado tras extraer 7z')` (líneas 101-103). Ver Duda 2.

## Invariantes

1. `extract` nunca ejecuta el archivo descargado. Los `.exe` sueltos se copian, no se lanzan (archives.py:1, 63-66).
2. ZIP y TAR se validan completos antes de escribir cualquier entrada.
3. 7z y RAR se validan en el listado antes de ejecutar la extracción (`7z x`).
4. El valor de retorno `True` significa contenedor extraído y `False` significa archivo suelto copiado. El llamador usa esa distinción para `extract_dir` y `extract_to` (ARCHITECTURE.md).
5. Los formatos no listados producen error sin escribir en `destination` (salvo los copiados).
6. La copia de `.jar`/`.ico` ocurre antes de comprobar contenedores: un JAR nunca se descomprime por tener estructura ZIP.
7. Las rutas internas de todos los formatos pasan por `storage.inside` / `portable_path`.

## Casos límite y errores

- **ZIP no soportado por `zipfile` (Deflate64)**: se reintenta con 7-Zip, pero solo después de validar. Si la validación falla, no se llama a 7-Zip (test `test_zip_fallback_never_bypasses_path_validation`).
- **TAR corrupto o no reconocido**: se considera no-TAR; cae a los pasos siguientes (7z por firma o copia suelta). Un TAR con enlace simbólico se rechaza antes de escribir (test `test_tar_rejects_symlinks`).
- **Extensión engañosa**: el orden prioriza contenido ZIP/TAR sobre sufijo `.7z`/`.rar`. Un `.7z` que contiene ZIP se extrae como ZIP.
- **MSI corrupto**: `YitoError`; nada se publica en `destination` (`test_corrupt_msi_is_not_copied_as_payload`).
- **7-Zip ausente**: `YitoError('Falta el extractor privado 7-Zip; ejecuta bootstrap')` antes de tocar la descarga.

## Casos de test del original

Tests que ejercitan `archives.py` (de forma directa o vía `extract`/`extract_7zip`):

| Test | Archivo | Qué comprueba |
|---|---|---|
| `test_tar_rejects_symlinks` (activation:81-90) | `test_activation.py` | TAR con entrada `SYMTYPE` hacia `../outside` → `YitoError`; `out/` no existe |
| `test_tar_and_standalone_file` (activation:92-103) | `test_activation.py` | `good.tar.gz` con `folder/file.txt` → extrae `out/folder/file.txt` con bytes `test`; `tool.exe` con contenido `MZ fixture` se copia a `exe/tool.exe` idéntico |
| `test_jar_is_preserved_unless_url_alias_requests_zip_extraction` (activation:105-112) | `test_activation.py` | `Tool.JAR` con contenido ZIP se copia íntegro; `tool.zip` con el mismo contenido sí extrae `Main.class` |
| `test_deflate64_zip_uses_private_extractor` (activation:114-126) | `test_activation.py` | ZIP con Deflate64 generado con 7z (`compress_type == 9`) se extrae con el 7-Zip privado; `out/fixture.txt` igual al original |
| `test_extensionless_7zip_detected_by_signature` (activation:128-137) | `test_activation.py` | Archivo 7z sin extensión (`download`) se detecta por firma y extrae `out/fixture.txt` |
| `test_zip_fallback_never_bypasses_path_validation` (activation:139-146) | `test_activation.py` | ZIP con `../escape.txt` → `YitoError`; `extract_7zip` (parcheado) no se llama; `out/` no existe |
| `test_rar_rejects_unsafe_entries_before_extraction` (activation:148-162) | `test_activation.py` | `subprocess.run` parcheado devuelve listados maliciosos (`../escape`, `Symbolic Link = outside`, `Encrypted = +`); cada uno → `YitoError`; se llama una sola vez (listado, `l`), nunca a extraer; `out/` no existe |
| `test_corrupt_msi_is_not_copied_as_payload` (msi:165-172) | `test_msi.py` | MSI inválido (`b'invalid MSI'`) → `YitoError`; `out/` no existe |
| `test_cabinet_corruption_and_external_media_fail_before_publication` (msi:174-205) | `test_msi.py` | Variantes con `Media` externa, archivo ausente, tamaño incorrecto y CAB con spanning → `YitoError`; destino no creado. Usa `archives.extract` → `msi.extract_msi` → 7-Zip privado (copiado de `tests/tools/7zip`) |
| `test_zip_traversal_rejected_before_any_extraction` (engine:93-101) | `test_engine.py` | Llama directamente a `transfer.extract_zip` (no a `archives.extract`) con `good.txt` y `../escape`; `YitoError`; `out/good.txt` no existe |

Tests que parchean `archives` (no lo ejercitan de verdad; sirven de contrato de llamada):

- `test_burn.py:121,149,168` parchea `mito.archives.extract_7zip` para simular CAB de Burn.
- `test_inno.py:108` parchea `mito.archives.extract` para simular Inno.

No ejercitan `archives.py`: `test_catalog_performance.test_bad_archives_preserve_previous_snapshot` (usa `catalog.import_bucket` con `zipfile` directo) y `test_profile_transfer.test_import_rejects_unsafe_archives` (usa `profile_transfer.read_export` con `zipfile` directo). Verificar en F-catalog y F-profile.

## Dependencias

- `mito.i18n.tr` (archives.py líneas 14, 41, 46, 67, 73, 89, 92, 94, 98, 103): mensajes traducibles.
- `mito.storage`: `YitoError`, `inside`.
- `mito.transfer`: `MAX_EXPANDED`, `extract_zip`.
- `mito.inno.extract_inno` (línea 17), `mito.msi.extract_msi` (línea 27): módulos delegados; ver sus contratos.
- `subprocess`, `shutil`, `tarfile`, `zipfile`.
- Llamadores (fuera de alcance): `engine.py:336` (`extract`), `burn.py:138`, `hooks.py:205`, `msi.py:199` (`extract_7zip`), `pocketools.py:12`, `self_update.py`.

## Notas para Rust

### Crates sugeridas

- ZIP: `zip`. Validar nombres con la lógica de `inside` antes de extraer; comprobar enlace por `unix_mode() & 0o170000 == 0o120000`. Método Deflate64: verificar soporte en la versión elegida; si no, usar el 7-Zip privado como en el original (archives.py:30-35).
- TAR: `tar`, con `flate2` (gzip), `xz2` (xz), `bzip2` (bz2). Detectar compresión por firma de bytes, no por extensión.
- 7z y RAR: sin crate; invocar el 7-Zip privado con `std::process::Command` y `creation_flags(CREATE_NO_WINDOW)` (`0x08000000`) como en la línea 74. Parsear `7z l -slt -ba -sccUTF-8` igual que el original.
- Hashes e I/O: ver `transfer.md`.

### Mapa de decisiones

- Mantener el orden de decisión de `extract` (innosetup .exe → jar/ico → msi → zip → tar → 7z/rar → sueltos → error). Cambiar el orden cambia el resultado con extensiones engañosas.
- Devolver un enum en Rust (`Extracted` / `LooseFile`) en lugar de `bool`.
- Inno Setup y MSI se delegan a sus módulos (contratos `inno.md` y `msi.md`, pendientes); aquí solo se llama.

### Diferencias inevitables

- En Windows, la detección de junction se hace con `symlink_metadata` y el atributo de punto de reanclaje (`FILE_ATTRIBUTE_REPARSE_POINT`). `Path.is_junction()` de Python exige 3.12; el original no define una regla propia para junctions más allá de esa llamada.
- `zipfile.is_zipfile` busca el registro de fin de directorio en todo el archivo (incluido el caso de datos extra al final). Rust (`zip::ZipArchive::new`) puede aceptar o rechazar archivos de forma distinta; ver `transfer.md`, Duda 5.

### Dudas

- Duda 1: `ARCHITECTURE.md` (sección "Alcance de portabilidad") afirma que el extractor acepta `gzip`, `xz` y `bzip2`. En el código solo se aceptan dentro de TAR (`tarfile`). Un `.gz` suelto cae en `Formato de archivo todavía no soportado`. ¿Debe Rust aceptar gzip, xz y bz2 sueltos (descomprimir a un archivo) o mantener el comportamiento actual?
- Duda 2: en `extract_7zip`, la post-comprobación de enlaces (líneas 101-103) se ejecuta después de extraer. Si falla, los archivos ya están en `destination`. El llamador debe limpiar. ¿Debe Rust validar antes o limpiar tras el fallo?
- Duda 3: el límite `MAX_EXPANDED` de TAR se comprueba con `member.size` declarado (archives.py:40), no con bytes leídos. Mismo problema que en ZIP (transfer.md, Duda 4).
- Duda 4: `subprocess.run(..., check=True)` (líneas 75, 100) lanza `CalledProcessError`, no `YitoError`. ¿Debe Rust mapear el código de salida de 7-Zip a un error propio?
- Duda 5: la comprobación de enlaces 7z usa `'l' in Attributes` (línea 91), una búsqueda de subcadena. Con `-slt` el atributo de enlace en Unix es `l` como primera letra de los permisos. ¿Debe Rust comprobar solo el primer carácter o la letra `l` en cualquier posición?
- Duda 6: CAB suelto no se contempla en `extract`. ¿Es intencional (solo dentro de MSI/Burn)?
- Duda 7: si `extract_zip` falla a mitad (p. ej. `NotImplementedError` en la segunda entrada), las entradas anteriores ya están escritas y el reintento con 7-Zip las sobrescribe (`-y`). ¿Debe Rust limpiar `destination` antes del fallback?
- Duda 8: el orden de detección por contenido (ZIP antes que sufijo `.7z`) hace que un `.exe` con contenido ZIP se extraiga. Ver transfer.md, Duda 5.
