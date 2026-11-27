# burn.py → mito_win::burn

Origen: `core/app/mito/burn.py` (201 líneas), `core/tests/test_burn.py` (176 líneas, 6 tests). Referencia de formato: `wixtoolset/wix3` `src/tools/wix/BurnCommon.cs` y `BurnReader.cs` (docstring, línea 3).

Nota de crate: PARITY.md asigna `burn.py` a `mito-win`. Usa `mito-transfer` (`MAX_EXPANDED`, `hash_verifier`) y `mito-storage` (`inside`, `portable_path`).

## Responsabilidad

Leer un bundle WiX 3 Burn v2 (`setup.exe` con la sección `.wixburn`) como datos, sin ejecutarlo. Se extraen los dos CAB adjuntos (contenedor UX y contenedor de payloads) con el 7-Zip privado, se valida el manifiesto XML, se comprueban hashes y tamaños, y se publica el resultado en `UX/` y `AttachedContainer/`. Solo se extraen payloads `embedded`; los `external` se omiten con un aviso.

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `containers(stream)` (18) | stream binario seekable | lista `[(offset, size), (offset, size)]`: UX y adjunto | `YitoError('Burn: …')` (ver «Formato») |
| `inventory(manifest, *, log=…)` (69) | bytes del manifiesto XML | `plans`: lista de 2 listas de `(source, target, size|None, digest|None)` | `YitoError('Burn: …')` |
| `extract_burn(archive, destination, root, *, log=…)` (137) | ruta del bundle, destino, raíz MITO | `None` (publica en `destination`) | `YitoError` por contenedores, manifiesto, CAB, tamaños, hashes, conflictos |
| `NS` (15) | `'{http://schemas.microsoft.com/wix/2008/Burn}'` | — | — |

## Formato binario

Todas las lecturas usan `read(offset, size)`, que comprueba `offset ≥ 0`, `size ≥ 0`, `offset + size ≤ longitud` (error `'Burn: truncated or invalid container range'`) y que se leen exactamente `size` bytes (`'Burn: truncated header'`).

### Cabecera PE

| Paso | Offset | Tamaño | Campo | Comprobación y error |
|---|---|---|---|---|
| DOS | 0 | 64 | `e_magic` = `MZ` | `'Burn: missing DOS header'` |
| DOS | 60 | u32 LE (`<I`) | `e_lfanew` = `pe` | — |
| PE | `pe` | 24 | `PE\0\0` | `'Burn: missing PE header'` |
| PE | `pe`+6 | u16 LE (`<H`) | `NumberOfSections` = `count` | `1 ≤ count ≤ 96`, si no `'Burn: invalid PE section count'` |
| PE | `pe`+20 | u16 LE | `SizeOfOptionalHeader` = `optional` | — |
| Secciones | `pe+24+optional` | `count × 40` | tabla de encabezados de sección | — |

Cada entrada de sección tiene 40 bytes. Se busca la que empieza por `.wixburn` (8 bytes, `sections[i:i+8]`). Debe haber **exactamente una**: si no, `'Burn: missing or duplicate .wixburn section'`.

De esa entrada: `raw_size, offset = <II` en `entrada+16` (`SizeOfRawData`, `PointerToRawData`). Si `raw_size < 56`: `'Burn: short section'`.

### Cabecera de la sección `.wixburn` (56 bytes, en `offset`)

| Offset relativo | Tipo | Nombre en `containers` | Significado |
|---|---|---|---|
| 0 | u32 | `magic` | Debe ser `0x00F14300` |
| 4 | u32 | `version` | Debe ser `2` |
| 24 | u32 | `stub` | Offset del CAB UX (contenedor de manifiesto y UI); el stub ejecutable va antes |
| 28 | u32 | (ignorado) | — |
| 32 | u32 | `signature` | Offset de la firma del motor (0 si no hay firma) |
| 36 | u32 | `signature_size` | Tamaño de la firma |
| 40 | u32 | `kind` | Debe ser `1` |
| 44 | u32 | `count` | Debe ser `2` |
| 48 | u32 | `ux` | Tamaño del CAB UX |
| 52 | u32 | `attached` | Tamaño del CAB adjunto |

