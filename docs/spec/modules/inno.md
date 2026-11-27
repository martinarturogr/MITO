# inno.py → mito_win::inno

Origen: `core/app/mito/inno.py` (139 líneas), `core/tests/test_inno.py` (177 líneas, 10 tests).

Nota de crate: PARITY.md asigna `inno.py` a `mito-win`. Usa el binario `innounp` privado, fijado en `core/tools/innounp/innounp.exe` (versión 2.71.1 según `archives.md`).

## Responsabilidad

Extraer como datos los archivos bajo `{app}` (o bajo un constante `{code_*}`) de un instalador Inno Setup, usando `innounp` privado. Nunca se ejecuta el instalador. Antes de publicar, se valida el listado completo y se compara con lo extraído. Es el camino de `archives.extract` cuando `innosetup: true` y el archivo termina en `.exe` (`archives.py:40`), y de `hooks.expand_inno` (`Expand-InnoArchive`).

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `APP = '{app}'` (14) | constante | — | — |
| `inno_root(extract_dir)` (17) | valor de `extract_dir` | constante elegida (`'{app}'`, `'{code_X}'`) o `None` | — (no lanza) |
| `restore_declared_targets(payload, targets)` (29) | carpeta payload, lista de rutas relativas declaradas | `None`; puede crear archivos | `YitoError('Inno: ambiguous variants for {relative}')` |
| `app_inventory(listing, *, root=APP, max_entries=150000, max_expanded=MAX_EXPANDED)` (53) | salida texto de `innounp -v` | `dict[casefold] = (relative_posix, size)` | `YitoError('Inno: …')` |
| `extract_inno(archive, destination, root, constant=APP)` (99) | instalador, destino, raíz MITO, constante | `None` | `YitoError('Inno: …')`, `YitoError` de herramienta incompleta |

## Argumentos exactos de `innounp`

Constantes del código:

```
common = ['-b', '-n', '-o', '-h', '-u', '-a']
```

Según los comentarios del original (`inno.py:106-107`): `-b` no pide nada (batch), `-n` no adivina formatos más nuevos, `-a` desempaqueta solo una copia de los datos compartidos, y la salida es la regional o no (ver Dudas para `-o`, `-h`, `-u`). El original no permite ejecutar el instalador en ningún caso.

Invocaciones (`run`, líneas 109-115):

| Paso | Argumentos (en este orden) | `cwd` | Salida usada |
|---|---|---|---|
| Listado | `innounp.exe -v -b -n -o -h -u -a <archive>` | `stage` (temporal) | `stdout` → `app_inventory` |
| Extracción | `innounp.exe -x -b -n -o -h -u -a -c<constant> -d<stage>/files <archive> <constant>\*` | `stage` | ninguna (se escriben ficheros) |

Ejemplo literal de extracción con `constant='{app}'`: `-x -b -n -o -h -u -a -c{app} -d<stage>\files <archive> {app}\*`. El argumento final es `constant + '\\*'`, con una barra invertida literal.

`subprocess.run([str(executable), *arguments], cwd=cwd, stdin=DEVNULL, capture_output=True, text=True, encoding='utf-8', errors='strict', creationflags=CREATE_NO_WINDOW)` (solo si `subprocess` tiene `CREATE_NO_WINDOW`).

Si `returncode != 0`: `YitoError(f'Inno: extraction tool failed ({rc}): {stderr.strip() or stdout.strip()}')`.

## `inno_root(extract_dir)` (líneas 17-26)

Devuelve el valor de `extract_dir` si es texto y:

- `extract_dir.casefold() == '{app}'`, o
- coincide con `\{code_[A-Za-z_][A-Za-z0-9_]*\}` (regex literal, distingue mayúsculas: `{CODE_x}` no vale).

Si no, devuelve `None`. Resultados que el test fija:

| Entrada | Salida |
|---|---|
| `'{code_GetDestDir}'` | `'{code_GetDestDir}'` |
| `'{app}'` | `'{app}'` |
| `'{sys}'`, `'{tmp}'`, `'{code_x}\\sub'`, `'sub'`, `'{code:GetDestDir}'`, `None` | `None` |

Un `'{APP}'` se devuelve tal cual (sin normalizar a `{app}`). La comparación es sin distinguir mayúsculas.

## Parseo del listado: `app_inventory` (líneas 53-96)

