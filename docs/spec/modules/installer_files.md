# installer_files (tests) → mito_engine::install (+ mito_transfer::archives)

Origen: `core/tests/test_installer_files.py` (135 líneas, 6 tests). No hay módulo `.py` propio: el comportamiento que prueba se reparte entre `engine.py` (`_install_payload`, 318-440), `archives.py` (despacho por tipo, `extract`), `hooks.py` (ejecución), `catalog.py` (`installer_definition`, `compatibility`) y `transfer.py` (descargas, hashes, `download_filename`).

Este contrato es el mapa de qué decide cada parte, para que la implementación Rust tenga un único sitio de referencia.

## Responsabilidad

Probar, con un servidor HTTP local en `127.0.0.1` y sin tocar la red real, que un manifiesto con varias URL, extracción de un ZIP, un `pre_install` y un `installer.file` `.ps1` se instala con los argumentos correctos, con el entorno de `home/` y que todos los fallos previos a la publicación dejan la instalación anterior intacta.

## Entorno de prueba

- Carpeta temporal con prefijo `mito installer spaces ` (con espacios) para probar rutas con espacios en el contexto del hook.
- `Engine(root).initialize()` y perfil `test` (`{'workspace':'test','apps':[],'env':{}}`).
- Payloads servidos por `ThreadingHTTPServer(('127.0.0.1',0))`:
  - `/sdk.zip`: ZIP con `sdk/tool.py` (contenido `# never executed`).
  - `/pyshim.ps1`: el script `pyshim.ps1` de la clase.
- `pyshim.ps1` (texto literal en el test, líneas 27-31):
  - `param($destination)`;
  - comprueba que existe `$destination/prepared` (si no: `throw 'pre_install missing'`);
  - escribe `tool.ps1` con `& python "$PSScriptRoot/tool.py" @args`;
  - escribe `arguments.json` con `destination`, `arguments=@($args)`, `names=@($fname)` y `home=$env:USERPROFILE`.
- `manifest()` (líneas 50-54): `version='1'`; `url` = las dos URL locales; `hash` = sha256 de cada payload; `extract_dir='sdk'`; `bin='tool.ps1'`; `pre_install='Set-Content "$dir/prepared" ready'`; `installer={'file':'pyshim.ps1','args':'$dir'}`.
- `install(manifest)` escribe `core/buckets/test.json` y llama `engine.install('tool', 'test')` con `download` real (`mito.transfer.download`) y `mito.terminal.prepare` parcheado.

## Despacho de extracción por tipo de archivo (`archives.extract`)

La decisión depende del **nombre local** (`download_filename(url)`), no de la URL completa, y de `innosetup` del manifiesto. El orden importa: el primer criterio que aplica gana. Este orden es el de `archives.md` (`archives.py`):

| # | Criterio | Acción | Devuelve | Notas |
|---|---|---|---|---|
| 1 | `innosetup=True` y sufijo `.exe` (sin distinguir mayúsculas) | `inno.extract_inno` (innounp privado) | `True` (contenedor) | El instalador no se ejecuta. |
| 2 | Sufijo `.jar` o `.ico` | copia íntegra a `destination/<nombre>` | `False` (suelto) | Antes de mirar contenido; un JAR con estructura ZIP no se descomprime. |
| 3 | Sufijo `.msi` | `msi.extract_msi` (APIs de lectura de Windows Installer + 7-Zip para CAB) | `True` | Sin MSI válido, error sin publicar. |
| 4 | Contenido ZIP (`zipfile.is_zipfile`) | `transfer.extract_zip`; si `NotImplementedError` (p. ej. Deflate64) → `extract_7zip` | `True` | Validación completa antes de escribir. |
| 5 | Contenido TAR (`tarfile.is_tarfile`, también gzip/xz/bz2) | extracción nativa tras validar miembros | `True` | Solo `isfile`/`isdir`; enlaces rechazados. |
| 6 | Sufijo `.7z` o `.rar`, o sin sufijo y firma `7z\xBC\xAF\x27\x1C` o `Rar!\x1A\x07` | `archives.extract_7zip` (7-Zip privado, listado previo) | `True` | La firma solo se mira con sufijo vacío. |
| 7 | Sufijo vacío, `.exe`, `.dll`, `.jar`, `.ps1`, `.cmd`, `.bat` o `.com` | copia suelta a `destination/<nombre>` | `False` | Nunca se ejecuta. |
| 8 | Cualquier otro sufijo | `YitoError('Formato de archivo todavía no soportado: {p1}')` | — | Sin escribir en `destination`. |

Formatos que **no** tienen rama en `extract`:

