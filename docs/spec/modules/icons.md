# icons.py → mito_win::icons

Origen: `core/app/mito/icons.py` (442 líneas). No existe `core/tests/test_icons.py`. La cobertura está en `core/tests/test_shortcuts.py`: `test_badge_color_changes_background_and_keeps_white_letters` (99-106), `test_badge_default_limit_and_profile_changes_keep_icon` (214-223), `test_badge_preserves_icon_and_native_sizes` (225-233), `test_declared_icon_is_used_and_refreshed_for_current_registry` (123-148) y `test_declared_icon_profile_scope_fallback_and_confinement` (150-167).

## Responsabilidad

Generar un `.ico` de acceso directo con una insignia de texto (1-4 caracteres) en la esquina inferior derecha, sobre el icono de la aplicación, con color de fondo de una paleta cerrada. Guarda el resultado en caché en disco. Extrae el icono de un `.exe` o `.dll` con la API de recursos de Windows.

## API pública del original

| Función / constante | Entradas | Salida | Errores |
|---|---|---|---|
| `create_mito_shortcut_icon(source, destination_directory, launcher_id, badge_text='MITO', badge_color='naranja')` | ruta del icono (`.ico`, `.exe` o `.dll`), carpeta de caché, id del lanzador, texto, color | `Path` del `.ico` (existente o nuevo) | `YitoError` (lista más abajo) |
| `BADGE_COLORS` | — | dict clave → RGB | — |
| `DEFAULT_BADGE_COLOR` | — | `'naranja'` | — |
| `_validate_badge_text(value)` | texto | texto normalizado | `YitoError` |
| `_read_icon_container(source)` | ruta | bytes del `.ico` | `YitoError` |
| `_extract_first_pe_icon(source)` | `.exe`/`.dll` | bytes de un `.ico` | `YitoError` (solo en Windows) |
| `_load_icon_frames(icon_bytes, source)` | bytes de `.ico` | lista de `Image` RGBA, una por tamaño | `YitoError` |
| `_add_mito_badge(frame, badge_text, color)` | `Image` RGBA | `Image` RGBA | — |
| `_add_small_badge(frame, badge_text, color)` | igual | igual | — |
| `_encode_png_ico(frames)` | lista de `Image` | bytes de `.ico` con PNG | `YitoError` si la lista está vacía |

`_BADGE_REVISION = b"mito-shortcut-badge-v5"` se usa en el digest de caché. Subirlo invalida toda la caché cuando cambia el render.

## Paleta `BADGE_COLORS` (copiada literalmente)

| Clave | RGB |
|---|---|
| `negro` | `(34, 34, 39)` |
| `rojo` | `(172, 35, 47)` |
| `azul` | `(26, 75, 160)` |
| `naranja` (por defecto) | `(196, 80, 19)` |
| `rosa` | `(157, 43, 100)` |
| `verde` | `(26, 112, 61)` |
| `marron` | `(109, 65, 35)` |
| `morado` | `(102, 54, 135)` |

## Algoritmo completo

### A. Generación (`create_mito_shortcut_icon`)

1. `badge_text = _validate_badge_text(badge_text)`: `strip().upper()`; longitud 1-4 y `isalnum()`. Si no, `YitoError('shortcut.badgeText debe contener entre 1 y 4 letras o números')`. Este texto es distinto del de `shortcuts.badge_text` (`La etiqueta admite…`): `shortcuts` valida antes, así que este mensaje solo aparece si se llama directamente.
2. Si `badge_color` no está en `BADGE_COLORS`: `YitoError('Elige un color de la paleta del icono')`.
3. `source = source.resolve()`. Si no es archivo: `YitoError('No existe el icono del launcher: {p1}')`.
4. `icon_bytes = _read_icon_container(source)`:
   - sufijo `.ico` (sin distinguir mayúsculas): `read_bytes()`; error de lectura → `YitoError('No se pudo leer el icono {p1}: {p3}')`;
   - sufijo `.exe` o `.dll`: `_extract_first_pe_icon(source)`;
   - otro: `YitoError('Formato de icono no soportado para el acceso directo: {p1}')`.
5. `digest = sha256(icon_bytes + _BADGE_REVISION + badge_text.encode('utf-8') + badge_color.encode('ascii')).hexdigest()[:16]`.
6. `destination = (dir / f"{launcher_id}-mito-{digest}.ico").resolve()`. Si no está dentro de `dir` (`relative_to`): `YitoError('El icono generado sale de su directorio: {p1}')`.
7. Si `destination` ya existe: devuelve la ruta (caché). No se regenera.
8. `frames = [_add_mito_badge(f, badge_text, BADGE_COLORS[badge_color]) for f in _load_icon_frames(icon_bytes, source)]`.
9. `generated = _encode_png_ico(frames)`.
10. Escritura atómica: `NamedTemporaryFile(dir=dir, prefix=f".{launcher_id}-mito-", suffix=".tmp", delete=False)`; `write`, `flush`, `fsync`; `os.replace(temporal, destination)`. Si hay `OSError`: borra el temporal y lanza `YitoError('No se pudo guardar el icono MITO: {p1}')`.