Regex literal de las filas:

```
^\s*(\d+)\s+\d{4}-\d{2}-\d{2} \d{2}:\d{2} {5}(.+)$
```

Grupo 1 = tamaño en bytes; grupo 2 = nombre de fichero (puede tener espacios; se conservan). Las cinco espacios antes del nombre vienen de `ExtSp` (`innounp` rellena la fecha de 16 a 19 caracteres y añade dos separadores; el comentario de la línea 61-62 lo explica).

Estados:

1. Antes de la cabecera: se ignoran líneas hasta una que empiece por `Size` y contenga `Filename` (`started = True`).
2. Una línea que empieza por `---`:
   - si ya hay filas (`count > 0`): `ended = True` y se sale del bucle;
   - si no: se ignora (la línea de separación justo tras la cabecera).
3. Cualquier otra línea debe cumplir el regex completo (`fullmatch`). Si no: `'Inno: unrecognized file listing'`. Esto incluye líneas vacías tras la cabecera.
4. Por fila:
   - `path = portable_path(name)`; debe tener partes y `str(path) == name.replace('/', '\\')`. Si no: `'Inno: non-canonical file path'`.
   - `folded = str(path).casefold()`; `parents` = casefold de padres distintos de `.`.
   - Si `folded` ya está en `all_paths` o en `directories`, o si `parents ∩ all_paths` no está vacío: `'Inno: duplicate or overlapping file path'`.
   - `all_paths.add(folded)`; `directories.update(parents)`; `count += 1`; `total += size`.
   - Si `count > max_entries` o `total > max_expanded`: `'Inno: archive exceeds extraction limits'`.
   - Si `path.parts[0].casefold() == root.casefold()` y hay más de una parte: `relative = PureWindowsPath(*parts[1:]).as_posix()`; `entries[relative.casefold()] = (relative, size)`.
5. Al final: si no se vio cabecera, no se cerró con `---`, o no hay entradas: `f'Inno: no supported {root} payload or incomplete listing'` (sin `tr`).

Notas:

- Solo se devuelven los archivos bajo `root`. Las otras raíces (`{tmp}`, etc.) cuentan para límites y duplicados, pero no se extraen.
- `root` se compara en casefold, pero `relative` conserva la capitalización del listado.
- Los límites de `app_inventory` son los de línea 53; `extract_inno` no los pasa, así que usa los valores por defecto (150 000 y `MAX_EXPANDED`).

## `restore_declared_targets(payload, targets)` (líneas 29-50)

Inno renombra archivos duplicados con sufijo `,N` (`Editor.exe`, `Editor,1.exe`, `Editor,2.exe`). Esta función restaura el nombre declarado cuando las copias numeradas son idénticas.

Para cada `relative` (sin repetidos, en orden):

1. `target = inside(payload, relative)`. Si ya existe, o si su carpeta padre no es directorio: se salta.
2. Patrón: `re.escape(stem) + r',([1-9]\d*)' + re.escape(suffix)`, sin distinguir mayúsculas (`re.I`).
3. Copias: entradas de `target.parent` que cumplen el patrón, son fichero y no son enlace. Se guardan como `(n, path)`.
4. Se ordenan por `n`. Si hay menos de 2, o los `n` no son exactamente `1..k` consecutivos: se salta sin error.
5. Se calcula `sha256` (`hashlib.file_digest`) de cada copia. Si hay más de un hash distinto: `YitoError(f'Inno: ambiguous variants for {relative}')`.
6. Si todos son iguales: `shutil.copy2(copies[0][1], target)`.

Las copias numeradas no se borran.

## Extracción: `extract_inno` (líneas 99-139)

1. `inno_root(constant) is None` → `YitoError(f'Inno: unsupported root {constant}')`.
2. `executable = root/'core/tools/innounp/innounp.exe'`. Si no es fichero: `YitoError(tr('Herramienta incompleta: {p1}', p1='innounp'))`. Clave i18n: `Herramienta incompleta: {p1}`.
3. `with TemporaryDirectory(dir=archive.parent) as stage`.
4. `inventory = app_inventory(run(['-v', *common, archive], stage), root=constant)`.
5. `output = stage/'files'`; `mkdir()`.
6. `run(['-x', *common, f'-c{constant}', f'-d{output}', archive, constant + '\\*'], stage)`.
7. Se recorre `output.rglob('*')`:
   - enlace o junction → `YitoError('Inno: unmanaged link in extracted payload')`;
   - `relative = path.relative_to(output).as_posix()`; `inside(output, relative)` valida;
   - si es fichero: si `relative.casefold()` ya está → `'Inno: duplicate extracted file'`; si no, `actual[folded] = (path, size)`.