- Burn (`setup.exe` WiX 3 v2): no se detecta por firma ni por `.wixburn`. Un `setup.exe` sin `innosetup` cae en el paso 7 y se copia suelto. Solo se procesa con `Expand-DarkArchive` (`burn.extract_burn`).
- CAB suelto: no soportado (paso 8 si tiene sufijo `.cab`, o suelto si no).
- gzip, xz, bzip2 sueltos sin TAR: paso 8 (ver `archives.md`, Duda 1).
- `.nupkg` se trata como ZIP por contenido (paso 4).

Efecto de la decisión en el motor (`engine.py:353-358`): si `extract` devuelve `True`, el `extract_dir` se resuelve dentro del directorio extraído. Si devuelve `False` (archivo suelto), `source = expanded` (raíz), por lo que `extract_dir` no aplica. Por eso `sdk.zip` se extrae desde `sdk/` y `pyshim.ps1` se copia a la raíz del payload aunque el manifiesto declare `extract_dir: 'sdk'`.

## Flujo de instalación de una URL (`engine._install_payload`, 318-440)

Para cada `index` de `urls` (líneas 324-359):

1. `expected` = hash de `download_hashes(urls, m.get('hash'))`. Si es `None`, se loguea `"<key>: descarga sin hash declarado (i/N)"`.
2. `download(url, stage/'download-<i>', expected, root=root)`.
3. `filename = download_filename(url)` (nombre local; ver regla de `#/` y `?` en `transfer.md`).
4. `extract_dir = positional('extract_dir')`: si el manifiesto da lista, debe tener el mismo largo que las URL (si no: `'<campo>: cantidad distinta de URL'`); si es texto, se usa para todas. Vacío → `'.'`.
5. `selected = inno_root(extract_dir)` solo si `innosetup`; si no, `None`.
6. `extracted = archives.extract(archive, stage/'expanded-<i>', root, filename, log=log, innosetup=…, inno_root=selected or APP)`.
7. `source = inside(expanded, extract_dir if extracted else '.')`; `target = inside(payload, positional('extract_to'))`. Si `source` no es directorio: `'extract_dir no encontrado: {p1}'`.
8. Antes de copiar: si algún fichero de `source` ya existe en `target`: `'Dos artefactos intentan escribir el mismo archivo'`.
9. Copia: `move_whole` (renombrado único) o, si falla, `copytree` con `os.replace` por fichero. Logs `Archivos <key>: i/N` cada 1000 ficheros.
10. Tras todas las URL: si `innosetup`, `restore_declared_targets` (ver `inno.md`).
11. `run_pre_install` (ver `hooks.md`), después `run_installer`.
12. `Validando <key>` y comprobación de `bin` (`Comando ausente: …`).
13. Publicación: `payload.rename(destination)`.

Diferencias de `extract_to`: se resuelve dentro de `payload` (`inside`). Varias URL pueden tener `extract_to` como lista (mismo largo).

## Validación previa de `installer` (`catalog.installer_definition`, `compatibility`)

- Sin `installer`, `None`, `{}`, `{'script': ''}` o `{'script': []}`: compatible (ejemplos del test de `test_engine.py:509-511`).
- `installer` que no es diccionario (`True`, `'setup.exe'`, `[]`): `Pasos no soportados: installer`.
- Con `file`: solo se admiten `file` y `args`. Cualquier otra clave → `Pasos no soportados: installer.<clave>`.
- `file` debe ser texto con extensión `.ps1` (sin distinguir mayúsculas) y pasar `portable_path`. Si no: `Pasos no soportados: installer.file`.
- `args` debe ser texto o lista de textos. Si no: `installer.args debe ser texto o una lista de líneas`.
- Sin `file`: solo `script`, texto o lista de textos. Si no: `installer.script debe ser texto o una lista de líneas`.
- Todo esto se valida **antes de descargar** (`engine.install` → `installation_plan` → `compatibility`).

## Invariantes (de la suite y del motor)

