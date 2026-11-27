# configuration_ui.py, profile_management_ui.py, dialog_header.py, localized_ui.py, native_ui.py → mito-tui::{configuration_ui, profile_management_ui, dialog_header, localized_ui, native_ui}

Origen:
- `core/app/mito/configuration_ui.py` (129 líneas): `SettingRow`, `ConfigurationDialog`.
- `core/app/mito/profile_management_ui.py` (82 líneas): `ManageProfileDialog`, `DeleteProfileDialog`.
- `core/app/mito/dialog_header.py` (22 líneas): `DialogHeader`.
- `core/app/mito/localized_ui.py` (17 líneas): `translate_bindings`, `ModalScreen`.
- `core/app/mito/native_ui.py` (15 líneas): `localize_script`.
- Dependencias de datos: `core/app/mito/configuration.py` (146 líneas; `PROPERTIES`, `PREFIXES`, `parse`, `validated`, `Configuration`), que se describe aquí solo en lo que afecta al diálogo y a disco.

Tests que los afectan:
- `core/tests/test_ui.py`: `test_global_configuration_editor_controls_save_and_cancel`,
  `test_pocketool_configuration_and_update_dialog`, `test_profile_configuration_editor_validation_and_save`,
  `test_export_import_profile_through_dialogs` (diálogo de gestión), `test_compact_sidebar_version_and_create_profile_dialog`
  (orden de botones y layout de Gestionar profile), `test_mito_shortcut_and_delete_selected_profile`.
- `core/tests/test_i18n.py`: `test_native_labels_are_escaped_as_data` (`localize_script`),
  `LanguageUITests.test_english_interface_dialogs_catalog_and_worker_messages` (textos ingleses de cada diálogo),
  `LanguageUITests.test_configuration_language_select_save_cancel_and_reopen`.
- `core/tests/test_mcp_ui.py`: `test_jobs_monitor_is_optional_and_separate` (`mcp.jobs.monitor` por la configuración).

## Responsabilidad

Editar `config.properties` (global) o `profiles/<name>.json` (variables de un profile) con un formulario de
filas, validar antes de escribir y devolver `True` solo si se guardó; gestionar el diálogo de perfil (crear,
configurar, exportar/importar, archivos, acceso directo, eliminar); dar la base visual común (`DialogHeader`,
`ModalScreen` con bindings traducidos) y los diálogos nativos de Windows con sus rótulos traducibles.

## Diálogo `ConfigurationDialog(engine, profile=None)` → `True` si se guardó, `False` si se canceló

### Composición
- Contenedor `Vertical#configuration-dialog`: ancho 95 %, máx. 136, altura 90 %, padding `1 2`, borde `#b5dc64`, fondo `#153429`.
  Superposición `#00000088`.
- `DialogHeader(título, 'configuration-close')`. Título: «CONFIGURACIÓN DEL PROFILE» si hay profile; si no, «CONFIGURACIÓN».
- `Static#configuration-context` (altura auto, margen inferior 1, markup off):
  - Profile: `'Profile: {p1} · Variables que sobrescriben las globales.'`.
  - Global: `'config.properties · Variables comunes a todos los profiles.'`.
- `VerticalScroll#configuration-rows` (altura 1fr, mínimo 3): una `SettingRow` por entrada de `settings.values`.
- `Horizontal#configuration-add-row` (altura 3, margen superior 1):
  - `Select#configuration-kind` (ancho 1fr, sin vacío, valor `env.`, deshabilitado en modo profile). Opciones:
    - `('Variable de entorno' → 'env.')`.
    - Solo global: cada clave de `PROPERTIES` como `(clave, clave)`, y cada prefijo de `PREFIXES` como `(prefijo + '<nombre>', prefijo)`.
  - `Input#configuration-name` (ancho 1fr). Placeholder «Nombre de la variable».
  - `Button#configuration-add` «Añadir» (ancho 10).
- `Static#configuration-error` (color `#ff7070`, margen superior 1, altura auto).
- `Button#configuration-save` «Guardar» (primary, ancho 100 %, margen superior 1).