8. Si `{key: size for actual}` ≠ `{key: size for inventory}`: `'Inno: extracted inventory differs from validated listing'`.
9. `destination.mkdir(parents=True, exist_ok=True)`. Para cada entrada de `inventory`: `target = inside(destination, relative)`, `mkdir` del padre, `shutil.copy2(actual[casefold][0], target)`.

Nada se publica en `destination` hasta que el listado y lo extraído coinciden.

Nota: el stage está dentro de `archive.parent`, no del destino. Esto evita que el instalador original se mezcle con la extracción.

## Invariantes

1. `innounp` es el único ejecutable que se lanza, y nunca se lanza el instalador (`inno.py:107`).
2. Se usa `stdin=DEVNULL`: `innounp` no puede pedir confirmación.
3. El listado se parsea de forma estricta (`fullmatch`): cualquier línea desconocida aborta.
4. Solo se extraen archivos bajo la constante; las demás raíces no se escriben.
5. Ninguna ruta sale de `destination` (`inside`) y ninguna ruta puede ser enlace.
6. Nombres canónicos: `\` como separador, sin `.` ni `..`, sin nombres reservados (validado por `portable_path`).
7. Sin solapamientos: un fichero no puede ser padre de otro ni repetirse en casefold.
8. Tamaño total ≤ 12 GiB, entradas ≤ 150 000.
9. La extracción solo se publica si el inventario del listado coincide exactamente con lo extraído (nombre casefold y tamaño).
10. Los archivos duplicados (`,N`) solo se restauran si todas las variantes son idénticas.

## Casos límite y errores

- Herramienta ausente: `YitoError` antes de cualquier `subprocess.run` (test `test_missing_tool_does_not_publish`).
- `innounp` con código ≠ 0: `Inno: extraction tool failed (<rc>): <stderr o stdout>`.
- Listado vacío, sin cabecera, o sin cierre `---`: `Inno: no supported {app} payload or incomplete listing` o `unrecognized file listing`.
- Solo archivos fuera de `{app}` (p. ej. `{tmp}\helper.exe`): `no supported … payload`.
- Dos entradas que difieren solo en mayúsculas: `duplicate or overlapping file path`.
- `{app}\file` y `{app}\file\child` (en cualquier orden): `duplicate or overlapping file path`.
- Nombre reservado (`NUL.txt`): error de `portable_path`.
- Ruta absoluta o fuera (`C:\escape`, `..\escape`): error de `portable_path`.
- Fila con línea en blanco tras la cabecera: `unrecognized file listing`.
- Límites: `max_expanded` menor que el total, o `max_entries=0`: `archive exceeds extraction limits`.
- Tamaño extraído distinto del listado: `extracted inventory differs from validated listing`.
- Un enlace en el payload extraído: `unmanaged link in extracted payload`.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_hook_helper_extracts_app_inside_payload_only` | `test_inno.py:19-43` | `expand_inno` con `{app}` y `{code_GetDestDir}` llama a `extract_inno` con esas constantes; un destino fuera del payload y `{sys}` fallan. |
| `test_declared_variants_must_be_identical` | `test_inno.py:45-60` | `Editor,1.exe` y `Editor,2.exe` iguales → se restaura `Editor.exe`. Distintos → `ambiguous variants` y no se crea `Editor.exe`. `Undeclared` no se toca. |
| `test_existing_target_is_preserved` | `test_inno.py:62-68` | Si `Editor.exe` existe, no se sobrescribe. |
| `test_normalizes_app_and_omits_other_roots` | `test_inno.py:70-73` | `{app}\Sub\Editor.exe` → `{'sub/editor.exe': ('Sub/Editor.exe', 3)}`; `{tmp}\helper.exe` se omite. |
| `test_scripted_destination_constant_selects_its_root` | `test_inno.py:75-84` | Con `root='{code_GetDestDir}'` se extraen sus archivos; `inno_root` acepta solo `{app}` y `{code_X}`. |
| `test_engine_extracts_the_declared_inno_root_instead_of_a_subfolder` | `test_inno.py:86-111` | `engine._install_payload` llama a `archives.extract` con `innosetup=True`, `inno_root='{code_GetDestDir}'`; el resultado es `Editor.exe` en la raíz (no en subcarpeta). |
| `test_rejects_unsafe_or_ambiguous_paths` | `test_inno.py:113-121` | Seis casos (`..\escape`, `C:\escape`, duplicado en mayúsculas, padre/hijo en ambos órdenes, `NUL.txt`) → `YitoError`. |
| `test_rejects_limits_missing_payload_and_incomplete_table` | `test_inno.py:123-131` | Límite de tamaño `2` con total 3; `max_entries=0`; sin raíz `{app}`; listado sin `---` final; salida desconocida → `YitoError`. |
| `test_only_private_tool_runs_and_inventory_precedes_publication` | `test_inno.py:133-169` | Con dos contenidos (`app`, `wrong size`): se llama dos veces a `innounp`, con `-b -n -a`; `payload` no existe durante el listado; `-c{app}` y `{app}\*` en la extracción. Con `wrong size` → `YitoError` y sin `payload`. |
| `test_missing_tool_does_not_publish` | `test_inno.py:171-177` | Sin `innounp` → `YitoError`, `subprocess.run` no se llama, sin `payload`. |

