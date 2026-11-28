# open_with.py + open_with_ui.py → mito-win::open_with y mito-tui::open_with_ui

Origen: `core/app/mito/open_with.py` (278 líneas), `core/app/mito/open_with_ui.py` (130 líneas),
`core/tests/test_open_with.py` (9 tests: 7 de registro y 2 de diálogo). Botones en `ui.py`:
`openwith-<n>` «Extensiones ✓/✗» (líneas 1409-1418) y rama `open-with` (línea 1767).

## Responsabilidad

Entradas «Abrir con» de Windows por componente. Cada registro crea un ProgID propio por perfil, componente
y acceso directo (`HKCU\Software\Classes\MITO.<16 hex>`) cuyo comando es el mismo lanzador que los accesos
directos (`mito run --profile --app`), de modo que aplican el entorno del profile, el workspace y las copias por
profile. Las extensiones las elige el usuario (los manifiestos Scoop no declaran tipos de archivo). La
aplicación predeterminada nunca se cambia: Windows la protege. Los registros viven en `core/state/open-with.json`.

## API pública del original

| Función / constante | Entradas | Salida | Errores |
|---|---|---|---|
| `REGISTRY`, `CLASSES` | — | `'core/state/open-with.json'`, `r'Software\Classes'` | — |
| `COMMON`, `PRESETS` | — | sugerencias del diálogo (ver «Tablas») | — |
| `winreg()` (46) | — | módulo `winreg` (se importa al llamar; permite parchear) | — |
| `registry(root)` (51) | raíz | `dict` ProgID → registro, o `{}` | `read_json` |
| `save(root, records)` (56) | raíz, dict | `None` | E/S |
| `extensions(text)` (60) | texto libre | lista ordenada sin duplicados de `.ext` | `YitoError` |
| `progid(profile, app, shortcut=None)` (76) | strings | `'MITO.' + sha256(seed)[:16]` | — |
| `display_name(profile, app, shortcut=None)` (81) | strings | nombre visible | — |
| `command(engine, profile, app, shortcut=None)` (86) | motor (solo `engine.root`), strings | línea de comando | — |
| `notify()` (94) | — | `None`; llama `SHChangeNotify` | — |
| `set_value(key, name, value)` (100) | clave abierta | `None` (`REG_SZ`) | `OSError` |
| `write_progid(record, command_line, icon)` (105) | registro, comando, icono | `None` | `OSError` |
| `delete_tree(path)` (128) | ruta bajo HKCU | `None` (borra recursivamente; ignora `FileNotFoundError`) | `OSError` |
| `remove_progid(record)` (143) | registro | `None` | `YitoError` si el ProgID no es válido |
| `current_command(record)` (158) | registro | comando registrado o `None` | — |
| `icon_for(engine, lock, profile, shortcut, badge=None, color=None)` (167) | motor, lock, … | ruta del `.ico` generado | ver `shortcuts.py` |
| `register(engine, profile, installation, text, shortcut=None)` (176) | perfil, instalación, texto de extensiones | `record` | `YitoError` |
| `entries(root, profile, installation)` (210) | raíz, perfil, instalación | lista de registros | — |
| `unregister(root, select, log=print)` (215) | raíz, predicado, log | lista de registros retirados | captura y registra |
| `apply(engine, profile, installation, wanted, log=print)` (234) | motor, perfil, instalación, `{atajo: [ext]}` | `None` | igual que `register` |
| `relocate(engine, old, copied, log=print)` (251) | motor, antigua raíz, `copied` | número de registros afectados | captura y registra |

`identifier` (storage) normaliza el perfil (`register` y `apply` lo exigen).

## Claves de registro (HKCU)

Todas bajo `HKCU\Software\Classes` (`CLASSES`). `(Predeterminado)` = valor con nombre vacío. Todos son `REG_SZ`
salvo `OpenWithProgids`. Ejemplo con perfil `default`, aplicación `main/foliate`, acceso directo `Foliate`
(`progid` = `MITO.38613277e964aa60`, `display_name` = `default - main-foliate - Foliate`):