### Inicialización
- `settings = Configuration(engine, profile)`.
- Solo global: rellena con valores por defecto las claves `mito.language`, `mcp.jobs.monitor` y `terminal.defaultShell`
  (`setdefault`). Por eso siempre aparecen y se escriben al guardar.

### Filas (`SettingRow(key, value, profile=False)`)
- `free = profile or key.startswith('env.')`. `display_key` = clave sin `env.` (en global) o la clave completa.
- Clave:
  - Libre (`free`): `Input(display_key)` con placeholder «Variable» y clase `setting-key`.
  - Fija: `Static(key)` (markup off) con clase `setting-key`.
- Valor:
  - Si la clave está en `PROPERTIES` con lista de valores (y no es libre): `Select` con las opciones. Etiqueta de
    `mito.language = system` → «Automático (Windows)»; el resto, el valor tal cual. Si el valor actual (en minúsculas)
    no está en la lista, se añade `('{p0} (valor no válido)', valor)` para que quede visible.
  - Si no: `Input(value)` con placeholder «Valor» y clase `setting-value`.
- `Button.setting-remove` «Eliminar» (variant error). Sin confirmación.
- `pair(profile)` → `(clave, valor)`:
  - Clave: si es libre, el texto del `Input` sin espacios; si es fija, la clave original.
  - En global, las claves libres reciben el prefijo `env.`.
  - Valor: `str(Select o Input .value)`.

### Acciones de `on_button_pressed` (asíncrono)
- Siempre limpia `#configuration-error`.
- `configuration-close` o `Esc` → `dismiss(False)`.
- `setting-remove` → quita la fila (`await parent.remove()`), sin confirmación ni escritura.
- `configuration-add`:
  1. `kind = Select#configuration-kind`; `name = Input#configuration-name` sin espacios.
  2. Si `kind` no está en `PROPERTIES` y `name` está vacío → error «Introduce el nombre de la propiedad o variable.».
  3. `key = name` en modo profile; si no, `kind` si está en `PROPERTIES`, o `kind + name`.
  4. Si alguna fila tiene la misma clave (comparación sin distinguir mayúsculas) → error «Esta propiedad ya está añadida.».
  5. Monta la nueva `SettingRow(key, PROPERTIES[key][1] o '')`, la hace visible con `scroll_visible` y limpia el nombre.
- `configuration-save`:
  1. `settings.save([fila.pair(profile) for fila in filas])`.
  2. Si lanza `YitoError`, `OSError` o `ValueError`: `capture('configuration.save')` y muestra el mensaje en `#configuration-error`. No cierra.
  3. Si no: `dismiss(True)`.
- `on_select_changed` para `#configuration-kind`: `#configuration-name` se deshabilita si el valor está en `PROPERTIES`;
  su placeholder es «Nombre de la variable» para `env.` y «Nombre» en los demás.

### Semántica de guardado (`Configuration.save`)
- Valida (`validated`) el conjunto completo. Después, dentro de `mutation(root)`, comprueba que el archivo no cambió
  desde que se abrió el diálogo; si cambió, error «La configuración cambió fuera del diálogo. Ciérralo y vuelve a abrirlo.».
- Profile: `data['env'] = valores` y `save_profile(profile, data)`. Los valores no se recortan.
- Global: ver «Formato de disco».

## Diálogo `ManageProfileDialog(profile, can_delete)` → `(acción, nombre|None)` o `None`

- `VerticalScroll#manage-profile-dialog`: ancho 62, altura auto, máx 90 %, borde `#b5dc64`.
- `DialogHeader('GESTIONAR PROFILE', 'manage-profile-close')`.
- `Static#managed-profile-name` centrado: profile actual o «Ningún profile seleccionado».
- `Input#new-profile` placeholder «Nombre del nuevo profile».
- `Button#create` «Crear profile» (primary).
- `Horizontal#profile-transfer-actions` (altura 4): `Button#export-profile` «Exportar» (deshabilitado sin profile), `Button#import-profile` «Importar».
- `Button#mito-shortcut` «Acceso directo» (deshabilitado sin profile).
- `Button#profile-configuration` «Configuración» (deshabilitado sin profile).
- `Button#profile-files` «Archivos de configuración» (deshabilitado sin profile).
- `Button#delete-profile` «Eliminar profile» (variant error, deshabilitado si `can_delete` es falso; `can_delete = profile and len(profiles) > 1`).