1. Ningún fichero descargado se ejecuta: `.ps1` y `.exe` sueltos se copian (paso 7).
2. La extracción siempre se hace en `stage`, nunca en `payload`, y solo se publica el conjunto final.
3. Un fallo en cualquier paso previo a `Publicando` deja `installed()`, el perfil y el directorio `core/apps/<bucket>/<app>` como estaban.
4. Los hooks reciben `$fname` como texto si hay una URL y como lista si hay varias.
5. `$dir` y `$original_dir` apuntan al payload, y `home` (`$env:USERPROFILE`) apunta a `root/home`.
6. Los argumentos de `installer.file` se sustituyen solo para las variables documentadas (ver `hooks.md`).
7. Una URL sin hash declarado requiere confirmación antes de descargar (`confirm_unverified`).
8. Un hash incorrecto aborta antes de ejecutar cualquier hook.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_mixed_downloads_file_arguments_and_architecture` | `test_installer_files.py:63-79` | Con `architecture.64bit.installer` y dos URL (`sdk.zip` + `pyshim.ps1`): `pyshim.ps1` y `tool.py` en el payload; `tool.ps1` contiene `& python`; `arguments.json`: `destination` con el prefijo `mito installer spaces `; con argumentos como texto, `arguments = ['two words']` (el primer argumento lo consume `param($destination)`); con lista, `['', 'two words', '$(throw "injection")', '1', 'False', '$dir_suffix']` (`$dir` → destino; `${version}` → `1`; `$global` → `False`; `$(throw …)` y `$dir_suffix` literales); `names = ['sdk.zip','pyshim.ps1']`; `home = root/home`. |
| `test_bad_hash_prevents_all_hooks` | `test_installer_files.py:81-89` | Hash `0`×64 en la segunda URL → `Hash incorrecto`; `run_pre_install` y `run_installer` no se llaman; `installed()` vacío. |
| `test_unverified_script_requires_confirmation_before_download` | `test_installer_files.py:91-97` | Hash `None` en la segunda URL y `confirm_unverified` devuelve `False` → error y `download` no se llama. |
| `test_file_errors_preserve_installed_version` | `test_installer_files.py:99-111` | Tres scripts que fallan (`throw`, `exit 19`, borrar `tool.py` y `throw`) → `installer.file falló`; `installed()`, perfil y `tool.py` de la versión anterior iguales. |
| `test_invalid_file_modes_rejected_before_download` | `test_installer_files.py:113-124` | Nueve manifiestos inválidos (`../outside.ps1`, `C:/outside.ps1`, `setup.exe`, `setup.msi`, `script` + `file`, `keep`, `args: [7]`, `args` sin `file`, `file: False`) → error antes de descargar (`download` no se llama). |
| `test_missing_file_and_missing_archive_directory_are_not_ignored` | `test_installer_files.py:126-135` | `installer.file` inexistente → `No existe missing.ps1`; `extract_dir: 'missing'` → `extract_dir no encontrado`; no queda ninguna instalación. |

## Dependencias

- `engine`: `install`, `installation_plan`, `_install_payload`, `installed`, `profile`, `save_profile`.
- `archives.extract` (despacho), `inno.restore_declared_targets`, `hooks.run_pre_install`/`run_installer`, `bucket_resources.sources`/`prepare`.
- `transfer`: `download`, `download_hashes`, `download_filename`, `hash_verifier`, `extract_zip`.
- `catalog`: `installer_definition`, `compatibility`, `effective`.
- `storage`: `YitoError`, `inside`, `portable_path`, `write_json`.

## Notas para Rust

- El despacho de `archives.extract` se implementa como una función que devuelve `enum Extraction { Container, LooseFile }`. Mantener el orden de los criterios (ver Mapa de decisiones de `archives.md`).
- `download_filename`: hacer la regla de `#/` y de `?` idéntica (test `test_download_names_are_shared_and_confined`, `test_engine.py:590-601`).
- `positional(field)` y `extract_to` como lista: mismo largo que `url`.
- Los tests de integración con servidor local usan `ThreadingHTTPServer`; en Rust, `tiny_http` o `hyper` en un hilo, o un mock de `download`.
- `mito installer spaces ` con espacios en la ruta: la ruta del contexto JSON y del runner deben quedar entrecomilladas al pasarlas a PowerShell (aquí se pasan como argumentos, no como texto).

## Dudas

1. Con `args` de texto `'$dir "two words"'`, el test espera `arguments == ['two words']`. El motor sí expande `$dir` a la ruta del payload, pero `pyshim.ps1` tiene `param($destination)`, que recoge ese primer argumento. La lista `$args` solo contiene el resto. El resultado depende del script de prueba y no de una regla del motor.
2. El paso 7 de `archives.extract` copia `.ps1` sueltos, pero el mismo `.ps1` como `installer.file` se busca después en `payload`. Un `.ps1` de URL con `extract_dir` (p. ej. `sdk`) se ignora porque `extracted=False`. Es el comportamiento esperado, pero no está escrito en ningún sitio del original.
3. `Burn` no tiene rama en `archives.extract`. Un `setup.exe` de Burn en `url` sin `innosetup` se copia suelto. Confirmar que es lo que se quiere (ver `burn.md`).
4. `move_whole` y `copytree` pueden terminar con distinto orden de archivos, pero el resultado es el mismo. El log `Archivos <key>: i/N` depende del camino elegido (`LongStep`). No está cubierto por tests de `installer_files`.
5. `download_hashes` lanza `'La cantidad de hashes debe coincidir con las URL'` si la lista no coincide. El test `test_mixed_downloads…` usa listas de largo 2 correctas; no hay caso de largo distinto.