```
HKCU\Software\Classes\MITO.38613277e964aa60
    (Predeterminado)                 REG_SZ  default - main-foliate - Foliate
    FriendlyTypeName                 REG_SZ  default - main-foliate - Foliate
    DefaultIcon\
        (Predeterminado)             REG_SZ  <icono>,0
    Application\
        ApplicationName              REG_SZ  default - main-foliate - Foliate
        ApplicationIcon              REG_SZ  <icono>,0
        ApplicationDescription       REG_SZ  MITO
    shell\open\
        FriendlyAppName              REG_SZ  default - main-foliate - Foliate
    shell\open\command\
        (Predeterminado)             REG_SZ  <raíz>\core\tools\python\pythonw.exe -B -I -X utf8 -m mito run --profile default --app main/foliate --shortcut Foliate -- "%1"

HKCU\Software\Classes\.pdf\OpenWithProgids
    MITO.38613277e964aa60            REG_NONE  (vacío, sin datos)
HKCU\Software\Classes\.epub\OpenWithProgids
    MITO.38613277e964aa60            REG_NONE  (vacío, sin datos)
```

- `<icono>` es la ruta de `icon_for` (ver Dudas). `<raíz>` es `engine.root`.
- No se escribe nada bajo `Software\Classes\Applications\…`: el nombre «Application» aparece solo como subclave
  del ProgID (ver Dudas).
- `OpenWithProgids` usa `REG_NONE` con datos vacíos (`b''`), y el nombre de valor es el ProgID.

**Sin shortcut** (`shortcut=None`): la semilla es `profile/app`; por ejemplo `default/main/foliate`
→ `MITO.ef616740509e44aa`, y el nombre visible es `default - main-foliate` (sin sufijo). El comando omite `--shortcut`.

Lo que se escribe en cada registro (`write_progid`), en este orden: valor predeterminado y `FriendlyTypeName`;
`DefaultIcon` (predeterminado `icono,0`); `Application` (`ApplicationName`, `ApplicationIcon`, `ApplicationDescription = MITO`);
`shell\open` (`FriendlyAppName`); `shell\open\command` (predeterminado = comando); y por cada extensión
`.<ext>\OpenWithProgids` (valor = ProgID, `REG_NONE`).

## Comando del lanzador

`command()` construye `subprocess.list2cmdline([pythonw, '-B', '-I', '-X', 'utf8', '-m', 'mito', 'run', '--profile', profile, '--app', app, ('--shortcut', shortcut)?])` y añade literalmente ` -- "%1"`.

- `pythonw` = `<engine.root>\core\tools\python\pythonw.exe`.
- `list2cmdline` entrecomilla los argumentos que contienen espacios o están vacíos.
- Ejemplo: `<root>\core\tools\python\pythonw.exe -B -I -X utf8 -m mito run --profile default --app main/foliate --shortcut Foliate -- "%1"`.

## Formatos de disco

**Registro** `core/state/open-with.json` (`write_json`: UTF-8, `indent=2`, `ensure_ascii=False`, salto final). Clave:
`progid`. Valor: un registro:

```json
{
  "MITO.38613277e964aa60": {
    "progid": "MITO.38613277e964aa60",
    "profile": "default",
    "installation": "main/foliate",
    "app": "main/foliate",
    "shortcut": "Foliate",
    "extensions": [".pdf", ".epub"]
  }
}
```

- `shortcut` es `null` cuando la entrada es la principal (sin atajo).
- `extensions` tiene el orden de inserción y sin duplicados.
- El registro también guarda `installation` (clave del componente `bucket/app`) y `app` (= `lock['key']`).

Otros archivos: `core/state/icons/…` (iconos, generados por `shortcuts.application_icon`), y el lock
`mutation.lock` durante `register`/`apply` (`mutation(root)`).

## Invariantes

1. `extensions` admite texto separado por espacios, comas o punto y coma. Pasa a minúsculas, añade el punto si falta, valida
   con `^\.[a-z0-9][a-z0-9_+-]{0,15}$` (1 a 16 caracteres tras el punto) y elimina duplicados conservando el orden.
2. `register` exige que el componente esté en `profile['apps']` y que `application_target(lock, shortcut)` resuelva
   (si el atajo no existe o es ambiguo, `YitoError`). Un componente inactivo no genera registro.
3. El ProgID siempre tiene la forma `MITO.<16 hex minúsculas>`. `remove_progid` rechaza cualquier otro valor
   (`Registro de «Abrir con» no válido`), así que nunca borra claves ajenas (test `test_foreign_progid_is_never_deleted`).
4. `remove_progid` borra solo el valor `ProgID` de cada `OpenWithProgids` y el árbol `MITO.<hex>`. Nunca toca el valor
   predeterminado de `.ext` ni la asociación de usuario.
5. `register` con un ProgID existente primero retira el registro anterior (`remove_progid(previous)`) y luego escribe.
   Si la escritura falla, retira el nuevo y elimina el registro del JSON (el anterior también se pierde; ver Dudas).