### B. Carga de tamaños (`_load_icon_frames`)

1. Abre `icon_bytes` con Pillow. `sizes = set(base.info.get('sizes') or {base.size})`. Fallo de lectura → `YitoError('No se puede interpretar el icono {p1}')`.
2. Filtra tamaños con `0 < w <= 256` y `0 < h <= 256`, ordenados por `(área, (w, h))`. Si no queda ninguno: `YitoError('El icono no contiene tamaños válidos: {p1}')`.
3. Para cada tamaño: reabre la imagen; si el formato es `ICO`, asigna `frame.size = size`; `load()`; `convert('RGBA')`. Fallo → `YitoError('No se puede leer el tamaño {p1}x{p3} de {p5}')`.

### C. Insignia grande (lado más corto > 48 px): `_add_mito_badge`

1. `frame = frame.convert('RGBA')`; `shortest = min(w, h)`. Si `shortest <= 48` → `_add_small_badge`.
2. `overlay` transparente de `(w, h)`; `draw = ImageDraw.Draw(overlay)`.
3. `font_size = max(5, round(shortest * 0.23))`.
4. Bucle:
   - `font = ImageFont.load_default(size=font_size)`;
   - `bbox = draw.textbbox((0, 0), text, font=font)`; `tw = bbox[2]-bbox[0]`; `th = bbox[3]-bbox[1]`;
   - `hpad = max(1, round(shortest * 0.035))`; `vpad = max(1, round(shortest * 0.018))`;
   - `bw = tw + 2*hpad`; `bh = th + 2*vpad`;
   - si `bw <= w` o `font_size <= 4`: sale; si no, `font_size -= 1`.
5. `left = w - bw`; `top = h - bh`; `radius = max(1, round(shortest * 0.04))`; `outline = max(1, round(shortest * 0.008))`.
6. `draw.rounded_rectangle((left, top, w-1, h-1), radius=radius, fill=(*color, 244), outline=(255,255,255,245), width=outline)`.
7. Posición del texto: `x = left + (bw - tw)/2 - bbox[0]`; `y = top + (bh - th)/2 - bbox[1]`. `draw.text((x, y), text, font=font, fill=(255,255,255,255))`.
8. Devuelve `Image.alpha_composite(frame, overlay)`.

### D. Insignia pequeña (lado más corto <= 48 px): `_add_small_badge`

1. `shortest = min(w, h)`; `font_size = max(4, round(shortest * 0.3))`; `padding = max(1, shortest // 32)`.
2. Bucle igual al de C, pero `bw = tw + 2*padding` y `bh = th + 2*padding`; corta si `bw <= w` o `font_size <= 4`.
3. `left = w - bw`; `top = h - bh`. Rectángulo sólido sin contorno ni esquinas: `draw.rectangle((left, top, w-1, h-1), fill=(*color, 255))`.
4. Posición del texto igual que en C, con `fill=(255,255,255,255)`.
5. `draw.fontmode = "1"` (sin antialias) antes de `draw.text`.
6. Devuelve `alpha_composite(frame, overlay)`.

### E. Fuente

- `ImageFont.load_default(size=n)` devuelve la fuente que incluye Pillow. Si FreeType está disponible (lo habitual en las ruedas de Windows), es una fuente vectorial escalable, no un mapa de bits propio. **No hay tabla de glifos que copiar**: `icons.py` no define ninguna.
- Versión fijada: `pillow-12.3.0` en `core/bootstrap.lock.json`. El parámetro `size` de `load_default` se usa tal cual; la familia exacta de fuente y su licencia deben confirmarse con esa versión (ver dudas).

### F. Codificación ICO (`_encode_png_ico`)

1. `unique = {frame.size: frame}` (gana el último de cada tamaño); orden por `(área, (w, h))`. Vacío → `YitoError('No hay imágenes para construir el icono MITO')`.
2. Para cada imagen: `save(buffer, format='PNG', optimize=True)`.
3. Cabecera `<HHH>` = `(0, 1, n)`.
4. Entrada de directorio de 16 bytes `<BBBBHHII>`: ancho (`0` si `>= 256`), alto (`0` si `>= 256`), colores `0`, reservado `0`, planos `1`, bits `32`, tamaño del PNG, desplazamiento (inicia en `6 + 16*n`, avanza con cada imagen).
5. Resultado: cabecera + entradas + imágenes PNG concatenadas.