Comprobaciones (`'Burn: only v2 bundles with UX and one attached CAB are supported'`): `magic`, `version`, `kind`, `count` exactos.

Después (`'Burn: invalid or overlapping containers'`):

- `start = signature + signature_size` si `signature != 0`; si no, `start = stub + ux`.
- Error si `stub < offset + 56`, `start < stub + ux`, `ux == 0`, o `attached == 0`.
- Resultado: `[(stub, ux), (start, attached)]`.

Para cada par `(position, size)`:

- Error `'Burn: truncated or oversized CAB'` si `size > MAX_EXPANDED` (12 GiB) o `position + size > longitud`.
- Se leen 36 bytes en `position`. Deben cumplirse: `cab[0:4] == b'MSCF'`, `u32 @8 == size` (`cbCabinet`), y `u16 @30 & 3 == 0` (sin spanning). Si no: `'Burn: invalid or spanning CAB'`.

En el bundle de prueba: sección en offset 512, cabecera en 512 (magic), `<8I` en 536, `stub=1024`, `ux=100`, `attached=100`, CAB en 1024 y 1124 (o 1144 con firma de 20 bytes en 1124).

### Contenedores

- Índice 0 = CAB UX. Dentro hay un fichero llamado `0`, que es el manifiesto XML.
- Índice 1 = CAB adjunto. Sus ficheros tienen los nombres de `SourcePath` (`a0`, `a1`, …).

## Manifiesto XML

Límite: `len(manifest) > 16 MiB` → `'Burn: oversized manifest'`.

Decodificación: `utf-8-sig`. Error `'Burn: manifest must be UTF-8'`.

Rechazado si el texto contiene `\0`, `<!DOCTYPE` o `<!ENTITY` (en mayúsculas): `'Burn: unsupported XML declaration or oversized manifest'`.

`ET.fromstring(text)`. Error de parseo: `'Burn: invalid manifest XML'`.

La raíz debe ser `NS + 'BurnManifest'`: si no, `'Burn: unsupported manifest namespace'`.

Grupos de payloads (`groups`):

1. `grupo 0` = `BurnManifest/UX/Payload` (contenedor UX).
2. `grupo 1` = `BurnManifest/Payload` (contenedor adjunto).

Total de `Payload`: si > 150 000: `'Burn: oversized payload inventory'`.

### Atributos de `Payload`

| Atributo | Uso | Regla |
|---|---|---|
| `Packaging` | modo de empaquetado | por defecto `embedded`; se compara en minúsculas. `external` solo se omite en el **grupo 1**; en el grupo 0 cualquier valor distinto de `embedded` lanza `'Burn: unsupported payload packaging'`. |
| `SourcePath` | nombre del fichero dentro del CAB | obligatorio; `portable_path(source).parts` debe tener un solo segmento; si no, `'Burn: invalid payload source'` |
| `FilePath` | destino relativo | obligatorio; `portable_path`; `parts` no vacío (si no, `f'Burn: empty payload destination for {source}'`) |
| `FileSize` | tamaño declarado | opcional. `int()`; error `'Burn: invalid payload size'` si no es entero o es negativo. Se suma a `total`. |
| `Hash` | digest declarado | opcional. `hash_verifier(digest)` (de `transfer.py`) valida el formato; ver `transfer.md`. |

Duplicados por `SourcePath` en casefold dentro del mismo grupo: `f'Burn: duplicate payload source: {source}'`.

Colisión archivo/directorio por grupo: si un padre de `FilePath` coincide (casefold) con otro `FilePath` del mismo grupo: `'Burn: file/directory collision'`.

Los duplicados de **destino** entre grupos o dentro del mismo grupo no se rechazan aquí. Se resuelven en `extract_burn` comparando bytes.

Límites: `sum(len(plans)) > 150 000` o `total > MAX_EXPANDED`: `'Burn: oversized payload inventory'`.