6. `notify()` (`SHChangeNotify(SHCNE_ASSOCCHANGED=0x08000000, 0, None, None)`) se llama en `finally` de `register`,
   tras `unregister` con cambios y tras `relocate` con cambios. El Explorador recarga asociaciones sin reiniciar.
7. `apply` es idempotente: si los valores de un atajo coinciden con los registrados, no hace nada (test: `register` no se llama).
   Lista vacía = retirar la entrada de ese atajo.
8. `unregister` mantiene los registros no seleccionados y solo guarda y notifica si retiró algo.
9. `relocate` (tras mover la instancia): si el comando del registro contiene la antigua raíz, reescribe comando e icono
   con la nueva raíz. Si se copió la instancia (`copied=True`), elimina el registro y deja la clave en la instancia
   original. Devuelve el número de registros afectados (ambos casos cuentan).
10. El predeterminado de la extensión (`.pdf` sin subclave de usuario) no se modifica nunca.

## Casos límite y errores

| Clave (texto español) | Cuándo |
|---|---|
| `Extensión no válida: {p1}` | un token no cumple la expresión (`p1` = token en minúsculas tal como se escribió) |
| `Indica al menos una extensión` | texto vacío o solo separadores |
| `El componente debe estar activo en el profile` | `register`: la instalación no está en `apps` |
| `Registro de «Abrir con» no válido` | `remove_progid` con ProgID que no cumple `MITO\.[0-9a-f]{16}` |
| `No se pudo retirar «Abrir con» {p1}: {p3}` | `unregister` o `relocate`: error de registro (`p1` = clave, `p3` = excepción) |
| `Registrado en «Abrir con» para {p1}. Puede tardar unos segundos en aparecer en el Explorador.` | `apply` tras `register` (`p1` = extensiones unidas por espacio) |
| `Retirado de «Abrir con».` | `apply` con lista vacía |

Errores de `application_target` (texto del motor, p. ej. `Shortcut unavailable or ambiguous` en los tests) se propagan.

## Tablas

`PRESETS` (nombre visible → extensiones; el nombre no pasa por `tr`):

| Nombre | Extensiones |
|---|---|
| Word | .docx .doc .docm .dotx .dot .rtf .odt |
| Excel | .xlsx .xls .xlsm .xlsb .xltx .csv .ods |
| PowerPoint | .pptx .ppt .pptm .ppsx .pps .potx .odp |
| PDF / eBook | .pdf .epub .mobi .azw3 .djvu .xps .cbz .cbr |
| 7-Zip | .7z .zip .rar .tar .gz .tgz .bz2 .xz .zst .iso .cab .wim |
| VLC | .mp4 .mkv .avi .mov .wmv .webm .flv .mpg .mpeg .m4v .3gp .mp3 .flac .wav .ogg .m4a .aac .opus .wma |
| IrfanView | .png .jpg .jpeg .gif .bmp .webp .tif .tiff .ico .heic |
| Notepad++ | .txt .log .md .ini .cfg .conf .json .xml .yaml .yml .toml .ps1 .bat .cmd .py .js .ts .html .css |

`COMMON` (sugerencias del desplegable «Habituales», en este orden): .pdf .epub .djvu .cbz .txt .md .rtf .csv .json
.xml .html .log .docx .doc .xlsx .xls .pptx .ppt .odt .ods .odp .png .jpg .jpeg .gif .bmp .webp .svg .tif .ico
.mp3 .flac .wav .ogg .m4a .aac .opus .mp4 .mkv .avi .mov .webm .wmv .zip .7z .rar .tar .gz .py .js .ts .java .c
.cpp .cs .go .rs .ps1 .bat .ini .yaml .toml.

## Diálogo `OpenWithDialog` (open_with_ui.py)

Constructor: `OpenWithDialog(name, entries=(), current=None)`. `entries` son tuplas `(label, executable, extra)`;
`current` es `{label: [extensiones]}`. Resultado: `{label: [extensiones]}` para **todas** las entradas (lista vacía
= retirar), o `None` si se cancela. Clase `ModalScreen[object]`, `BINDINGS = [('escape','cancel','Cancelar')]`.
Fondo `$background 70%`.

Widgets, en orden, con sus ids (CSS `#open-with-dialog`: ancho 76, altura auto, borde redondeado `#b5dc64`, fondo `#153429`):