Resultado:
- `manage-profile-close` o `Esc` → `None`.
- `create`: si el nombre (sin espacios) está vacío, enfoca `#new-profile` y no cierra (sin mensaje). Si no, devuelve `('create', nombre)`.
- Cualquier otro botón devuelve `(id, None)`. El llamador (`ui.py`) despacha por `id`.

## Diálogo `DeleteProfileDialog(profile)` → `True` si se confirma

- `VerticalScroll#delete-profile-dialog`: ancho 66, altura auto, borde `#e27878`.
- `Static` «¿Eliminar el profile {p1}?» (`p1` = profile; markup off).
- `Static` «Se eliminará su configuración. Se conservan componentes, datos persistentes, home y workspace. Los accesos de este profile dejarán de funcionar.».
- `Button#delete-profile-cancel` «Cancelar» (con foco al montar).
- `Button#delete-profile-confirm` «Eliminar profile» (variant error).
- `Esc` → `False`.

## `DialogHeader(title, close_id)` (`Horizontal`)

- Altura 3, margen inferior 1, alineación vertical media.
- Hijos, en orden:
  1. `Static.header-spacer` (ancho 3), para equilibrar el título.
  2. `Static.dialog-title` (ancho 1fr, altura 3, texto centrado, negrita, color `#b5dc64`).
  3. `Button.dialog-close` «×» con `id = close_id` y `tooltip = 'Cerrar'`: ancho 3, sin borde, fondo transparente,
     color `#ff7070`, negrita; al foco o pasar el ratón el fondo es `#563330`.
- Lo usan todos los diálogos con cabecera (manifiesto, configuración, perfil, componente info, versiones, pocketools…).

## `localized_ui.py`

- `translate_bindings(node, translate=tr)`: para cada lista de bindings de `node._bindings.key_to_bindings`,
  sustituye la descripción de cada binding por `translate(descripción)`. Mantiene la clave y la acción.
- `class ModalScreen(TextualModalScreen)`: en `__init__` llama a `translate_bindings(self)`. Todos los diálogos
  de MITO heredan de esta clase (no de `textual.screen.ModalScreen`, salvo `open_with_ui`, `protocols_ui`).
- Las descripciones de los bindings son claves: «Cerrar», «Cancelar», «Salir», «Buscar componentes», «Inicio».

## `native_ui.py`

`localize_script(script, *labels, translate=tr)`:
- Para cada rótulo, busca el literal de PowerShell `'<rótulo>'` (con las comillas simples) y lo reemplaza por
  `'<traducción>'`. Las comillas simples dentro de la traducción se duplican (`'` → `''`).
- No hay escape de `$`; la traducción se trata como dato dentro de la cadena literal.
- Solo sustituye literales completos; los valores del usuario (rutas, credenciales) no pasan por este mecanismo.
- Ejemplo de test: `localize_script("$title = 'Open'", 'Open', translate=lambda _: "User's $folder")`
  → `"$title = 'User''s $folder'"`.

Rótulos que pasan por `localize_script` (y se ejecutan con `powershell.exe -NoLogo -NoProfile -STA -Command`):

| Módulo (llamador) | Rótulos |
|---|---|
| `host_apps.py` (F8, `choose_file`) | `MITO - Agregar aplicación del host`; `Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)\|*.exe;*.bat;*.cmd;*.ps1` |
| `host_data.py` (F8, `choose_folder`) | `MITO - Selecciona la carpeta del host para enlazar` |
| `network.py` (F9, `prompt_credentials`) | `MITO - Autenticación de red`; `Usuario y contraseña del portal. Cancelar continúa sin conexión.`; `No se pudo verificar Internet. Reintenta o cancela para continuar.` |
| `profile_files_ui.py` (F11) | `MITO - Archivo de configuración` (archivo) o `MITO - Carpeta de configuración` (carpeta) |
| `profile_transfer_ui.py` (F11, `choose_export_file`) | `MITO - Importar profile`; `Profiles MITO (*.mito.json;*.json;*.zip)\|*.mito.json;*.json;*.zip\|Todos los archivos (*.*)\|*.*` |