## Dependencias

- `storage`: `YitoError`, `inside`, `portable_path`.
- `transfer.MAX_EXPANDED`.
- `i18n.tr`: `Herramienta incompleta: {p1}`.
- `hashlib`, `re`, `shutil`, `subprocess`, `tempfile`, `pathlib.PureWindowsPath`.
- Llamadores: `archives.extract` (rama Inno, `archives.py:40-41`); `engine.py:336-354` (pasa `innosetup` e `inno_root`); `engine.py:409-412` (`restore_declared_targets`); `hooks.expand_inno` (`Expand-InnoArchive`).

## Notas para Rust

- Ejecutar `innounp` con `std::process::Command`, `creation_flags(0x08000000)` (CREATE_NO_WINDOW), `stdin(Stdio::null())` y salida UTF-8 estricta (`String::from_utf8`, error si no es válida).
- Parsear el listado con `regex` (mismo patrón); la lógica de estados es línea a línea.
- Hash con `sha2` (`Sha256`), en bloques; no hace falta cargar el archivo entero.
- El orden de los argumentos de `innounp` debe mantenerse igual: los tests de Rust comprobarán la lista exacta.
- Constante `APP = "{app}"` y `inno_root` como función pura.
- `inside`/`portable_path` del crate de almacenamiento.

## Dudas

1. El original documenta `-n`, `-b` y `-a`, pero no explica `-o`, `-h` ni `-u` (el comentario de `app_inventory` menciona «UTF-8 no regional», que puede ser `-u`). Verificar con `innounp -?` antes de fijar la lista de Rust.
2. `inno_root('{APP}')` devuelve `'{APP}'`, y ese valor se pasa a `innounp` tal cual. Confirmar que `innounp` acepta la constante sin distinguir mayúsculas, o normalizar a `{app}`.
3. `app_inventory` falla con cualquier línea en blanco después de la cabecera. Confirmar que el `innounp` fijado nunca emite líneas vacías dentro de la tabla.
4. `restore_declared_targets` usa `copies[0]` (el número 1), no un hash por nombre. Es correcto mientras todas las variantes sean idénticas, pero no comprueba que `Editor.exe` original sea idéntico a `,1`. ¿Debe comprobarse?
5. `extract_inno` no pasa `max_expanded` ni `max_entries` a `app_inventory`. Si el instalador supera el límite global, se detecta en el listado, así que es suficiente. Confirmar.
6. El stage está dentro de `archive.parent` (carpeta de staging de MITO). ¿Debe Rust usar siempre `core/staging`?
7. `restore_declared_targets` se llama con `shortcuts[*][0]` y las rutas de `bins`. Si un nombre de acceso directo tiene `\` (ruta en subcarpeta), `inside` lo acepta. Confirmar con un caso real del catálogo.
8. `innounp` puede escribir mensajes en `stdout` que contengan líneas `---` antes de la tabla. La lógica de cabecera lo maneja solo si aparecen tras `Size … Filename`. Verificar.