### G. Extracción de icono de un PE (`_extract_first_pe_icon`, solo Windows)

Si `os.name != 'nt'`: `YitoError('La extracción de iconos de ejecutables requiere Windows')`.

1. `LoadLibraryExW(ruta, NULL, LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)` (`0x2 | 0x20`). Si falla: `YitoError('Windows no pudo abrir los recursos de {p1}: error {p3}')` con `GetLastError`.
2. `EnumResourceNamesW(módulo, RT_GROUP_ICON = 14)` recoge los nombres. Enteros `<= 0xFFFF` son IDs; el resto son cadenas wide. Sin nombres: `YitoError('El ejecutable no contiene un icono extraíble: {p1}')`.
3. Usa solo el **primer** grupo (`names[0]`):
   - `FindResourceW` → `SizeofResource` → `LoadResource` → `LockResource`. Fallo → `YitoError('Windows no pudo localizar un recurso de icono en {p1}')`, o `YitoError('Windows no pudo cargar un recurso de icono en {p1}')` si la carga o el bloqueo fallan o el tamaño es `<= 0`.
4. `_group_icon_to_ico(grupo)`:
   - `len < 6` → `YitoError('Recurso de icono truncado en {p1}')`;
   - `(reserved, type, count)` = `<HHH`; si `reserved != 0`, `type != 1` o `count < 1` → `YitoError('Recurso de icono no válido en {p1}')`;
   - `len < 6 + 14*count` → `YitoError('Directorio de icono truncado en {p1}')`;
   - por cada entrada de 14 bytes `<BBBBHHIH` (ancho, alto, colores, reservado, planos, bits, tamaño, **ID de recurso**): carga el `RT_ICON` (3) con ese ID y escribe la entrada de 16 bytes `<BBBBHHII` con el desplazamiento acumulado.
5. Siempre `FreeLibrary(módulo)` en `finally`.

## Formato de disco

- Caché: `core/state/icons/<launcher_id>-mito-<digest>.ico`. Ejemplo: `core/state/icons/app-25caa424deb27105-4030fe15-mito-<16 hex>.ico`. `launcher_id` lo genera `shortcuts.py` con `identifier` (minúsculas).
- El digest depende de los bytes del icono origen, la revisión, el texto y el color. Dos llamadas con los mismos datos devuelven el mismo archivo sin regenerarlo.
- El `.ico` generado tiene 3 tamaños con la prueba del original (16, 32, 256); el número real depende del icono de origen (los tamaños del `.ico` de origen, filtrados a `<= 256`).
- Los archivos antiguos de la caché no se borran nunca. El original no tiene limpieza.

## Invariantes

1. El color de fondo de la insignia siempre es el de `BADGE_COLORS`; el texto siempre es blanco.
2. Mismos datos de entrada → mismo nombre de archivo. Un cambio de color, texto o icono de origen genera un archivo nuevo; el anterior queda en disco.
3. El `.ico` de salida sirve en todos los tamaños del origen `<= 256` px; los tamaños se ordenan de menor a mayor área.
4. El destino siempre queda dentro de `destination_directory` (comprobado con `resolve` y `relative_to`).
5. La escritura es atómica: un `.ico` a medias nunca aparece con el nombre final.
6. `badge_text` en mayúsculas.
7. Lo que se extrae de un PE son los bytes de un `.ico` válido construido a partir del primer grupo del ejecutable.

## Casos límite y errores

- Insignia de 4 caracteres en un icono de 16 px: `font_size` baja hasta 4 y el bucle para aunque `bw > w`. La insignia puede salirse del icono por la izquierda (`left < 0`). El original no lo comprueba.
- Un `.ico` con un solo tamaño de 256: solo hay un frame; los tamaños pequeños no se generan.
- Un `.exe` sin grupos `RT_GROUP_ICON`: error de «no contiene un icono extraíble».
- Un `.ico` con cabecera declarando `sizes` vacíos: `supported_sizes` vacío → error.
- Un `.ico` que Pillow no puede leer: `No se puede interpretar el icono`.
- `source` no resoluble o inexistente: `No existe el icono del launcher`.
- Directorio de caché no escribible: `No se pudo guardar el icono MITO`.

## Casos de test del original