| Id / tipo | Texto o valor | Notas |
|---|---|---|
| `Label.title` | `Abrir con` | `tr` |
| `Static` | nombre de la aplicación | `markup=False` |
| `Select#open-with-entry` | opciones `(label, label)`, sin vacío | solo si hay más de una entrada |
| `Label` | `Extensiones registradas` | `tr` |
| `DataTable#open-with-table` | una fila por extensión; columna `key='extension'`; sin cabecera; `cursor_type=row`; `zebra_stripes`; alto 8 | |
| `Select#open-with-preset` | prompt `Presets`; opciones `Nombre (n)` → extensiones unidas por espacio | `tr` del prompt |
| `Select#open-with-common` | prompt `Habituales`; opciones = `COMMON` | `tr` del prompt |
| `Input#open-with-extensions` | placeholder `Extensiones (por ejemplo: pdf epub)` | `tr` |
| `Button#open-with-add` | `Añadir` | `tr` |
| `Button#open-with-remove` | `Quitar` | `tr` |
| `Static` (sin id) | `Aparecerá en «Abrir con» del menú contextual de esos archivos y se abrirá con el entorno del profile. No cambia la aplicación predeterminada.` | `tr`, `markup=False` |
| `Static#open-with-error` | vacío o mensaje de error | color `#ff9090` |
| `Button#open-with-cancel` | `Cancelar` | `tr` |
| `Button#open-with-confirm` | `Guardar` (variant primary) | `tr` |

Comportamiento:
- Montaje: añade columna, rellena la tabla de la entrada seleccionada y da foco a `#open-with-extensions`.
- Entrada inicial: primera etiqueta con extensiones en `current`, o la primera etiqueta.
- `Select` de preset o common: al cambiar, añade su valor (`add`) y limpia el select. Valor vacío se ignora.
- `Select` de entrada: cambia `self.entry`, limpia el error y repinta la tabla.
- `add(text)`: `extensions(text)`; si falla, muestra el mensaje en `#open-with-error` y devuelve `False`; si no, añade
  las que falten, limpia el error y repinta.
- `Quitar`: borra la fila con el cursor (solo si `0 <= cursor_row < n`).
- `Enter` en el campo: `add` y vacía el campo si tuvo éxito.
- `Añadir`: igual que Enter.
- `Guardar`: si el campo tiene texto y `add` falla, no cierra. Si no, cierra con `{label: list(values)}` de todas las entradas.
- `Cancelar`, `Esc` o cualquier otro botón: `dismiss(None)`.

Cadenas traducibles del diálogo: `Abrir con`, `Extensiones registradas`, `Presets`, `Habituales`,
`Extensiones (por ejemplo: pdf epub)`, `Añadir`, `Quitar`, `Guardar`, `Cancelar`, y la frase informativa anterior.
Los nombres de preset y las extensiones son datos literales.

Uso desde `ui.py`:
- Botón `openwith-<n>`: `current = {record.get('shortcut'): record['extensions'] for record in entries(root, profile, installation)}`; `OpenWithDialog(lock['key'], engine.application_entries(lock), current)`; si el resultado no es `None` → `begin('open-with', profile, target, arguments=result)`, que llama `apply(engine, profile, target, arguments, log)`.
- Etiqueta del botón: `Extensiones ✓` si hay registros para el componente y perfil, `Extensiones ✗` si no. Se muestra con el componente activo y `can_open(lock)`.

## Casos de test del original

| Test | Archivo:líneas | Qué comprueba |
|---|---|---|
| `test_extensions_are_normalized_and_validated` | test_open_with.py:61-65 | `PDF, .epub;djvu pdf` → `['.pdf','.epub','.djvu']`; `''`, `'  '`, `'a/b'`, `'..pdf'` y 20 caracteres → `YitoError`. |
| `test_register_writes_progid_with_profile_launcher` | :67-82 | ProgID con 16 hex; comando con `pythonw.exe`, `run --profile default --app main/foliate --shortcut Foliate` y ` -- "%1"`; `ApplicationName` correcto; `OpenWithProgids` marcado en `.pdf` y `.epub`; al re-registrar con `djvu` desaparece `.pdf`. |
| `test_inactive_component_or_unknown_entry_is_rejected` | :84-89 | Instalación inactiva y atajo inexistente → `YitoError`; registro vacío. |
| `test_unregister_removes_only_selected_records` | :91-98 | `unregister` por perfil borra solo ese ProgID (clave y `OpenWithProgids`); el otro perfil conserva su registro. |
| `test_apply_adds_replaces_and_removes_extensions` | :100-113 | `apply` añade, amplía sin perder, no llama a `register` si no cambia, y con `[]` retira todo. |
| `test_foreign_progid_is_never_deleted` | :115-117 | `remove_progid` con `Applications\notepad.exe` → `YitoError`. |
| `test_relocation_rewrites_after_move_and_releases_after_copy` | :119-129 | Tras mover, el comando contiene la nueva raíz; tras copiar, el comando conserva la antigua y el registro queda vacío. |
| `test_dialog_lists_adds_from_common_and_text_and_removes` (UI) | test_open_with.py:133-163 | Muestra `.docx`; elegir `.pdf` en `common`; escribir `mp3, AVI` y añadir; quitar la primera fila; Guardar → `{'Foliate': ['.pdf','.mp3','.avi']}`. |
| `test_preset_adds_its_extensions_without_duplicates` (UI) | :165-188 | Todos los presets son válidos y sin duplicados; elegir `Word` añade sus extensiones; Guardar → `{'Office': [...Word]}`. |