Externos: si hay al menos uno, `log(f'Burn: {n} external payload reference(s) omitted; extracting embedded files only')` (f-string, sin `tr`). Sus `SourcePath`, `FilePath` y `DownloadUrl` no se resuelven ni se descargan (test `mixed_manifest`).

Salida: `plans = [plan_ux, plan_adjunto]`, donde cada plan es `[(source, target_posix, size, digest), …]`.

## Extracción (`extract_burn`, 137-201)

1. `destination.parent.mkdir`. Se abre el archivo y se crea `scratch = TemporaryDirectory(dir=destination.parent)`.
2. `ranges = containers(stream)`. `remaining = MAX_EXPANDED`.
3. Para cada contenedor `index`:
   a. Se copia el rango en bloques de 1 MiB a `scratch/{index}.cab`. Si se corta: `'Burn: truncated CAB'`.
   b. `extract_7zip(cabinet, scratch/{index}, root, max_expanded=remaining)`. Lo llama como `archives.extract_7zip` (import diferido).
   c. `remaining -= Σ size de ficheros de la carpeta`. Si `< 0`: `'Burn: oversized containers'`.
4. `manifest = expanded[0]/'0'`. Si no es fichero o pesa más de 16 MiB: `'Burn: missing or oversized manifest'`.
5. `plans = inventory(manifest.read_bytes(), log=log)`.
6. Para cada `(folder, plan)`:
   a. `expected = {source.casefold()}`; si es el índice 0, se añade `'0'`.
   b. `actual = {nombre.casefold() de ficheros de primer nivel}`. Si `actual != expected` o hay subcarpetas: `'Burn: CAB contents do not match manifest'`.
   c. Por payload: `file = inside(folder, source)`. Si `size` declarado y `file.stat().st_size != size`: `'Burn: payload size mismatch'`.
   d. Hash: `hasher, expected_hash = hash_verifier(digest)`. Si hay `hasher`, se hashea en bloques de 1 MiB. Si `hexdigest().lower() != expected_hash`: `'Burn: payload hash mismatch'`.
   e. `relative = ('UX/' if index == 0 else 'AttachedContainer/') + target`. Si `relative.casefold() == 'ux/manifest.xml'`: `'Burn: reserved manifest destination'`.
   f. Si ya existe un destino igual en casefold: se compara con `filecmp.cmp(previous, file, shallow=False)`. Si difieren: `f'Burn: conflicting payloads for destination: {relative}'`. Si son iguales: `log(f'Burn: identical payload merged: {relative}')` y se omite la copia.
7. Publicación (solo si todo ha pasado): `copies` incluye primero `(manifest, 'UX/manifest.xml')`. Para cada entrada: `target = inside(destination, relative)`, `mkdir(parents=True)`, `shutil.copyfile`.

Orden de validación: todo se verifica antes de escribir en `destination` (test `test_all_payloads_verified_before_output_is_written`).

## Verificación de hashes

- `hash_verifier(digest)` (`transfer.py`) acepta `None`, `''`, `skip` (sin hash, `(None, None)`), `algo:digest`, o un digest hex de 32/40/64/128 caracteres (md5/sha1/sha256/sha512).
- Un digest inválido (p. ej. `bad`) lanza error ya en `inventory`.
- Con `hasher` la comparación es estricta, en minúsculas.
- Sin hash declarado no hay comprobación de contenido; solo tamaño (si se declaró).

## Uso de 7-Zip

Cada CAB se extrae con `archives.extract_7zip`, que usa el binario privado `core/tools/7zip/7z.exe` y valida el listado previo. Los CAB del bundle se escriben a disco como `<índice>.cab` en `scratch`. No se pasa ninguna contraseña.

## Mensajes

- `log` (`tr`): ninguno en este módulo.
- `log` (sin `tr`): `Burn: {n} external payload reference(s) omitted; extracting embedded files only`, `Burn: identical payload merged: {relative}`.
- Errores: todos `YitoError` con cadena literal, sin `tr`. Lista completa en la sección «Formato», «Manifiesto» y «Extracción».