El texto de los filtros de archivo es una clave; el separador `|` (escapado como `\|` en esta tabla) es parte de la cadena.

## Traducciones de referencia (inglés, de `test_i18n.py`)

Ejemplos verificados por `LanguageUITests` con `mito.language=english`. Sirven para comprobar la
búsqueda de bundle y los placeholders:

| Clave (español) | Inglés verificado en el test |
|---|---|
| Configuración (botón de la barra) | Settings |
| Internet accesible | Internet available |
| ningún profile | no profile |
| Salir (binding de Ctrl+Q) | Quit |
| Cancelar (binding de Esc) | Cancel |
| Cerrar (binding de Esc) | Close |
| Generar (ShortcutDialog, botón confirmar) | Create |
| Descargar e instalar (UnverifiedDownloadsDialog) | Download and install |
| Eliminar (RemoveComponentDialog, botón confirmar) | Remove |
| Crear profile (ManageProfileDialog) | Create profile |
| Eliminar profile (DeleteProfileDialog, botón confirmar) | Remove profile |
| Exportar (ManageProfileDialog) | Export |
| Abrir (HostInfoDialog, carpeta) | Open |
| Guardar (ConfigurationDialog) | Save |
| Profile {p1} creado (`p1 = two`) | Profile two created |
| Paso omitido: MITO no ejecuta {p1}. (`p1 = post_install`) | Skipped step: MITO does not run post_install. |

Las claves de «Instalar y crear profile» (ImportReviewDialog), «Enlazar» (HostDataConfirm) y «Conectar»
(ProxyDialog) se verifican en sus propios módulos; no se fijan aquí. Las notas de un manifiesto («No se traduce
esta nota.») se muestran sin cambios porque son datos del catálogo.

## Formato de disco

### `config.properties` (global)

Ruta: `<raíz>/config.properties`. Codificación de lectura `utf-8-sig`; escritura `utf-8`, fin de línea `\n`.
Escritura atómica: archivo temporal en la misma carpeta y `os.replace`, dentro de `mutation(root)`.

Lectura (`parse`):
- Una línea activa tiene un `=`; se corta en el **primer** `=`. Clave y valor se recortan (espacios).
- Se ignoran las líneas vacías, las que empiezan por `#` o `!` y las que no tienen `=`.

Guardado (`Configuration.save` para global):
- Recorre las líneas originales. Las que no son claves (vacías, comentarios, sin `=`) se conservan en su sitio.
- Una línea cuya clave está en el conjunto nuevo se reescribe como `clave=valor` (en su posición).
- Las claves **que no están en el conjunto nuevo se eliminan del archivo** (así «Eliminar» quita la clave).
- Las claves nuevas se añaden al final, en el orden del conjunto.

Ejemplo del test (`test_global_configuration_editor_controls_save_and_cancel`). Antes:

```properties
# Keep this comment
env.CUSTOM=old
```

Tras editar (CUSTOM eliminado, NEW añadido, selección de idioma/terminal/proxy) y guardar, el archivo contiene el
comentario original y las claves nuevas, por ejemplo: `mito.language=system`, `mcp.jobs.monitor=false`,
`terminal.defaultShell=powershell`, `enable_proxy_authentication=true`, `proxy_authentication_username_field=login`,
`env.NEW=a=b`. `CUSTOM` ya no aparece. Un valor que contiene `=` se guarda tal cual tras la primera clave.

Cancelar (`Esc`) no escribe nada.

### `profiles/<nombre>.json` (variables de un profile)

Ruta: `<raíz>/profiles/<identifier(profile)>.json`. Se guarda con `engine.save_profile`; el diálogo solo cambia
`env`:

```json
{"workspace": "one", "apps": ["main/tool"], "env": {"CUSTOM": "value"}}
```

Las claves de `env` son los nombres de variable tal como los introduce el usuario (sin recortar valores).

## Propiedades globales (`PROPERTIES`) y prefijos (`PREFIXES`)