| Test (`test_shortcuts.py`) | Líneas | Qué comprueba |
|---|---|---|
| `test_badge_color_changes_background_and_keeps_white_letters` | 99-106 | Para cada color: `_add_mito_badge` sobre un frame de 32 px deja el píxel (31,31) igual a `(*rgb, 255)` y aparece blanco `(255,255,255,255)` en los datos. Usa la rama pequeña. |
| `test_badge_default_limit_and_profile_changes_keep_icon` | 214-223 | Por defecto `MITO`; `ab12` → `AB12`; `ABCDE`, `A B`, `!` dan error; desactivar el componente no cambia el archivo del icono. |
| `test_badge_preserves_icon_and_native_sizes` | 225-233 | El `.ico` tiene exactamente `{16, 32, 256}`; el píxel (0,0) conserva el fondo azul; la mitad inferior derecha tiene más de un color; `DEV` genera otro archivo. |
| `test_declared_icon_is_used_and_refreshed_for_current_registry` | 123-148 | Un icono rojo se usa; al cambiar el origen a verde se genera otro archivo con píxel verde; el `.lnk` se actualiza y solo queda uno. |
| `test_declared_icon_profile_scope_fallback_and_confinement` | 150-167 | Un icono inválido en el payload da una insignia azul (el icono de MITO); si el icono falta, el `component_icon_source` cae al ejecutable; `C:/outside.ico` da error. |

## Dependencias

- `i18n.tr` (todos los mensajes salvo los de `_validate_badge_text`, que también pasan por `tr`).
- `storage.YitoError`.
- Pillow (`Image`, `ImageDraw`, `ImageFont`, `UnidentifiedImageError`), `ctypes` y `wintypes`, `struct`, `tempfile`, `hashlib`, `os`, `io`.
- Llamadores: `shortcuts.application_icon` (`shortcuts.py:36-47`), `shortcuts.py:131`, `shortcuts.py:189`, `shortcuts.py:214`; `host_apps.shortcut` (`host_apps.py:175`).

## Notas para Rust

- `mito-win` ya tiene `resources.rs` con `group_icons` y `replace_group_icons` (RT_ICON, RT_GROUP_ICON, `LoadLibraryExW`). Reutilizar la lectura del grupo; `_group_icon_to_ico` y el armado del `.ico` pueden compartir código.
- Generación de imagen: el workspace no tiene crate de imágenes. Hace falta una dependencia (`image` con `png` y el redimensionado nativo, o `ico`) y aprobarla con ADR. Pillow hace el alpha compositing con `alpha_composite`; la crate `image` tiene el equivalente `imageops::overlay` con alfa.
- La fuente: si Rust no puede usar la misma fuente que Pillow, el resultado será visualmente distinto aunque la lógica sea idéntica. Propuesta: incluir una fuente TTF/OTF con licencia compatible en `assets/` y documentar el cambio en ADR. Confirmar qué fuente usa Pillow 12.3.0 antes de decidir.
- Mantener las constantes `0.23`, `0.3`, `0.035`, `0.018`, `0.04`, `0.008`, los alfas `244`/`245`/`255` y el umbral `48` exactamente como en el original.
- `_BADGE_REVISION` debe conservar el valor `mito-shortcut-badge-v5` hasta que cambie el render. Así no se regenera la caché existente ni se rompe la compatibilidad de nombres.
- `sha2` ya está en el workspace; el digest se calcula sobre `icon_bytes` y los literales de la revisión.

## Dudas

1. La fuente de `ImageFont.load_default(size=…)` en Pillow 12.3.0 no está confirmada en este contrato. Hay que verificar en una instalación (`python -c "from PIL import ImageFont; ..."`) qué familia y licencia tiene, antes de decidir qué fuente lleva Rust.
2. El umbral `shortest <= 48` define dos estilos distintos (rectángulo sin contorno frente a rectángulo redondeado con contorno y alfa 244). ¿Es intencional que la rama pequeña no tenga contorno ni alfa?
3. Cuando el bucle de tamaño de fuente termina por `font_size <= 4`, la insignia puede no caber (`bw > w`). El original no lo trata. ¿Debe recortarse o dar error?
4. `_extract_first_pe_icon` usa solo el primer grupo de iconos. Si un `.exe` tiene varios grupos, ¿es esa la intención?
5. La caché nunca se limpia. ¿Debe limpiarse en la versión Rust (por ejemplo, al borrar accesos)?
6. `_validate_badge_text` acepta letras Unicode (`isalnum`). Con una fuente sin esos glifos, el resultado se ve mal. Ver también `shortcuts.md`, duda 6.
7. `frame.size = size` solo se aplica si el formato es `ICO`. ¿Qué pasa con un origen que Pillow no reconoce como ICO pero tiene varios tamaños? En la práctica el origen siempre es `ICO` por la extensión.