## Invariantes

1. El bundle nunca se ejecuta. Se lee con `open('rb')` y `struct`.
2. Un solo `.wixburn`, versión 2, `kind=1`, `count=2`, CAB no espaciados (sin bits `prev`/`next`).
3. Los CAB adjuntos y UX se verifican (`MSCF`, tamaño, flags) antes de extraer.
4. El manifiesto UTF-8, sin DOCTYPE ni ENTITY, y con namespace de WiX Burn.
5. Solo payloads `embedded` se extraen. Los `external` nunca se descargan ni se resuelven.
6. Los nombres pasan por `portable_path`/`inside`. Ningún destino puede caer en `UX/manifest.xml`.
7. Tamaño y hash (cuando se declaran) se comprueban **antes** de publicar.
8. Dos payloads con el mismo destino solo se permiten si sus bytes son idénticos.
9. Nada se escribe en `destination` hasta que todo el bundle está validado.
10. Total ≤ 12 GiB; número de payloads ≤ 150 000.

## Casos límite y errores

- Bundle truncado (`containers` con `bundle()[:1150]`): `'Burn: truncated or invalid container range'`.
- Offsets que se solapan, CAB con tamaño 0, `ux` o `attached` 0: `'Burn: invalid or overlapping containers'`.
- `count` de PE fuera de `1..96`: `'Burn: invalid PE section count'`.
- Dos secciones `.wixburn`: `'Burn: missing or duplicate .wixburn section'`.
- Manifiesto con `SourcePath="../a0"`, `FilePath="C:/escape"`, `FilePath="NUL.txt"` (reservado): error de `portable_path`.
- `FileSize="-1"`: `'Burn: invalid payload size'`.
- `Hash="bad"`: error de `hash_verifier`.
- `FilePath="nested/tool.msi/child"` con `nested/tool.msi` en el mismo grupo: `file/directory collision`.
- `DOCTYPE` con entidades (`<!DOCTYPE x [<!ENTITY e "test">]>`): rechazado antes del parseo.
- Destino `NESTED/TOOL.MSI` y `nested/tool.msi` con bytes distintos: `conflicting payloads for destination: …`.
- Payload del CAB con bytes distintos del hash: `payload hash mismatch`.
- CAB con un fichero de más (`unknown`): `CAB contents do not match manifest`.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_extracts_real_cabs_using_private_7zip` | `test_burn.py:49-66` | Bundle real: CAB generados con `makecab`, `extract_burn` con 7-Zip privado; `AttachedContainer/nested/tool.msi` contiene `tool`. El `external` de `mixed_manifest` se omite. |
| `test_reads_container_offsets_with_and_without_engine_signature` | `test_burn.py:68-71` | `containers` devuelve `(1024, 100), (1124|1144, 100)` con y sin firma. |
| `test_rejects_truncated_unsupported_overlapping_and_spanning_containers` | `test_burn.py:73-81` | Siete escrituras de u32 sobre offsets de la sección y del CAB (`516`, `560`, `552`, `556`, `544`, `1054` (flags del CAB), `564`) y un bundle truncado (`[:1150]`) → `YitoError`. |
| `test_rejects_unsafe_duplicate_unknown_packaging_and_xml_payloads` | `test_burn.py:83-93` | 11 manifiestos inválidos (rutas, `NUL.txt`, `SourcePath` fuera, `Packaging="unknown"`, externo en UX, `FileSize=-1`, `Hash=bad`, duplicado de `a0`, colisión, DOCTYPE) → `YitoError`. |
| `test_external_references_are_excluded_from_embedded_inventory_and_logged` | `test_burn.py:95-99` | `inventory(mixed)` == `inventory(embedded)`; se registra un único aviso con `1 external payload reference(s) omitted`. |
| `test_repeated_destinations_require_identical_extracted_bytes` | `test_burn.py:101-132` | Tres combinaciones (`tool` con hash; `evil` con hash; `evil` sin hash). Bytes idénticos: se fusiona y se registra `identical payload merged`. Bytes distintos: `conflicting payloads…TOOL.MSI` y no se crea `output`. |
| `test_all_payloads_verified_before_output_is_written` | `test_burn.py:134-159` | Cuatro casos (`tool`, `evil`, `tool`+extra, sin `a0`). Solo el primero publica; los demás fallan sin crear `output`. No hay descargas (`mito.transfer.download` no se llama). |
| `test_hook_helpers_are_confined_and_msi_uses_existing_extractor` | `test_burn.py:161-176` | Ver `hooks.md`. Usa `expand_installer('msi')` y destinos fuera del payload. |

Los tests que parchean `mito.archives.extract_7zip` (`test_burn.py:121,149,168`) simulan el CAB: el parche escribe `0` y `a0` / `a1` en la carpeta destino.

## Dependencias

- `storage`: `YitoError`, `inside`, `portable_path`.
- `transfer`: `MAX_EXPANDED`, `hash_verifier`.
- `archives.extract_7zip` (import diferido).
- Llamadores: solo `hooks.expand_installer('dark')` (`Expand-DarkArchive`). `archives.extract` no tiene rama Burn: un `setup.exe` de Burn sin `innosetup` se copia suelto (ver Dudas).
- `i18n.tr`: no se usa.

## Notas para Rust

- Leer con `std::io::Read + Seek`, sin mapear el fichero entero. `read_exact` equivale a `read(offset, size)` con las comprobaciones de rango.
- Parseo binario con `u32::from_le_bytes` sobre `[u8; N]` leídos en cada offset. No hay crate necesario.
- XML: `quick-xml` (o `roxmltree`). Hay que rechazar `DOCTYPE` y `ENTITY` antes de parsear, igual que el original.
- Hashes: `sha1`, `sha2`, `md-5` (solo para comparar; el original usa `usedforsecurity=False`). Ver `transfer.md`.
- `filecmp.cmp(shallow=False)` equivale a comparar bytes con `std::fs::read` o en bloques.
- El CAB se extrae con el 7-Zip privado igual que en `msi`. Mismo `extract_7zip`.
- El formato del bundle usa `FileSize` y `Hash` opcionales. Mantener la lógica de «sin hash = sin comprobación de contenido».

## Dudas

0. `archives.extract` no tiene rama para Burn (ni `.exe` ni firma). Un `setup.exe` de Burn descargado sin `innosetup` se copia como archivo suelto y solo se puede usar con `Expand-DarkArchive`. Confirmar que es el comportamiento deseado para el catálogo.
1. La cabecera `.wixburn` tiene un campo sin nombre en offset 28 (`_`). Confirmar con `BurnCommon.cs` que no es relevante (el original lo ignora).
2. `start = signature + signature_size if signature else stub + ux` ignora `signature_size` cuando `signature == 0`. ¿Puede haber `signature_size > 0` sin `signature`?
3. `containers` acepta `magic` y `version` de la sección pero no valida `stub` contra la cabecera PE. ¿Basta con el solapamiento para evitar ataques de offsets?
4. `extract_burn` mezcla payloads de UX y adjuntos en `destinations` con el prefijo (`UX/` o `AttachedContainer/`). Un payload UX con `FilePath="x"` y uno adjunto con `FilePath="x"` no colisionan. Esto es intencionado según el código, pero no lo cubre ningún test.
5. El texto del aviso de externos y del de fusión no se traduce. Decidir si se quedan así.
6. `hash_verifier` acepta `algo:digest` (p. ej. `sha256:…`). El manifiesto de Burn usa `Hash` sin prefijo en los tests. Confirmar si WiX produce `Hash` en hexadecimal sin prefijo.
7. `inventory` valida `Hash` aunque el payload sea `external` (ese se omite antes, así que no se valida). Consistente con el código, pero conviene documentarlo.
8. El cálculo de `remaining` suma tamaños de archivos del CAB con `rglob` y no con los del manifiesto: un CAB con un fichero extra fallará después, no aquí.