| Clave | Valores permitidos | Por defecto | Fase | Notas |
|---|---|---|---|---|
| `terminal.defaultShell` | `cmd`, `powershell`, `mito` | `cmd` | F6 | Primera entrada del menú de Windows Terminal |
| `mito.language` | `system` y 24 idiomas (`LANGUAGE_CHOICES`) | `system` | F6 | `system` = idioma de Windows |
| `mcp.jobs.monitor` | `true`, `false` | `false` | F12 | Muestra «MCP · Jobs» |
| `mito.update_url` | libre (URL validada, obligatoria) | `https://api.github.com/repos/danielgube/mito/releases/latest` | F11 | |
| `enable_proxy_authentication` | `true`, `false` | `false` | F9 | |
| `proxy_authentication_url` | libre (URL validada si no vacía) | `''` | F9 | |
| `proxy_authentication_username_field` | libre | `username` | F9 | |
| `proxy_authentication_password_field` | libre | `password` | F9 | |
| `proxy_authentication_require_https` | `true`, `false` | `true` | F9 | |
| `http_proxy` | libre (URL validada si no vacía) | `''` | F9 | |
| `https_proxy` | libre (URL validada si no vacía) | `''` | F9 | |
| `no_proxy` | libre | `''` | F9 | |
| `network.check_url` | libre (URL validada; obligatoria) | `https://www.gstatic.com/generate_204` | F9 | |
| `network.check_status` | código HTTP 100-599 | `204` | F9 | Solo dígitos |
| `network.timeout_seconds` | número > 0 y ≤ 60 | `4` | F9 | |
| `network.poll_seconds` | libre (sin validación en `validated`) | `20` | F9 | Ver Dudas |
| `shortcut.badgeText` | 1-4 letras o números (`badge_text`) | `MITO` | F8 | |
| `shortcut.badgeColor` | `negro`, `rojo`, `azul`, `naranja`, `rosa`, `verde`, `marron`, `morado` | `naranja` | F8 | |
| `posthog.enabled` | `true`, `false` | `false` | ver Dudas | Telemetría: revisar |
| `posthog.host` | libre (URL validada si no vacía) | `''` | ver Dudas | Telemetría: revisar |
| `posthog.api_key` | libre | `''` | ver Dudas | Telemetría: revisar |

Prefijos (se añaden como fila nueva con el nombre que escribe el usuario):

| Prefijo | Reglas | Fase |
|---|---|---|
| `env.` | nombre de variable válido (`engine.validate_env`) | F6 |
| `bucket.` | nombre con `identifier`; valor (fuente de catálogo) no vacío | F6 |
| `proxy_authentication_form_field.` | nombre no vacío (si no, «Falta el nombre del campo del formulario») | F9 |
| `pocketool.` | nombre con `identifier`, sin repetir; origen validado con `pocketools.validate_source`; si el origen es `pocketools/<x>` debe coincidir con el nombre | F10 |

### Validación (`configuration.validated`)

Mensajes exactos (aparecen en `#configuration-error`):

```
Introduce una clave válida
{p0}: el valor debe ocupar una línea
Clave repetida: {p1}
{p0}: elige uno de los valores permitidos
{p0}: introduce un número mayor que 0 y hasta 60
{p0}: introduce un código HTTP entre 100 y 599
{p0}: introduce una fuente de catálogo
Falta el nombre del campo del formulario
Pocketool repetida: {p1}
La carpeta local debe coincidir con el nombre del paquete.
La configuración cambió fuera del diálogo. Ciérralo y vuelve a abrirlo.
```

Reglas comunes: la clave no puede contener `=`, `\r`, `\n` ni `\0`, ni empezar por `#` o `!`; el valor no puede
contener `\r`, `\n` ni `\0`. Para `env.` y para el perfil se valida con `engine.validate_env` (los errores del motor,
p. ej. variables reservadas como `PATH`, se muestran tal cual: la prueba espera la palabra «reservada»).
En global los valores se recortan; en profile no.

## Casos límite y errores

- Un valor almacenado que no está en la lista permitida aparece como `'{p0} (valor no válido)'`. Si se guarda sin
  cambiarlo, la validación lo rechaza con «elige uno de los valores permitidos».
- Eliminar una fila fija (p. ej. `terminal.defaultShell`) la quita del conjunto guardado: `Configuration.save`
  elimina la clave del archivo. Al volver a abrir el diálogo reaparece con su valor por defecto (`setdefault`).