## Dependencias

- `i18n.tr`; `storage`: `YitoError`, `identifier`, `mutation`, `read_json`, `write_json`.
- `shortcuts`: `application_icon`, `badge_text`, `badge_color`, `component_icon_source` (icono y etiqueta del ProgID).
- `engine`: `profile(name)`, `selected(data)`, `application_target(lock, shortcut)`, `application_entries(lock)`, `root`.
- `winreg` (módulo estándar de Windows); `ctypes` para `SHChangeNotify`; `hashlib`; `re`; `subprocess`.
- Tests: `async_support` (diálogo con Textual).
- `protocols.py` reutiliza `command`, `icon_for`, `notify`, `delete_tree`.

## Notas para Rust

- `mito-win::open_with`: API de registro HKCU con el crate `winreg` (o `windows-registry`). `SHChangeNotify` en `mito-win`, con `// SAFETY:`.
- Escribir `OpenWithProgids` como `REG_NONE` con 0 bytes (`set_raw_value` con `RegType::REG_NONE`).
- `progid`: SHA-256 de los bytes UTF-8 de `profile/app[/shortcut]`, primeros 16 caracteres hex. Hash de referencia:
  `default/main/foliate/Foliate` → `38613277e964aa60`; `default/main/foliate` → `ef616740509e44aa`.
- `extensions`: la expresión regular se reproduce con `regex` o con validación manual.
- `mito-tui::open_with_ui`: `ratatui` según ADR 0011; los ids de widgets pasan a identificadores de foco. Los textos y el orden se mantienen.
- El campo de `command` depende del lanzador Rust (ver Dudas, punto 2).

## Dudas

1. **«Applications» no existe en el original.** El encargo menciona `Applications`, pero `open_with.py` no escribe `HKCU\Software\Classes\Applications\…`. Solo escribe la subclave `Application` dentro del ProgID. Confirmar que el port tampoco debe crearla.
2. **Lanzador.** El comando usa `core/tools/python/pythonw.exe -m mito run`. El port Rust necesita otro lanzador (`mito.exe run` o similar). Hay que decidir si el comando registrado cambia (instalaciones existentes) y documentarlo en ADR. Hasta entonces el contrato es el de la fuente.
3. **Icono.** `icon_for` devuelve la ruta que genera `shortcuts.application_icon` (no revisado en este contrato); los nombres de archivo `open-<tag>-<hash8>` se heredan del original. Confirmar la carpeta (`core/state/icons`).
4. **Pérdida del registro anterior.** En `register`, si ya existía el ProgID, se retira antes de escribir; si la escritura falla, el `except` elimina también la entrada del JSON. El registro previo no se restaura. ¿Debe restaurarse?
5. **Etiqueta vs atajo.** `apply` usa la etiqueta de `application_entries` como clave de atajo, y `ui.py` construye `current` con `record.get('shortcut')` (`None` para la entrada principal). Si la etiqueta de la entrada principal no es `None`, los valores existentes no se muestran y `apply` podría registrar con atajo inválido. Confirmar cómo `application_entries` etiqueta la entrada principal.
6. **Relocate con copia.** `relocate(copied=True)` elimina el registro sin tocar la clave, que sigue apuntando a la instancia original. Confirmar que es el comportamiento deseado (igual que `host_exposure.relocate`).
7. **Cadena traducida como clave de icono.** `icon_for` incluye `badge` y `color` en el nombre del icono; si cambian, se genera un icono nuevo y el antiguo queda huérfano. Sin limpieza en este módulo.
8. **Mensaje de extensión.** `Extensión no válida: {p1}` muestra la entrada tal cual (en minúsculas). Si el texto del usuario contiene caracteres de control, se muestran en el log. No hay test.
9. **Nombres de preset.** `Word`, `Excel`, `PowerPoint`, `PDF / eBook`, `7-Zip`, `VLC`, `IrfanView`, `Notepad++` son marcas: no se traducen (correcto), pero conviene confirmar que no se muestran como texto de dominio público.