- Cambiar `mito.language` en el diálogo global: se guarda y el gestor pide reabrirse (ver `ui.md`, «`open_configuration`»).
- Dos guardados seguidos desde dos gestores: el segundo recibe «La configuración cambió fuera del diálogo…».
- Un `ModalScreen` cerrado con `Esc` no escribe. `Esc` en `ManageProfileDialog` devuelve `None`.
- Sin profile válido, `ConfigurationDialog(engine, profile)` puede lanzar `YitoError`/`OSError`/`ValueError` al
  leer el archivo; lo captura `open_configuration` y lo muestra con notify.

## Dependencias

- `i18n` (`tr`), `localized_ui.ModalScreen`, `dialog_header.DialogHeader`, `configuration` (`Configuration`,
  `PROPERTIES`, `PREFIXES`), `storage` (`YitoError`, `mutation`, `identifier`), `error_tracking` (`capture`),
  `engine` (`validate_env`, `save_profile`, `profile`), `network` (`validate_url`), `shortcuts` (`badge_text`),
  `pocketools` (`validate_source`), `i18n` (`DEFAULT_SETTING`, `LANGUAGE_CHOICES`).
- Usados por: `ui.py` (`open_configuration`, `manage_profile_result`, botones de la barra lateral), `mcp_ui.py`
  (`mcp.jobs.monitor`), y los diálogos `pocketools_ui.py`, `network_ui.py`, `host_apps_ui.py`, `profile_transfer_ui.py`,
  `profile_files_ui.py` y `self_update_ui.py`, que heredan de `localized_ui.ModalScreen`. `open_with_ui.py` y
  `protocols_ui.py` heredan directamente de `textual.screen.ModalScreen` (sin traducción de bindings).

## Notas para Rust

- Crate `mito-tui`. Las filas de configuración son un vector de estructuras `{ clave, valor, libre, opciones }`;
  el `Select` es una lista con selección; `Input` un campo de texto con cursor. Foco con Tab; `Esc` cancela.
- `DialogHeader` es un bloque de tres celdas: relleno, título centrado y botón `×` (tecla Enter o clic).
- `ModalScreen` es un diálogo apilado; los bindings del diálogo se traducen en su constructor con el mismo catálogo
  de `mito-i18n` que el resto de la interfaz.
- `localize_script` es una función pura: sustituye literales PowerShell entre comillas simples duplicando `'`.
- La escritura de `config.properties` debe ser atómica (temporal + renombrado) y bajo el mismo bloqueo `mutation`.
- Mantener las dos reglas de `Configuration.save` (conservar líneas no clave; eliminar claves no presentes).

## Dudas y ambigüedades

1. **Telemetría.** `posthog.enabled`, `posthog.host` y `posthog.api_key` están en `PROPERTIES` y se pueden editar.
   Las reglas de producto dicen «Sin telemetría remota». Confirmar si estas claves se omiten en el port.
2. **`network.poll_seconds` sin validación.** `validated` no la comprueba, pero `start_network_poll` la convierte a
   número (valor no numérico → 20). Confirmar si el port debe validarla al guardar (la spec de USAGE dice «mínimo 5»).
3. **Etiquetas no traducidas.** Las opciones de `mito.language` muestran los nombres de idioma tal cual (`español`,
   `english`, …). Solo «Automático (Windows)» se traduce. Confirmar si es intencionado.
4. **Eliminar sin confirmación.** `setting-remove` quita la fila sin pedir confirmación; la eliminación se aplica al guardar.
   Confirmar que es el comportamiento deseado para claves de sistema como `terminal.defaultShell`.
5. **Crear perfil con nombre vacío.** `ManageProfileDialog` solo enfoca el campo, sin mensaje de error.
   Confirmar si el port debe mostrar un aviso.
6. **Clave duplicada.** La comparación de duplicados del diálogo usa `casefold`. Confirmar si el port debe comparar
   sin distinguir mayúsculas también al validar en `validate_env` (el original solo lo hace en el diálogo).
7. **Orden de las claves al guardar.** Las claves nuevas se añaden al final en el orden de las filas del diálogo.
   Confirmar que el port mantiene el mismo orden de inserción para que el diff del archivo coincida.
