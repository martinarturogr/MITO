# ui.py → mito-tui::ui

Origen: `core/app/mito/ui.py` (1952 líneas), `core/tests/test_ui.py` (898 líneas, 26 tests).
Los diálogos de configuración, perfiles, cabecera, i18n de bindings y diálogos nativos están en
`ui_dialogs.md` (`configuration_ui.py`, `profile_management_ui.py`, `dialog_header.py`,
`localized_ui.py`, `native_ui.py`). Las pantallas de red, MCP, pocketools, host, transferencia de
perfiles, protocolos y «Abrir con» viven en sus propios módulos; aquí solo se documenta cómo se
invocan desde `YitoApp`.

Convención: «Fase» indica la fase del port en la que se implementa (ver «Mapa de fases»).
Los textos entre comillas son claves de i18n exactas (texto español de la plantilla). Los `{pN}` son
placeholders de `tr(clave, pN=valor)`; el valor no se traduce ni forma parte de la clave.

## Responsabilidad

`YitoApp` es la interfaz Textual del gestor MITO (ventana «MITO · {versión}»). Muestra el inventario
de componentes instalados (Inicio), el catálogo de buckets (Buscar e instalar), los profiles, la
configuración, el log de sesión y las operaciones largas en workers de hilo (instalar, quitar,
actualizar, activar/desactivar, versiones, abrir aplicación, abrir terminal). Toda mutación pasa por
el motor (`Engine`) dentro de un worker con `Operation` como canal de progreso y cancelación.

## Descripción de usuario

Resumen de `MANUAL.md` («La interfaz») y `spec/USAGE.md`, para verificar el comportamiento visible.

- **Barra lateral.** Selector de profile y «Gestionar profile»; «Inicio · Instalados»; «Buscar
  actualizaciones»; «Actualizar MITO» (solo si hay release posterior); «Abrir Home» (abre la carpeta
  `home` en el Explorador); «Configuración»; «Pocketools»; «MCP - disabled/enabled»; «MCP · Jobs»
  (solo con `mcp.jobs.monitor=true`); botón de estado de Internet.
- **Inicio.** Una fila por componente instalado con su casilla (activo en el profile seleccionado).
  Marcar o desmarcar solo cambia la selección del profile: no descarga ni borra. Con el profile activo
  aparecen «Abrir», «Acc. directo», «Extensiones ✓/✗», «Protocolos» (si la aplicación es lanzable).
  «Info», «Actualizar» (si hay versión compatible más nueva), «Eliminar».
- **Catálogo.** Búsqueda por palabras con ranking (identificador, luego descripción; coincidencias
  parciales al final), filtros por bucket, detalle de la aplicación seleccionada, «Instalar»,
  «Versiones…», «Manifiesto», «Actualizar catálogos», «Volver a Inicio».
- **Log.** Panel inferior con los mensajes de la sesión; los errores en rojo y negrita. «Copiar log»
  envía el texto completo al portapapeles, también durante una operación.
- **Operaciones.** Al tardar más de 2 s aparece un diálogo de progreso (bytes, velocidad, archivos,
  tiempo) con «Cancelar instalación» mientras la cancelación sea posible (hasta la publicación). Si el
  motor avanza un paso sin progreso medible, aparece un aviso «PREPARANDO ARCHIVOS» centrado.
- **Teclas.** `Ctrl+F` abre el catálogo y enfoca la búsqueda; `Esc` vuelve a Inicio o cierra un
  diálogo cancelable; `Ctrl+Q` sale (no se sale durante una operación).
- **Profiles.** «Gestionar profile» permite crear, configurar variables del profile, exportar/importar,
  archivos de configuración, acceso directo a MITO y eliminar (nunca el último profile).
- **Terminal.** Las pestañas de Windows Terminal se abren desde el menú; la interfaz solo prepara su
  `settings.json` al cambiar de profile o guardar la configuración.
- **Internet.** El botón de estado se comprueba en segundo plano y en rojo sin red; al pulsarlo diagnostica
  la red y, si hace falta, propone guardar el proxy o el portal y pide credenciales de portal solo para la sesión.
- **Idioma.** Automático (Windows) o uno de los 24 idiomas de la UE; se aplica al volver a abrir el gestor.

## Estructura de la pantalla principal (`compose`)

Orden de widgets. `Fase` es la fase de la fila; las sub-filas indican la fase de la función concreta.

```
Header                                  TITLE = 'MITO · ' + __version__ ; SUB_TITLE = 'My Integrated Terminal Orchestrator'
Horizontal#body (height 1fr)
  VerticalScroll#sidebar (width 34, padding 1 2, bg #153429)
    Label.title 'PROFILES'                                            F6
    Select#profile (prompt 'Selecciona un profile', valores = engine.profiles())   F6
    Button#manage-profile 'Gestionar profile'                         F6
    Label#actions-title.title 'ACCIONES' (margin-top 2)               F6
    Button#home-button 'Inicio · Instalados' variant success          F6
    Button#check-updates 'Buscar actualizaciones'                     F6 (componentes) / F11 (MITO)
    Button#self-update 'Actualizar MITO' variant success, oculto      F11
    Button#open-home 'Abrir Home'                                     F6
    Button#configuration 'Configuración'                              F6
    Button#pocketools 'Pocketools'                                    F10
    Button#mcp-button 'MCP - disabled' (literal, no traducido)        F12
    Button#mcp-jobs-button 'MCP · Jobs' (literal), oculto si mcp.jobs.monitor != 'true'   F12
    Button#network-status 'Comprobando Internet…' variant default     F9
    Static#busy-indicator (height 3, color #FFD166, negrita)          F6
ContentSwitcher#pages (width 1fr, initial 'home-page')
  Vertical#home-page.page                                             F6
    Label.title 'MIS COMPONENTES'
    Static#installed-summary.summary
    VerticalScroll#installed-list (height 1fr)   <- filas de componentes (ver «Inicio»)
    Horizontal#home-actions (height 3, margin-top 1)
      Button#browse 'Buscar e instalar nuevos componentes' variant primary
      Button#host-add 'Agregar aplicación del host'                    F8
      Button#bulk-update 'Actualizar todos ({p1})' variant success, oculto salvo >1 pendientes   F6
  Vertical#catalog-page.page                                          F6
    Label.title 'BUSCAR E INSTALAR · CATÁLOGOS SCOOP'
    Input#search placeholder 'Buscar aplicación, runtime o herramienta…'
    HorizontalScroll#bucket-filters (height auto)
      Checkbox#bucket-<nombre> (name=<nombre>, value = nombre no deshabilitado), una por bucket
      (nombres = ordenados de la unión de engine.catalog.sources() y de core/buckets/*.json)
    Static#summary.summary (height auto, min 3)
    DataTable#apps (cursor_type 'row', zebra_stripes, height 1fr)
    VerticalScroll#details-scroll (height 9)
      Static#details (markup False) 'Selecciona una aplicación.'
    Horizontal#catalog-actions (height 4)
      Button#install 'Instalar' variant success, disabled
      Button#versions 'Versiones…', disabled
      Button#manifest 'Manifiesto', disabled
      Button#refresh 'Actualizar catálogos'
      Button#back 'Volver a Inicio'
Horizontal#log-panel (height 8, border-top solid #264858)
  RichLog#log (wrap, markup False)
  Button#copy-log 'Copiar log' (width 14)
Footer
```

`on_mount` (orden): asigna columnas de `DataTable#apps` = `'Componente'`, `'Versión'`, `'Disponibilidad'`,
`'Licencia'`; selecciona el profile inicial (si `initial_profile` existe, si no el primero); refresca el
terminal por defecto (`terminal.prepare`); `refresh_home()`; escribe los avisos de `relocation.pop_notice`;
`terminal_identity.apply_safely`; `check_network()`; `start_network_poll()` (F9);
`check_component_updates()` (F6); `check_mito_update()` (F11).

## Estilo (TCSS resumido)

Colores fijos (no hay tokens); todos son literales de `YitoApp.CSS`:

| Selector | Propiedades |
|---|---|
| `Screen` | fondo `#102820`, texto `#e7efdf` |
| `Header` | fondo `#215139` |
| `#sidebar` | ancho 34, padding `1 2`, fondo `#153429` |
| `#sidebar Button` | ancho 100 %, margin-top 1 |
| `ContentSwitcher` | ancho 1fr |
| `.page` | padding `1 2` |
| `.title` | color `#b5dc64`, negrita, margin-bottom 1 |
| `#actions-title` | margin-top 2 |
| `Input, Select` | margin-bottom 1 |
| `DataTable` | altura 1fr |
| `#details-scroll` | altura 9 |
| `#details` | altura auto, padding 1, fondo `#1b3b2e` |
| `#bucket-filters` | altura auto, min 1, margin-bottom 1 |
| `#bucket-filters Checkbox` | ancho auto, altura 1, padding `0 1`, margin-right 1, sin borde |
| `Checkbox > .toggle--button` | color `#8A4B50` (apagado, rojo) |
| `Checkbox.-on > .toggle--button` | color `#69FF64` (encendido, verde) |
| `#catalog-actions` | altura 4; botones ancho 1fr, margin-right 1 |
| `#log-panel` | altura 8, borde superior sólido `#264858` |
| `#log` | 1fr x 1fr |
| `#copy-log` | ancho 14, min 14, margin `1 1` |
| `.summary` | altura 3, color `#95a9bd`; `#summary` altura auto min 3 |
| `#installed-list` | altura 1fr |
| `.component` | altura 1, margin-bottom 1, padding `0 1`, alineación vertical media |
| `.component-row-dark` / `-light` | fondo `#1b3b2e` / `#29513d` (alternos, según índice de la fila) |
| `.component Checkbox` | ancho 1fr, altura 1, alineación izquierda, sin borde, fondo transparente |
| `.component Button` | ancho 10, min 10, altura 1, padding `0 1`, sin borde |
| `.component-info` / `.component-update` | ancho 10 / 12, margin-left 1 |
| `.component-remove` | margin-left 1 |
| `.app-shortcut` | ancho 16, margin-left 1 |
| `.open-with-button` | ancho 20, margin-left 1 |
| `.protocol-button` | ancho 14, margin-left 1 |
| `.host-data-button` | ancho 12, margin-left 1 |
| `#home-actions` | altura 3, margin-top 1; botones 1fr |
| `#browse` / `#bulk-update` | margin-right 1 / margin-left 1 |
| `#busy-indicator` | altura 3, padding 1, color `#FFD166`, negrita |
| `.host-component` | overflow-x auto; `.host-component.component-row-dark` fondo `#593300`; `-light` fondo `#8f5000` |
| `.host-app-name` | ancho 1fr, altura 1, padding-left 5, alineación izquierda, color `#FFF` |

Diálogos (todos en `ModalScreen`, fondo de superposición `$background 70%` o `#00000088`): panel
`#153429`, borde redondeado. Color de borde: `#b5dc64` (por defecto), `#e27878` (eliminar, integridad),
`#e5ba65` (descargas sin hash, progreso, aviso de paso largo). Ancho y alto por diálogo en su sección.

## BINDINGS

Aplicación (`YitoApp.BINDINGS`; descripciones traducidas en `__init__` vía `translate_bindings`):

| Tecla | Acción | Descripción (clave) | Fase |
|---|---|---|---|
| `ctrl+q` | `quit` | «Salir» | F6 |
| `ctrl+f` | `search` | «Buscar componentes» | F6 |
| `escape` | `home` | «Inicio» | F6 |

Diálogos (descripción de cada BINDING; se traducen igual):

| Diálogo | Tecla | Acción | Descripción |
|---|---|---|---|
| ManifestDialog | escape | close | «Cerrar» |
| InstallVersionDialog | escape | cancel | «Cancelar» |
| VersionsDialog | escape | cancel | «Cancelar» |
| OpenApplicationDialog | escape | cancel | «Cancelar» |
| ShortcutDialog | escape | cancel | «Cancelar» |
| RemoveComponentDialog | escape | cancel | «Cancelar» |
| ComponentInfoDialog | escape | close | «Cerrar» |
| UnverifiedDownloadsDialog | escape | cancel | «Cancelar» |
| DownloadIntegrityDialog | escape | close | «Cerrar» |
| LongStepDialog | (ninguna) | | |
| OperationDialog | (ninguna; `Esc` cae al binding de la aplicación «Inicio») | | |

## Estado de `YitoApp`

Atributos que el port debe conservar (nombres del original en inglés):

- `engine`, `initial_profile`, `translator`.
- `entries`: lista `(key, manifest, status)` del catálogo; vacía = recargar.
- `catalog_installed`: conjunto de claves instaladas (los IDs de instalación son la clave `bucket/app`).
- `disabled_buckets`: leído de `core/state/catalog-filters.json` (lista de nombres).
- `selected`: clave `bucket/app` resaltada o `None`.
- `busy`, `busy_sequence`, `rebuilding`.
- `inventory`: `installation -> lock`; `row_ids`: índice de fila (texto) -> installation; `checkbox_states`.
- `host_inventory`: identidad -> ruta; `pending_updates`: instalaciones con actualización `Compatible`.
- `mito_update`: información de actualización de MITO o `None`.
- `log_messages`: lista de texto plano para «Copiar log».
- `current_operation`, `operation_dialog`, `long_step_dialog`, `long_step_sequence`.
- `mcp`, `mcp_timer`, `mcp_quit`, `mcp_confirmations`, `mcp_confirming`, `mcp_dirty` (F12).
- `network_timer` (F9, solo si el intervalo es > 0).

## Manejador de botones (`on_button_pressed`)

Orden de evaluación. Cada `return` es explícito; las filas siguen el orden del código.

1. **Acciones anteriores a la guarda de ocupado:** `manifest`, `mcp-button`, `mcp-jobs-button`, `copy-log`,
   `network-status`, `open-home`, `browse`, `home-button`/`back` se atienden aunque haya una operación en curso.
2. **Guarda de ocupado:** si `busy` o `rebuilding`, se ignora cualquier otro botón sin mensaje.
3. **Resto:** `check-updates`, `self-update`, `host-add`, `host-*`, `configuration`, `pocketools`,
   `manage-profile`, `bulk-update`; después la guarda de profile; después `info-`, `remove-`, `expose-`,
   `openwith-`, `protocol-`, `update-`, `install`/`versions`, `open-`, `link-`; y por último la acción genérica.

| Id (patrón) | Acción | Función / diálogo | Mensajes (log, notify) | Fase |
|---|---|---|---|---|
| `manifest` | Abre el manifiesto del seleccionado | `ManifestDialog(selected, manifest)`; solo si existe la entrada | — | F6 |
| `mcp-button` | Abre el control MCP | `mcp_ui.MCPDialog()` | — | F12 |
| `mcp-jobs-button` | Abre el monitor de trabajos | `mcp_ui.MCPJobsDialog()` solo si `mcp_monitor_enabled()` | — | F12 |
| `copy-log` | Copia el log | `copy_log('\n'.join(log_messages))` si hay mensajes | notify «El log está vacío» (sin severidad) | F6 |
| `network-status` | Comprueba Internet con diagnóstico | `check_network(prompt=True)` | ver F9 | F9 |
| `open-home` | Abre `root/home` | `persistence.open_location(root/'home')` | error: notify `str(exc)` (severidad error) + `capture('home.open-location')` | F6 |
| `browse` | Pasa al catálogo | `action_search()` | — | F6 |
| `home-button`, `back` | Vuelve a Inicio | `action_home()`: página `home-page`; si no `busy`, `check_mito_update()` | — | F6 (+F11) |
| `check-updates` | Busca actualizaciones de componentes y de MITO | `begin('check-updates', profile, None)` (no exige profile) | log «Búsqueda de actualizaciones finalizada.» | F6 (componentes) / F11 (MITO) |
| `self-update` | Confirma la actualización de MITO | si `mito_update`: `SelfUpdateDialog(mito_update)` → `confirm_mito_update(confirmed)` | — | F11 |
| `host-add` | Agrega aplicación del host (no exige profile) | `begin('host-add', profile, None)` | ver operación `host-add` | F8 |
| `host-open-<id>` | Abre app del host | exige profile (notify «Selecciona un profile» warning); `OpenApplicationDialog(path.stem)` → `begin('host-open', profile, id, coupled=result[0], arguments=result[1])` | — | F8 |
| `host-link-<id>` | Acceso directo del host | exige profile; `ShortcutDialog(application=True, engine)` → `begin('host-link', profile, id, badge, coupled, arguments, color)` | — | F8 |
| `host-info-<id>` | Info del host | `HostInfoDialog(path)` (resuelve con `host_apps.target`) | error: notify | F8 |
| `host-remove-<id>` | Quita la referencia del host | `RemoveHostDialog(path)` → `begin('host-remove', profile, id)` si confirmado | — | F8 |
| `host-cache-<id>`, `host-data-<id>` | Enlaza/desenlaza Cache o Data | `begin('host-cache'/'host-data', profile, id)` | — | F8 |
| `configuration` | Configuración global | `open_configuration()` | — | F6 |
| `pocketools` | Pocketools | `PocketoolsDialog(engine, profile)` | error: notify `str(exc)` + `capture('pocketools.open')` | F10 |
| `manage-profile` | Gestionar profile | `ManageProfileDialog(profile, bool(profile) and len(profiles) > 1)` → `manage_profile_result(profile, result)` | — | F6 |
| `bulk-update` | Actualizar todos | si `len(pending_updates) > 1`: `begin('update-all', profile, list(pending_updates))` | — | F6 |
| `info-<n>` | Info del componente | `ComponentInfoDialog(root, inventory[id], profile)`; resultado `'release-version'` → `begin('release-version', profile, id)` | — | F6 |
| `remove-<n>` | Eliminar componente | `RemoveComponentDialog(lock, users(engine, id))`; confirmado → `begin('remove-component', profile, id)` | — | F6 |
| `expose-<n>` | Enlace «Host · ✓/✗» del componente | `begin('host-expose', profile, id)` (sin diálogo previo; no exige profile) | ver operación `host-expose` | F8 |
| `openwith-<n>` | «Extensiones» | `OpenWithDialog(key, application_entries(lock), current)` → `begin('open-with', profile, id, arguments=result)` | — | F8 |
| `protocol-<n>` | «Protocolos» | `protocols.unregister(root, all, write_log)`; `rows = protocols.rows(engine, profile, id)`; `ProtocolDialog(key, entries, rows, Session)` | error: notify `str(exc)` + `capture('protocols.open')` | F8 |
| `update-<n>` | Actualizar componente | `begin('update-component', profile, id)` (botón deshabilitado si estado distinto de `Compatible`) | — | F6 |
| `open-<n>` | Abrir aplicación | `OpenApplicationDialog(key, application_entries(lock))` → `begin('launch', profile, id, shortcut=result[2], coupled=result[0], arguments=result[1])` | — | F6 (independiente) / F8 (acoplado) |
| `link-<n>` | Acceso directo de aplicación | `ShortcutDialog(application=True, entries, engine)` → `begin('application-shortcut', profile, id, badge, coupled, arguments, shortcut, color)` | — | F8 |
| `install` | Instala el seleccionado | si no hay selección o está deshabilitado: nada; si no `begin('install', profile, selected)` | — | F6 |
| `versions` | Elige otra versión | igual que `install` con `begin('versions', profile, selected)` | — | F6 |
| `refresh` | Actualiza catálogos | `begin('refresh', profile, selected)` (no exige profile) | — | F6 |
| Cualquier otro | Acción genérica | `begin(action, profile, selected)` | — | F6 |

Guarda de profile: todas las acciones salvo `create`, `refresh`, `install`, `versions`, `remove-*`,
`update-*`, `info-*`, `expose-*` (y las ya atendidas arriba) exigen profile; si falta, notify
«Selecciona un profile» (severidad warning) y retorno.

`manage_profile_result(profile, result)` (resultado de `ManageProfileDialog`; `None` = cerrar):

| `result[0]` | Acción | Fase |
|---|---|---|
| `create` | `begin('create', profile, name)` | F6 |
| `profile-configuration` | `open_configuration(profile)` | F6 |
| `profile-files` | `open_profile_files(profile)` → `ProfileFilesDialog`; guardado → notify «Archivos del profile guardados.» | F11 |
| `export-profile` / `import-profile` | `TransferPathDialog(root, profile or None, archive)` → `begin(action, profile, (path, name))` si hay selección. `archive` = `bool(engine.profile(profile).get('files'))` en exportación | F11 |
| `mito-shortcut` | `ShortcutDialog(engine)` → `begin('mito-shortcut', profile, None, badge, color)` | F8 |
| `delete-profile` | `DeleteProfileDialog(profile)` → `begin('delete-profile', profile, None)` si confirmado | F6 |

`open_configuration(profile=None)`: `ConfigurationDialog(engine, profile)` (un error de apertura →
notify `str(exc)`). Al guardar (`True`): notify «Configuración guardada. Abre nuevas pestañas o
aplicaciones para usar el nuevo entorno.». Si es global: `refresh_terminal_default()`; si el idioma
cambió respecto al traductor actual, notify «Vuelve a abrir el gestor MITO para aplicar el idioma
seleccionado.» (timeout 10 s); `refresh_mcp_monitor()` (F12); `check_network()` (F9).

## Comportamiento de métodos clave

### `current_profile()`
Devuelve el valor de `#profile` si es `str`; si el select está vacío (`Select.BLANK`) devuelve `None`.

### `load_catalog()`
Si `entries` está vacío, rellena `entries = [(key, manifest, engine.catalog.compatibility(key, manifest))
for key, manifest in engine.catalog.all()]`. Después llama a `filter_catalog()`.

### `filter_catalog()`
1. `term = #search.value.lower()`; `table.clear()`; `selected = None`; `catalog_installed = set(engine.installed())`.
2. Deshabilita `#install`, `#versions`, `#manifest`.
3. `rows = rank_entries(entries, term, disabled_buckets)` (ver `search.md`: sin palabras devuelve todo
   ordenado por clave; con palabras filtra por cobertura y ordena por cobertura, coincidencias de clave,
   de bucket, aplicación exacta y clave).
4. `#details` = «Selecciona una aplicación.» si hay filas; si no, «Sin resultados en los buckets seleccionados.».
5. Por fila: `add_row(key, str(manifest.get('version', '?')), availability, Text(license_label(manifest)), key=key)`
   donde `availability` = «Instalado» si `key in catalog_installed`; si no, «Disponible» si `status == 'Compatible'`;
   si no, «Todavía no soportado».
6. Resumen en `#summary`: «{p0} componentes · {p2} disponibles» + salto + línea de orden, con `p0 = len(rows)` y
   `p2` = filas `Compatible` no instaladas. Si no hay filas, la primera línea se escribe en negrita azul
   (`bold #4da6ff`) y se añade el aviso «Sin componentes…» con `refresh='Actualizar catálogos'` y
   `configuration='Configuración'` dentro del texto traducido.

`on_input_changed` (`#search`) vuelve a llamar a `filter_catalog()` si la app está montada. Cambiar una
casilla de bucket: `disabled_buckets` se actualiza, se escribe `core/state/catalog-filters.json` como lista
ordenada y se llama a `filter_catalog()`.

### `on_data_table_row_highlighted`
Ignora filas no presentes. `selected = str(row_key.value)`. Busca `(manifest, status)` en `entries`.
Si existe:
- `#manifest` habilitado.
- `#install` deshabilitado si `busy`, o instalado, o `status != 'Compatible'`.
- `#versions` deshabilitado si `busy` o `status != 'Compatible'` (también para instalados: elegir otra versión sustituye la instalada).
- `availability = «Instalado»` si instalado, si no `tr(status)` (aquí se muestra el motivo bruto, distinto de la columna).
- Texto de `#details` (`Static` sin markup): `'{key} · {version}\n{description}\n{availability}'` y, en este orden:
  - `'\npre_install: se ejecuta con PowerShell durante la instalación.'` si `architecture.64bit.pre_install` o `pre_install` (F7).
  - `'\n' + '{hook}: se ejecuta con PowerShell durante la instalación.'` con `hook='installer.script'` si el instalador (`architecture.64bit.installer` o `installer`) es dict con `script` (F7); igual con `installer.file`.
  - `'\nPaso omitido: MITO no ejecuta {p1}.'` con `p1='post_install'` si `architecture.64bit.post_install` o `post_install` (F7).
  - Si hay notas: `'\n\nNOTAS\n{p1}'` con las notas unidas por salto de línea (lista) o `str` (F6).
- Desplaza `#details-scroll` al inicio sin animación.

### `on_checkbox_changed` (casillas de Inicio)
Las casillas de bucket están en `#bucket-*`; las de componentes en `#installed-list Checkbox`.
Ignora eventos si `rebuilding`, `busy`, la casilla no está montada o no pertenece a la lista.
Solo un cambio sobre la casilla mostrada es intención del usuario: si hay profile y el profile coincide con
`checkbox_states` y `event.value != displayed_value`, se registra el nuevo valor y, si la pertenencia del
profile no coincide con el valor, se lanza `begin('toggle', profile, name, value)` → `engine.set_active`.
Así, eventos encolados tras un cambio de profile no modifican el profile nuevo.

### `refresh_home()` / `_refresh_home()`
Serializado con `asyncio.Lock`. Pone `rebuilding = True`, reconstruye `#installed-list` y calcula:

- `inventory = engine.installed()`; `active = engine.profile(profile)['apps']` si hay profile.
- Fila de componente (índice `i`), contenedor `Horizontal` con clases `component` y `component-row-dark|light`
  (la primera es oscura: `len(rows) % 2 == 0` → dark). Etiqueta de casilla: `'{key}   ·   {version}'` + `'   ·   retenida'` si `pinned`.
  Casilla `active-{i}`, `name=installation`, valor = `installation in active`, `disabled = not profile or busy`.
- Si `installation in active and can_open(lock)`: botón «Abrir» `open-{i}` (deshabilitado si busy), «Acc. directo»
  `link-{i}` (clase `app-shortcut`), «Extensiones ✓/✗» `openwith-{i}` (✓ si `open_with.entries` no vacío; clase `open-with-button`),
  «Protocolos» `protocol-{i}` (clase `protocol-button`).
- Si `can_open(lock)`: «Host · ✓» o «Host · ✗» `expose-{i}` (✓ si `host_exposure.linked`; clase `host-data-button`).
- Siempre: «Info» `info-{i}` (clase `component-info`).
- Si `available_update(engine, lock, catalog)` devuelve candidato: «Actualizar» `update-{i}` (clase `component-update`,
  variant success, deshabilitado si busy o `status != 'Compatible'`, tooltip `'Versión {p1} · {p3}'` con `p3 = tr(status)`).
  Si `status == 'Compatible'` la instalación entra en `pending`.
- «Eliminar» `remove-{i}` (clase `component-remove`, deshabilitado si busy).
- Filas de host (`host_inventory`), índice compartido con las anteriores para la alternancia de color, clases
  `component host-component` + sombra: `Static.host-app-name` con `'{stem} · Host'` (+ `' · No disponible'` si falta el archivo),
  «Abrir» `host-open-{id}` (deshabilitado si busy, no disponible o sin profile), «Cache · ✓/✗» `host-cache-{id}`,
  «Data · ✓/✗» `host-data-{id}`, «Acc. directo» `host-link-{id}`, «Info» `host-info-{id}`, «Eliminar» `host-remove-{id}`.
- `#bulk-update`: texto `'Actualizar todos ({p1})'`, visible si `len(pending) > 1`, deshabilitado si busy.
- Sin filas: `Static` «Todavía no hay componentes instalados.\nUsa el botón de abajo para añadirlos.».
- `#installed-summary`: `'{p0} instalados · {p2} activos en {p4} · {p6} del host\nLos cambios de entorno se cargan al abrir una nueva pestaña.'`
  con `p4` = profile o «ningún profile».

### `begin(action, profile, target, active=None, badge=None, coupled=False, arguments=(), shortcut=None, color=None)`
1. `start_busy_indicator()`.
2. Deshabilita todos los `Button`, `Checkbox` y `Select` salvo `#copy-log`.
3. Llama a `operation(...)` (worker de hilo; ver siguiente sección).

### Indicador de ocupado
`start_busy_indicator`: `busy = True`; `busy_sequence += 1`; limpia `#busy-indicator`; programa a los 2 s
`show_busy_indicator(sequence)`, que escribe `'⏳ …'` solo si sigue `busy` y la secuencia coincide.
`stop_busy_indicator`: `busy = False`; `busy_sequence += 1`; limpia el indicador. Una operación antigua
no puede mostrar el indicador de una nueva.

### `operation(action, profile, target, active, badge, coupled, arguments, shortcut, color)` (worker de hilo, `exit_on_error=False`)
- Crea `Operation(progress)`; `current_operation = op`; `monitored(op)` en `ExitStack`.
- `progress(state)`: en el primer estado `active` programa (en el hilo UI) `schedule_operation_dialog(op)`.
- `log(message, level)` y `confirm(downloads)` llaman a la UI con `call_from_thread`.
- `Cancelled` → log `'Instalación cancelada. No se ha publicado nada.'` (nivel error).
- `Exception` → `capture(root, exc, operation='ui.' + action, component=...)`; log `'Error: {p1}'` (nivel error).
  Si es `DownloadIntegrityError` → `push_screen(DownloadIntegrityDialog(exc))`.
- `finally`: cierra el `ExitStack`; `close_operation_dialog()`; `operation_done(profile)` (la variable
  `profile` puede haber cambiado: `create` y `delete-profile` la reasignan).

Tabla de acciones del worker:

| Acción | Llamada del motor | Logs de éxito | Fase |
|---|---|---|---|
| `check-updates` | `wait_startup_updates`; `refresh_component_updates(force=True)`; `refresh_mito_update(force=True)` | «Búsqueda de actualizaciones finalizada.» | F6 / F11 |
| `host-add` | `host_apps.discover` (errores → log «No se pudieron revisar los accesos del host: {p1}»); si hay hallazgos, `HostDiscoverDialog` (devuelve un archivo o `'browse'`, que pasa a `choose_file()`); sin hallazgos, `choose_file()` directamente; `host_apps.add`; luego por cada tipo `data`, `cache` sin entradas: `host_data.candidates` y `offer_host_data` | log «Buscando aplicaciones en el menú Inicio y el escritorio del host…», «Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado.», «Buscando carpetas del host: {p1}…» | F8 |
| `host-cache` / `host-data` | si ya hay enlaces: confirmación `'¿Eliminar los enlaces de Host {p1}?\n{p3}\nLos datos del host se conservan.'` (botón «Desenlazar») → `host_data.unlink`; si no: buscar carpetas, ofrecer cada una, y si no queda enlace preguntar `'¿Seleccionar otra carpeta del host manualmente?'` (botón «Seleccionar») → `choose_folder` | «Buscando carpetas del host: {p1}…» | F8 |
| `open-with` | `open_with.apply(engine, profile, target, arguments, log)` | (los de `apply`) | F8 |
| `host-expose` | `host_exposure.entries`; si ya hay: confirmación `'¿Eliminar los enlaces del host?\n{p1}\nLos datos de MITO se conservan.'` → `unlink` + log; si no: `candidates`, y por cada candidato `plan` y confirmación (`'{p0} en el host: ¿crear un enlace?…'`, texto completo en «Cadenas») y, si el host ya existe y no apunta al origen, `'La carpeta del host {p1} ya existe. Se renombrará a {p3} y se restaurará al desenlazar.'` (botón «Apartar y enlazar») → `link` | «No hay carpetas en el AppData de MITO que coincidan con el ejecutable. Abre antes la aplicación desde MITO.»; «Enlace creado en el host: {p1}» | F8 |
| `host-open` | `host_apps.launch(engine, profile, target, arguments, coupled=coupled)` | — | F8 |
| `host-remove` | `host_apps.remove(root, target)` | «Referencia eliminada. El ejecutable y los datos del host se conservan.» | F8 |
| `host-link` | `host_apps.shortcut(engine, profile, target, badge, coupled, arguments, color)` | «Acceso directo al host creado: {p1}» (nombre del `.lnk`) | F8 |
| `create` | `engine.create_profile(target)` (devuelve el nombre; reasigna `profile`) | «Profile {p1} creado» | F6 |
| `export-profile` | `profile_transfer.export_profile(engine, profile, target[0], log)` | «Profile exportado: {p1}» | F11 |
| `import-profile` | `profile_transfer.import_profile(engine, target[0], target[1], confirm_plan=…, confirm_unverified=confirm, log)`; `confirm_plan` → `confirm_import` (diálogo de revisión) | — | F11 |
| `toggle` | `engine.set_active(profile, target, active)` | — | F6 |
| `remove-component` | `components.remove(engine, target, log)` | (los del motor) | F6 |
| `update-component` | `components.update(engine, target, log, confirm_unverified=confirm)` | (los del motor) | F6 |
| `update-all` | `components.update_all(engine, target(list), log, confirm_unverified=confirm)` | (los del motor) | F6 |
| `install` | `install_from_catalog(target, log, confirm)` | (ver sección) | F6 |
| `versions` | `install_version_from_catalog(target, log, confirm)` | (ver sección) | F6 |
| `release-version` | `components.release_version(engine, target, log)` | (los del motor) | F6 |
| `delete-profile` | `profile = engine.delete_profile(profile)` | «Profile {p1} eliminado. Se conservan componentes, home y workspace.» | F6 |
| `mito-shortcut` | `shortcuts.mito_link(engine, profile, badge, color=color)` dentro de `mutation(root)` | «Acceso a MITO creado en el escritorio: {p1}» | F8 |
| `launch` | `engine.launch(profile, target, arguments=, coupled=, shortcut=)` | (ninguno) | F6 (F8 acoplado) |
| `application-shortcut` | `shortcuts.application_link(...)` dentro de `mutation(root)` | «Acceso directo creado en el escritorio: {p1}» | F8 |
| `refresh` | por cada bucket de `catalog.sources()`: `import_bucket(bucket, source)` dentro de `mutation(root)`; luego `entries = []` | «Actualizando {p1}…», «{p0}: {p2} manifiestos» | F6 |

`check-updates` y `refresh` no exigen profile. `refresh` vacía `entries`, así que el catálogo se recarga
al volver a él (`operation_done` recarga solo si la página activa es `catalog-page`).

### `install_from_catalog(target, log, confirm)`
1. `previous` = versión mostrada en `entries` para `target`; si no, la del catálogo `resolve(target)`.
2. `call_from_thread(wait_startup_updates)`: espera a los workers `component-updates` y `self-update-check`.
3. Log `'Comprobando {p1}…'`.
4. `catalog.refresh_keys([target], log)`; si el resultado no es `1`, **retorna sin error** (ver Dudas).
5. `entries = []`.
6. `engine.install(target, log, confirm_unverified=confirm, confirm_plan=review)`.
   `review(records)`: si la versión nueva es mayor que `previous` (ambas válidas según `version_key`)
   se muestra `InstallVersionDialog` (`confirm_install_version`); si se cancela, `YitoError('Instalación cancelada')`.
   Si la nueva no es mayor (o no comparable), no se pregunta y devuelve `True`.

### `install_version_from_catalog(target, log, confirm)`
1. `_, manifest = resolve(target)`; log `'Consultando versiones de {p1}…'`.
2. `found = versions.history(root, catalog, target)`. Si lanza, `capture('versions.history')` y log
   `'No se pudo consultar el histórico del bucket: {p1}'` (`found` queda `None`).
3. `installed = engine.installed().get(target)`.
4. `version = call_from_thread(choose_version, target, str(manifest.get('version','?')), installed.version or None,
   found, isinstance(manifest.get('autoupdate'), dict), found is not None, loader)`.
   `loader` = `lambda current, wanted: versions.load_versions(root, current, wanted)`; `VersionsDialog` lo
   llama como `loader(found, wanted)` (ver Dudas).
5. Vacío/cancelado → log `'Selección de versión cancelada'` y retorno.
6. `chosen, pin = versions.resolve(root, catalog, target, version, found=found if found is not None else {}, log)`.
7. `components.install_version(engine, target, chosen, pin, log, confirm_unverified=confirm)`.

### `long_step(line)` (aviso centrado «PREPARANDO ARCHIVOS»)
- Si hay `current_operation` activa: retorno (el diálogo de progreso ya muestra el paso).
- `long_step_sequence += 1`.
- `line.active` (es `LongStep` con `active=True`): programa a los 2 s `show_long_step(sequence, str(line))`
  que crea `LongStepDialog` solo si la secuencia coincide y no hay diálogo.
- `line.active = False`: descarta el diálogo actual con `finish()` (y cualquier timer pendiente queda invalidado por la secuencia).
- `LongStepDialog`: título «PREPARANDO ARCHIVOS», mensaje del paso (texto plano), contador
  `'Tiempo transcurrido: {p1} s'` que empieza en 0 y sube cada 1 s. `finish()`: marca `finished`; cierra
  de inmediato si es la pantalla superior, si no lo hace en el siguiente tick.

### `OperationDialog` (progreso con cancelación cooperativa)
- Se muestra a los 2 s del primer estado `active` (solo si la operación sigue activa, es la actual y no hay diálogo).
- Composición: título (`state.title`, por defecto «INSTALANDO»), texto (`state.text`), `ProgressBar`
  (indeterminada si `total` es falsy; si no `update(total, progress=min(received or 0, total))`), detalle, nota
  (`state.note`), contador `'Tiempo transcurrido: {p1} s'` (segundos enteros desde la apertura, reloj monótono),
  botón `#operation-cancel` «Cancelar instalación» (variant error, ancho 100 %).
- `tick()` cada 0.25 s: si `finished` o `!state.active` → `finish()` (cierra; si no es la pantalla superior,
  lo hará el siguiente tick). Si `state.started` cambió respecto a `phase`, reinicia las muestras de velocidad.
  Si `operation.cancelled`: botón «Cancelando…» deshabilitado; si no, deshabilitado solo si `!state.cancellable`.
- `detail(state)`:
  - `received is None` → cadena vacía.
  - `unit == 'files'` → `'{p1}/{p2} archivos'` con `p2 = total` o `?`.
  - Bytes: muestras `(t, received)` de los últimos 5 s y con `received` no decreciente; velocidad =
    `(received - first) / (now - first_t)` si el intervalo es ≥ 1 s. Texto: `'{h(received)} de {h(total)}'` si hay total,
    si no `'{h(received)} recibidos'`; si hay velocidad se añade `' · {h(speed)}/s'`.
- Botón: solo si `state.cancellable`, `operation.cancel()` y `tick()`. El motor comprueba la cancelación entre
  pasos y en las lecturas de red (cada 0.25 s); la cancelación no es posible tras el inicio de la publicación
  (`cancellable = False`).

### `write_log(message, level='info')`
Si es `LongStep`, llama a `long_step(message)`. Convierte a `str`; escribe `diagnostics.write(root, level, 'ui', message)`
(en `logs/mito.log`); añade el texto a `log_messages`; escribe en `#log` un `Text` con estilo `bold red` si
`level == 'error'`, si no sin estilo. Sin markup.

### `copy_log(text)` (worker `clipboard`, exclusivo)
Ejecuta `powershell.exe -NoLogo -NoProfile -NonInteractive -Command '[Console]::InputEncoding = UTF-8; Set-Clipboard -Value ([Console]::In.ReadToEnd())'`
con `input=text` (por stdin), `encoding=utf-8`, `timeout=10`, `CREATE_NO_WINDOW`. Éxito: notify «Log copiado al portapapeles».
`OSError` o `SubprocessError`: notify «No se pudo copiar el log al portapapeles» (severidad error).
El texto nunca viaja como código ni como argumento.

### `human_size(value)`
`size = float(value or 0)`; para `B`, `KB`, `MB`, `GB` (dividiendo entre 1024 en cada paso): si `size < 1024` o es
la última unidad, devuelve `'{size:.0f} B'` para bytes y `'{size:.1f} {unidad}'` para el resto. Ejemplos: `0` → `0 B`;
`5*1024*1024` → `5.0 MB`.

### `license_label(manifest)`
`license = manifest.get('license')`. Si es dict, toma `identifier` o `url`. Devuelve el texto de `license`
sin espacios si es `str` no vacío; en otro caso la clave «No declarada». El identificador de licencia no se traduce.

### `operation_done(profile)`
Habilita todos los widgets; reasigna las opciones del profile (sin disparar `Select.Changed`) y selecciona
`profile` si existe; `stop_busy_indicator()`; `refresh_home()`; deshabilita casillas si no hay profile; recarga
el catálogo si la página activa es `catalog-page`.

### `action_quit()` (Ctrl+Q)
- `busy`: notify «Espera a que termine la operación antes de salir» (sin salir).
- MCP activo (`mcp.state != 'disabled'`): `mcp_quit = True`; `mcp.stop()` (la salida ocurre al parar el servidor).
- Si no: `exit()`.

### `start_network_poll()` (F9)
Lee `network.poll_seconds` (por defecto `20`; valor no numérico → `20`). Si es > 0 y finito, programa
`poll_network` cada `max(5.0, seconds)` s. Con 0 no hay temporizador (`network_timer` no existe).

## Diálogos de `ui.py`

Todos son `ModalScreen` (ver `ui_dialogs.md` para `ModalScreen` y `DialogHeader`). Cada uno devuelve un
valor con `dismiss`. «Cancelar» o `Esc` devuelven el valor de cancelación indicado.

### ManifestDialog(key, manifest) → sin valor
- Contenedor `Vertical#manifest-dialog` (90 % x 90 %, padding `1 2`, borde `#b5dc64`).
- `DialogHeader('Manifiesto', 'manifest-close')`.
- `Static#manifest-component` (clave, markup off, altura auto, margen inferior 1).
- `TextArea#manifest-content` solo lectura, sin ajuste de línea, altura 1fr, con `json.dumps(manifest, ensure_ascii=False, indent=4)`; foco al montar.
- `Esc` o `manifest-close` cierran. F6.

### InstallVersionDialog(key, previous, version) → `True` si «Instalar», `False` si cancelado
- `VerticalScroll#install-version-dialog` (ancho 68, altura auto, max 90 %).
- `Label.title` «Nueva versión disponible»; `Static` con `'{key}\n{previous} → {version}'`.
- `Horizontal`: `Button#install-version-cancel` «Cancelar»; `Button#install-version-confirm` «Instalar» (variant success).
- `Esc` → `False`. F6.

### VersionsDialog(key, current, installed, found, autoupdate, history_available, loader=None) → `str` (versión) o `None`
- Ancho 76, altura auto, max 90 %. `history_versions(found, HISTORY_BATCH)` con `HISTORY_BATCH = 20`.
- `DialogHeader('VERSIONES DEL COMPONENTE', 'versions-close')`.
- `Static`: `'{key}\n' + 'Catálogo: {p1} · Instalada: {p3}'` (`p3` = instalada o `—` literal).
- Con versiones: `Static` «Versiones publicadas en el histórico Git del bucket, con el manifiesto y el hash de entonces.»;
  `Select#versions-list` (prompt «Selecciona o escribe una versión»; opciones `'{version} · {date}'` → `version`);
  `Button#versions-more` (texto `'Cargar {p1} más'` con `p1 = 20`, o «Sin más versiones»; deshabilitado si no hay más).
- Sin versiones: `Static` «No hay versiones en el histórico Git del bucket (sólo buckets de GitHub).».
- Si `autoupdate`: `Static` «Otra versión se genera desde autoupdate, como scoop install app@versión. Si no se obtiene su hash, se pedirá confirmación.»;
  `Input#versions-input` placeholder «Otra versión, por ejemplo 1.2.3».
- `Static` «La versión elegida sustituye a la instalada y queda retenida: Actualizar no la cambiará hasta liberarla en Info.».
- `Horizontal#versions-actions`: `Button#versions-cancel` «Cancelar»; `Button#versions-confirm` «Instalar» (success).
- `on_button_pressed`: `versions-more` → `load_more()`. `versions-confirm`: el valor es el texto del input (sin espacios) si
  hay input y no está vacío; si no, el select (`str`); si no hay nada → notify «Selecciona o escribe una versión» (warning) y sigue abierto.
  Cualquier otro botón (cancelar, cerrar) o `Esc` → `dismiss(None)`.
- `load_more()`: deshabilita el botón, texto «Cargando…», `wanted = limit + 20`; worker de hilo (`exit_on_error=False`)
  llama `loader(found, wanted)`; al terminar `call_from_thread(show_more, wanted, error)`.
- `show_more(wanted, error)`: con error → notify `'No se pudo consultar el histórico del bucket: {p1}'` (severidad error);
  si no, `limit = wanted`, `versions` se recalcula, el select conserva la selección si sigue existiendo. Siempre
  reasigna la etiqueta y `disabled` del botón. F6.

### OpenApplicationDialog(name, entries=()) → `None` o tupla
- `Vertical#open-application-dialog` (ancho 62, altura auto).
- `Label.title` «Abrir»; `Static` con el nombre (markup off).
- Si hay `entries`: `Select#application-entry` (opciones `(label, label)`, valor = la primera, sin vacío).
- `Label` «Argumentos» + `Input#application-arguments` (foco al montar).
- `Label` «Proceso» + `Select#application-process` con opciones `('detached', False)` y `('attached', True)` (valor `False`, sin vacío).
  **Etiquetas literales en inglés, no traducidas** (ver Dudas).
- `Horizontal`: `Button#application-open-cancel` «Cancelar»; `Button#application-open-independent` «Abrir» (primary).
- Resultado de `selected()`: `(bool(proceso), split(argumentos))` y, solo si hay `entries`, `+ (entrada,)`.
  `split` es el tokenizador de `arguments` (`run "two words"` → `['run', 'two words']`).
- `Enter` en el input y el botón `application-open-independent` llaman a `selected()`. `Cancelar` y `Esc` → `None`. F6 (F8 para «attached»).

### ShortcutDialog(application=False, entries=(), engine=None, pocketool=False) → tupla o `None`
- Valores por defecto: `badge_text(engine)` (lee `shortcut.badgeText` de `config.properties`; por defecto `MITO`)
  y `badge_color(engine)` (lee `shortcut.badgeColor`; por defecto `naranja`). **Un valor inválido en el archivo lanza `YitoError` sin capturar en el constructor** (ver Dudas).
- `Vertical#shortcut-dialog` (ancho 54, altura auto).
- `Label.title` «Generar acceso directo».
- Si `entries`: `Select#shortcut-entry`.
- Si `application`: `Label` «Argumentos» + `Input#shortcut-arguments`.
- Si `application` y no `pocketool`: `Label` «Proceso» + `Select#shortcut-coupled` (`detached`/`attached`, sin traducir).
- `Label` «Letras del icono (máximo 4)» + `Input#shortcut-badge` (valor por defecto, `max_length=4`).
- `Label` «Color del fondo» + `Grid#shortcut-colors` (4 columnas, 2 filas): un `Button#shortcut-color-<nombre>` por color de
  `BADGE_COLORS` en orden `negro, rojo, azul, naranja, rosa, verde, marron, morado`, con etiqueta `nombre.capitalize()`
  (`Negro`, `Rojo`, `Azul`, `Naranja`, `Rosa`, `Verde`, `Marron`, `Morado`).
  Al montar: fondo = RGB del color (`#rrggbb`), texto blanco. El seleccionado lleva `'✓ '` delante y borde `heavy white`; los demás `none`.
- `Static#badge-error` (color `#ff9090`).
- `Horizontal`: `Button#shortcut-cancel` «Cancelar»; `Button#shortcut-confirm` «Generar» (primary).
- `confirm()`: `badge_text(None, input)` (recorta, mayúsculas, vacío → `MITO`; más de 4 o no alfanumérico →
  «La etiqueta admite de 1 a 4 letras o números») y `badge_color(None, selected)` («Elige un color de la paleta del icono»).
  Error → texto en `#badge-error`, sin cerrar.
  - Con `application`: `(badge, color, coupled_or_False, split(args))` + `(entrada,)` si hay `entries`.
    `coupled` es `False` siempre si `pocketool`.
  - Si no: `(badge, color)`.
- `Enter` en la etiqueta de un diálogo de aplicación enfoca los argumentos; en el resto confirma.
- `Esc` y «Cancelar» → `None`. F8.
- Desde `ui.py`, `Generar acceso directo` aparece en: `link-<n>`, `host-link-<id>` y `mito-shortcut` (perfil).

### RemoveComponentDialog(lock, profiles) → `True` si «Eliminar»
- `VerticalScroll#remove-dialog` (ancho 68, borde `#e27878`).
- `Label.title` «Eliminar componente»; `Static` `'{key} · {version}'`.
- `Static` `'Se retirará de estos profiles: ' + (profiles unidos por ', ' o «ninguno»)` (markup off).
- `Static` «Se eliminarán los archivos de este componente. Se conservan el home, los datos persistentes y los workspaces. Las aplicaciones dependientes y sus accesos pueden dejar de funcionar. Cierra el componente antes de eliminarlo.».
- `Horizontal`: `Button#remove-cancel` «Cancelar» (con foco al montar); `Button#remove-confirm` «Eliminar» (error).
- `Esc` → `False`. F6.

### ComponentInfoDialog(root, lock, profile) → `None` o `'release-version'`
Cálculo en el constructor:
- `manifest = catalog.effective(lock['manifest'])`.
- `separate = isolated(lock)`.
- `scope`: si separado y profile → `'Profile: {p1}'`; si separado sin profile → «Selecciona un profile para ver sus datos»; si no → «Compartido entre profiles».
- `paths`: `('Binarios base', inside(root, lock.path))`; si separado y profile, `('Binarios del profile', payload_path(...))`;
  si no es separado o hay profile, por cada `persistence.locations(root, lock, profile)`: `('Persistencia: {p1}', ruta)`.
- `environment` = `Engine(root).component_environment(lock, profile)` si no es separado o hay profile; si no `None`.

Composición: `Vertical#component-info` (ancho 90 %, máx 110, altura 85 %, padding `1 0 1 2`, borde `#b5dc64`);
`DialogHeader('INFORMACIÓN DEL COMPONENTE', 'info-close')`; `VerticalScroll#info-content` > `Vertical#info-content-body`:

1. `Static`: `'{key} · {version}\n{scope}'`.
2. `Static#info-description`: descripción o «Sin descripción.».
3. Si `pinned`: grupo `info-group` con `Static.info-group-title` «VERSIÓN RETENIDA»; texto
   `'{p1} elegida desde {p3}. Actualizar no la sustituirá.'` (`p1` = versión; `p3` = `source` + ` ` + `commit[:10]` si hay commit)
   y `Button#info-release-version` «Liberar».
4. Grupo «RUTAS DEL COMPONENTE». Por cada ruta: `Static.info-path-label` (etiqueta) y `Static` con
   `ruta` + estado (`''` si es carpeta; `'\nArchivo: se abrirá su carpeta'` si es archivo; `'\nTodavía no creada'` si no existe);
   `Button#info-open-{i}` «Abrir», deshabilitado si la ruta no existe. Si el manifiesto no declara `persist`:
   `Static` «Este componente no declara persistencia.».
5. Grupo «APORTACIÓN AL ENTORNO»: `Static` «Se aplica al activar el componente. Otras variables del entorno pueden sobrescribir estos valores.».
   - Sin `environment`: «Selecciona un profile para ver sus datos».
   - Con `environment`: subtítulo «Variables de entorno» (`.info-subtitle`); sin variables → `'—'`; por cada variable una fila
     (`environment_row(nombre, valor)`). Subtítulo literal `path`; sin rutas → `'—'`; por cada ruta una fila `environment_row('', ruta, path_entry=True)`.
   - `environment_row`: `candidate = Path(valor sin comillas)`; `is_path` = absoluta y sin `;`. Si es ruta o `path_entry`:
     estado `'\nArchivo: se abrirá su carpeta'` si es archivo, `''` si es carpeta, `'\nTodavía no creada'` si no existe o es `None`;
     `Button#info-env-open-{n}` «Abrir», deshabilitado si `None` o no existe. Los valores son literales (no se expanden variables).
6. Grupo «NOTAS» (`'NOTAS'`) con las notas unidas por salto (o `str`). Solo si hay notas.
7. Avisos de hooks (F7): `'pre_install: se ejecuta con PowerShell durante la instalación.'` si `pre_install`;
   `'{hook}: se ejecuta con PowerShell durante la instalación.'` con `installer.script` / `installer.file`
   si `installer` (de primer nivel) es dict con esa clave; `'Paso omitido: MITO no ejecuta {p1}.'` con `post_install`.

Acciones: `info-close` o `Esc` → `dismiss()`; `info-release-version` → `dismiss('release-version')`;
`info-open-{i}` / `info-env-open-{n}` → `persistence.open_location(ruta)`; error (`OSError`, `YitoError`) →
`capture('component.open-location')` + notify `str(exc)` (error). F6 (hooks F7; la apertura de rutas F6).

### UnverifiedDownloadsDialog(downloads) → `True` si «Descargar e instalar»
- `VerticalScroll#unverified-dialog` (ancho 78, máx 85 %, borde `#e5ba65`, fondo de superposición `#00000088`).
- `Static` «DESCARGAS SIN HASH»; `Static` «No se podrá comprobar la integridad de estas descargas. ¿Quieres continuar?»;
  `Static` con las descargas (una por línea, markup off).
- `Button#unverified-cancel` «Cancelar» (foco al montar); `Button#unverified-confirm` «Descargar e instalar» (warning).
- `Esc` → `False`. Usado por las instalaciones (F6) y por la aprobación de trabajos MCP (F12).

### DownloadIntegrityDialog(error) → `None`
- `VerticalScroll#integrity-dialog` (ancho 90, máx 95 %, borde `#e27878`).
- `Static#integrity-title` (color `#e27878`, negrita) «DESCARGA ALTERADA POR LA RED».
- `Static` «Lo que ha llegado no es el archivo publicado, así que MITO lo ha descartado y no ha instalado nada.».
- Si `error.portal`: «Causa más probable: el portal de autenticación de la red. Pulsa INTERNET, conéctate y vuelve a intentarlo.»; si no: «Causa más probable: el proxy o el antivirus de la red ha sustituido o modificado la descarga. No es un fallo de MITO ni del catálogo. Prueba desde otra red o pasa estos datos a soporte.».
- `TextArea#integrity-details` solo lectura, ajuste de línea, altura 4: `'URL: {p1}\nRecibido: {p2} bytes desde {p3}'`
  (`p1` = URL o `?`; `p2` = tamaño o `?`; `p3` = host o `?`).
- `Button#integrity-close` «Cerrar» (primary, foco al montar). Cualquier botón o `Esc` → `dismiss(None)`. F6.

### LongStepDialog(message) → sin valor
Ver «`long_step`». Fondo `#00000088`, borde `#e5ba65`, ancho 72. F6.

### OperationDialog(operation) → sin valor
Ver «`OperationDialog`». Ancho 76, borde `#e5ba65`. Sin binding de `Esc`. F6.

### `DataTable` de catálogo (no es un diálogo)
Columnas: «Componente», «Versión», «Disponibilidad», «Licencia». Ver «`filter_catalog`».

## Operaciones de instalación y entorno (resumen de comportamiento)

- Una sola operación a la vez: `begin` deshabilita casi todo; `busy` impide otras acciones.
- Cancelación cooperativa: el botón de cancelar del `OperationDialog` solo está activo mientras `cancellable`.
  Al cancelar, el worker recibe `Cancelled` y registra «Instalación cancelada. No se ha publicado nada.».
  Un `YitoError('Instalación cancelada')` lanzado desde `review` (diálogo de versión) se trata como error normal (log «Error: …»).
- Un hilo nunca toca widgets: usa `call_from_thread`.
- Eventos de casillas se ignoran durante `rebuilding`; ver «`on_checkbox_changed`».
- El log se escribe en `logs/mito.log` mediante `diagnostics.write` (sin telemetría remota).

## Cadenas visibles al usuario (claves i18n exactas)

Fuente de traducción: `core/app/mito/locales/<idioma>.json`. Las claves son estas cadenas españolas.
Las etiquetas marcadas «literal» no pasan por `tr` en el original y deben mostrarse tal cual.

### Pantalla principal y barra lateral (F6 salvo nota)
```
PROFILES · Selecciona un profile · Gestionar profile · ACCIONES
Inicio · Instalados · Buscar actualizaciones · Abrir Home · Configuración · Pocketools (F10)
Actualizar MITO (F11; etiqueta: 'Actualizar MITO' + ' · ' + versión + ' · Offline' literal si local_path)
MCP - disabled / MCP - enabled (literal, F12) · MCP · Jobs (literal, F12)
Comprobando Internet… (F9) · Internet accesible · Internet no accesible · Sin conexión de red (F9) · Diagnosticando la red… (F9)
MIS COMPONENTES · Buscar e instalar nuevos componentes · Agregar aplicación del host (F8)
Actualizar todos ({p1})                                         p1 = número de pendientes
BUSCAR E INSTALAR · CATÁLOGOS SCOOP · Buscar aplicación, runtime o herramienta…
Selecciona una aplicación. · Sin resultados en los buckets seleccionados.
Instalar · Versiones… · Manifiesto · Actualizar catálogos · Volver a Inicio · Copiar log
Componente · Versión · Disponibilidad · Licencia                 columnas
Instalado · Disponible · Todavía no soportado                    disponibilidad
No declarada                                                     licencia
{p0} componentes · {p2} disponibles\nOrden: palabras coincidentes, bucket/nombre y descripción. Coincidencias parciales al final.
Sin componentes. Pulsa el botón "{refresh}". Si no aparece ninguno comprueba tus fuentes en "{configuration}"
                                                                 refresh = 'Actualizar catálogos'; configuration = 'Configuración'
Todavía no hay componentes instalados.\nUsa el botón de abajo para añadirlos.
{p0} instalados · {p2} activos en {p4} · {p6} del host\nLos cambios de entorno se cargan al abrir una nueva pestaña.
                                                                 p4 por defecto: ningún profile
retenida                                                         sufijo de la casilla
Abrir · Acc. directo · Extensiones (con ' ✓' / ' ✗' literal) · Protocolos · Info · Actualizar · Eliminar
Host · (con '✓' / '✗' literal) (F8)
Cache · (con '✓' / '✗' literal) · Data · (con '✓' / '✗' literal) (F8)
Versión {p1} · {p3}                                              tooltip; p3 = tr(status)
{nombre} · Host (literal ' · Host') · ' · No disponible' (F8)
Selecciona un profile                                            aviso (warning)
Selecciona o escribe una versión                                 aviso (warning)
El log está vacío · Log copiado al portapapeles · No se pudo copiar el log al portapapeles
Espera a que termine la operación antes de salir
Configuración guardada. Abre nuevas pestañas o aplicaciones para usar el nuevo entorno.
Vuelve a abrir el gestor MITO para aplicar el idioma seleccionado.
Archivos del profile guardados.
Búsqueda de actualizaciones finalizada.
Comprobando actualizaciones de componentes instalados…
Manifiestos de componentes instalados comprobados: {p1}.
Error al comprobar actualizaciones: {p1}
No se pudo comprobar la actualización de MITO: {p1}
Comprobando {p1}…
Consultando versiones de {p1}…
Selección de versión cancelada
No se pudo consultar el histórico del bucket: {p1}
Instalación cancelada · Instalación cancelada. No se ha publicado nada.
Error: {p1}
Profile {p1} creado · Profile exportado: {p1} · Profile {p1} eliminado. Se conservan componentes, home y workspace.
Acceso a MITO creado en el escritorio: {p1} · Acceso directo creado en el escritorio: {p1}
Actualizando {p1}… · {p0}: {p2} manifiestos
Enlace creado en el host: {p1} · Acceso directo al host creado: {p1}
Referencia eliminada. El ejecutable y los datos del host se conservan.
Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado.
Buscando aplicaciones en el menú Inicio y el escritorio del host…
No se pudieron revisar los accesos del host: {p1}
Buscando carpetas del host: {p1}…
Host data · Host cache                                           etiquetas de enlace (F8)
{p0}: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nCompartido por todos los profiles. La aplicación podrá modificar los datos del host. Cierra la aplicación antes de enlazar.
La carpeta {p1} ya existe y ha de ser borrada para generar el enlace.\nSe perderá su contenido local en MITO.
Borrar y enlazar
¿Eliminar los enlaces de Host {p1}?\n{p3}\nLos datos del host se conservan.      + botón «Desenlazar»
¿Seleccionar otra carpeta del host manualmente?                                 + botón «Seleccionar»
¿Eliminar los enlaces del host?\n{p1}\nLos datos de MITO se conservan.         + botón «Desenlazar»
No hay carpetas en el AppData de MITO que coincidan con el ejecutable. Abre antes la aplicación desde MITO.
{p0} en el host: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nSi la aplicación arranca fuera de MITO (inicio automático, asociaciones) usará estos datos, pero no el entorno del profile. Cierra la aplicación antes de enlazar.
   p0 = Data (AppData/Roaming) o Cache (AppData/Local) según host_exposure.KINDS
La carpeta del host {p1} ya existe. Se renombrará a {p3} y se restaurará al desenlazar.   + botón «Apartar y enlazar»
Configuración de red guardada en config.properties
El proxy {p0} exige autenticación HTTP 407 ({p1}), que MITO no admite.           p1 por defecto: Basic
La comprobación de Internet respondió HTTP {p0}.
No se pudo llegar a Internet ni con el proxy de Windows ni directamente.
Conecta el equipo a una red                                      aviso (warning)
```

### Hooks y detalle de catálogo (F7)
```
pre_install: se ejecuta con PowerShell durante la instalación.
{hook}: se ejecuta con PowerShell durante la instalación.            hook = installer.script | installer.file
Paso omitido: MITO no ejecuta {p1}.                                   p1 = post_install
NOTAS · \n\nNOTAS\n{p1}
```

### Catálogo: motivos de compatibilidad (`catalog.compatibility`, mostrados en detalle)
La columna muestra «Todavía no soportado» cuando el estado no es `Compatible`; el detalle muestra el motivo
exacto. Motivos del motor (no traducidos en esta vista; provienen de `catalog.py`): ver `catalog.md`.

### Diálogos
```
Manifiesto · Cerrar · Cancelar · Instalar · Eliminar · Generar · Abrir · Guardar   (ver ui_dialogs para los de perfil)
Nueva versión disponible
VERSIONES DEL COMPONENTE
Catálogo: {p1} · Instalada: {p3}                                  p3 por defecto: —
Versiones publicadas en el histórico Git del bucket, con el manifiesto y el hash de entonces.
Selecciona o escribe una versión
Cargar {p1} más                                                   p1 = 20
Sin más versiones · Cargando…
No hay versiones en el histórico Git del bucket (sólo buckets de GitHub).
Otra versión se genera desde autoupdate, como scoop install app@versión. Si no se obtiene su hash, se pedirá confirmación.
Otra versión, por ejemplo 1.2.3
La versión elegida sustituye a la instalada y queda retenida: Actualizar no la cambiará hasta liberarla en Info.
Argumentos · Proceso · detached · attached   (literal, sin traducir)
Generar acceso directo · Letras del icono (máximo 4) · Color del fondo
Negro · Rojo · Azul · Naranja · Rosa · Verde · Marron · Morado   (capitalize de la clave)
La etiqueta admite de 1 a 4 letras o números
Elige un color de la paleta del icono
Eliminar componente · Se retirará de estos profiles:  · ninguno
Se eliminarán los archivos de este componente. Se conservan el home, los datos persistentes y los workspaces. Las aplicaciones dependientes y sus accesos pueden dejar de funcionar. Cierra el componente antes de eliminarlo.
INFORMACIÓN DEL COMPONENTE · Sin descripción.
Profile: {p1} · Selecciona un profile para ver sus datos · Compartido entre profiles
Binarios base · Binarios del profile · Persistencia: {p1}
VERSIÓN RETENIDA · {p1} elegida desde {p3}. Actualizar no la sustituirá. · Liberar
RUTAS DEL COMPONENTE · Abrir · Todavía no creada · Archivo: se abrirá su carpeta (con '\n' delante)
Este componente no declara persistencia.
APORTACIÓN AL ENTORNO · Se aplica al activar el componente. Otras variables del entorno pueden sobrescribir estos valores.
Variables de entorno · path (literal) · —    (literal)
NOTAS
DESCARGAS SIN HASH · No se podrá comprobar la integridad de estas descargas. ¿Quieres continuar? · Descargar e instalar
DESCARGA ALTERADA POR LA RED
Lo que ha llegado no es el archivo publicado, así que MITO lo ha descartado y no ha instalado nada.
Causa más probable: el portal de autenticación de la red. Pulsa INTERNET, conéctate y vuelve a intentarlo.
Causa más probable: el proxy o el antivirus de la red ha sustituido o modificado la descarga. No es un fallo de MITO ni del catálogo. Prueba desde otra red o pasa estos datos a soporte.
URL: {p1}\nRecibido: {p2} bytes desde {p3}                        p2 por defecto: ?
Cerrar
PREPARANDO ARCHIVOS · Tiempo transcurrido: {p1} s                  (LongStep y OperationDialog)
INSTALANDO                                                          título por defecto del progreso
{p1}/{p2} archivos                                                  p2 por defecto: ?
{p1} de {p2} · {p1} recibidos · (' · {velocidad}/s' literal)
Cancelar instalación · Cancelando…
Abrir · Cancelar (diálogo de aplicación)
Buscar componentes · Salir · Inicio · Cancelar · Cerrar (BINDINGS)
```

Textos de las cadenas literales de la CSS/TCSS y de los `Static` sin traducir se conservan tal cual. Las
cadenas de `ui_dialogs.md` (perfil, configuración, cabecera) siguen en su documento.

## Mapa de fases

| Funcionalidad | Fase | Notas |
|---|---|---|
| Pantalla, sidebar, barra de título, footer, Ctrl+F/Ctrl+Q/Esc | F6 | |
| Inicio: lista de instalados, casillas de activación, resumen | F6 | |
| Catálogo: búsqueda, filtros por bucket, detalle, manifiesto, columnas | F6 | |
| Instalar, Versiones…, Actualizar (componente), Actualizar todos, Eliminar componente | F6 | |
| Info de componente, rutas persistentes, Abrir de rutas, Liberar versión | F6 | |
| Actualizar catálogos | F6 | |
| Profiles: selector, crear, eliminar, configurar variables | F6 | |
| Configuración global (idioma, terminal, claves generales) | F6 | Claves de proxy F9, pocketool F10, mito.update_url F11, badge F8, MCP F12 |
| Abrir terminal (preparar Windows Terminal al cambiar de profile o guardar) | F6 | |
| Abrir aplicación independiente (Abrir de componente) | F6 | |
| Abrir aplicación acoplada (Proceso = attached) | F8 | |
| Log, Copiar log, LongStep, OperationDialog, busy indicator, descarga alterada | F6 | |
| Descargas sin hash (confirmación) | F6 | Se usa también desde MCP (F12) |
| Hooks: pre_install, installer.script/file, post_install (avisos) | F7 | |
| Agregar aplicación del host, Info/Eliminar host, Abrir host, Acc. directo host | F8 | |
| Cache / Data del host (enlaces) | F8 | |
| Host · ✓/✗ (exposición de AppData al host) | F8 | |
| Extensiones («Abrir con») | F8 | `open_with` |
| Protocolos | F8 | `protocols`, `ProtocolDialog` |
| Accesos directos: ShortcutDialog, Generar acceso directo (componente, host, perfil) | F8 | |
| Estado de Internet, diagnóstico, ProxyDialog, NetworkSetupDialog, poll de red | F9 | |
| Pocketools (botón, diálogo, claves pocketool.*) | F10 | |
| Actualizar MITO (botón, diálogo, prepare, offline) | F11 | |
| Exportar/Importar profile, revisión de importación | F11 | |
| Archivos de configuración del profile | F11 | |
| MCP (control, MCP · Jobs, confirmaciones, salida) | F12 | |
| Progreso y cancelación del `Operation` que usan las operaciones de F6 | F6 | Las operaciones de F8-F12 reutilizan el mismo canal |

## Tests de `test_ui.py`

| Test | Qué comprueba | Fase |
|---|---|---|
| `test_checkbox_states_have_distinct_red_and_green_colors` | CSS de casillas: apagado `#8A4B50`, encendido `#69FF64` | F6 |
| `test_busy_indicator_is_delayed_and_ignores_previous_operations` | Indicador a los 2 s; una secuencia antigua no lo muestra | F6 |
| `test_import_review_identifies_component_replacement` | Revisión de importación muestra «Sustituir 1 en todos los profiles que lo usan» y no «Ya instalado» | F11 |
| `test_real_update_refreshes_rows_and_preserves_activation` | Actualizar reemplaza la instalación, conserva la activación en los profiles que la tenían y no edita un profile tras cambiar de selección | F6 |
| `test_startup_refresh_detects_update_for_inactive_installation` | Actualización de componente inactivo: botón success habilitado, sin activar | F6 |
| `test_host_application_add_open_shortcut_info_and_remove` | Agregar app del host, Abrir (acoplado), Acc. directo con color naranja, Info (abrir carpeta), Eliminar con confirmación | F8 |
| `test_global_configuration_editor_controls_save_and_cancel` | Configuración: Esc no escribe; añade claves de proxy, variable de entorno, elimina env, guarda conservando comentarios | F6 (claves de proxy F9) |
| `test_pocketool_configuration_and_update_dialog` | Clave `pocketool.` y actualización de pocketool desde carpeta; error mostrado si falta el origen | F10 |
| `test_profile_configuration_editor_validation_and_save` | Configuración del profile: nombre reservado `PATH` rechazado; guardado con `CUSTOM` | F6 |
| `test_altered_download_is_explained_in_a_centered_dialog` | Descarga alterada: URL, tamaño y host en el TextArea; mensaje de proxy; Esc cierra | F6 |
| `test_long_engine_step_shows_centered_notice_that_closes_itself` | Paso sin progreso muestra «PREPARANDO ARCHIVOS» y se cierra al terminar; el log conserva el mensaje | F6 |
| `test_install_progress_dialog_shows_bytes_and_cancels_the_installation` | Progreso «5.0 MB de 20.0 MB»; cancelación llega al motor y registra «Instalación cancelada. No se ha publicado nada.» | F6 |
| `test_periodic_network_check_turns_button_red_without_prompting` | Poll de red: botón rojo «Internet no accesible» sin diagnóstico ni diálogo; vuelve a verde | F9 |
| `test_network_status_and_cancel_dialog` | Botón de estado: ProxyDialog con portal; Esc no autentica; enviar credenciales llama `authenticate` | F9 |
| `test_unverified_download_popup_cancel_and_confirm` | Descargas sin hash: Esc no descarga; confirmar descarga e instala | F6 |
| `test_log_errors_are_red_and_copy_keeps_plain_text` | Error en rojo y negrita; Copiar log envía texto plano por stdin | F6 |
| `test_export_import_profile_through_dialogs` | Exportar a JSON; importar con selección y nombre nuevo; revisión y confirmación | F11 |
| `test_compact_sidebar_version_and_create_profile_dialog` | Título con versión; orden exacto de botones de la barra; Abrir Home; crear profile desde el diálogo | F6 (orden incluye F9/F10/F12 botones) |
| `test_catalog_filter_and_selection` | Filtro, selección, «Instalado» tras instalar, «Disponible» al quitar el marcador, sin resultados | F6 |
| `test_home_checkboxes_toggle_distinct_runtimes_without_uninstalling` | Activar varias versiones de Java sin desinstalar; desactivar; cambiar profile desmarca todo | F6 |
| `test_application_buttons_only_visible_for_active_launchable_app` | Abrir y Acc. directo solo si está activo y es lanzable; lanzar con argumentos; generar acceso con color azul | F6 (Abrir) / F8 (Acc. directo) |
| `test_mito_shortcut_and_delete_selected_profile` | Acceso a MITO con etiqueta por defecto y color naranja; eliminar profile con confirmación; no eliminar el último | F8 (acceso) / F6 (eliminar) |
| `test_bucket_filter_and_scrollable_notes` | Filtro de bucket persiste en `catalog-filters.json`; notas visibles en el detalle | F6 |
| `test_component_removal_requires_confirmation` | Eliminar pide confirmación; cancelar no llama al motor | F6 |
| `test_update_button_only_for_newer_version` | Botón «Actualizar» solo si hay versión más nueva; deshabilitado si el manifiesto nuevo no es Compatible; desaparece si ya no hay | F6 |
| `test_info_dialog_shows_binaries_and_persistence` | Info: rutas base y persistencia, hook `installer.script`, Abrir según existencia | F6 (hook F7) |

Tests relacionados fuera de `test_ui.py` (ver `ui_dialogs.md`): `test_i18n.py` (`LanguageUITests`,
`test_native_labels_are_escaped_as_data`) y `test_mcp_ui.py` (`test_jobs_monitor_is_optional_and_separate`).

## Dependencias

Módulos del original usados por `ui.py` (en orden de aparición): `i18n` (`tr`, `Translator`), `localized_ui`
(`ModalScreen`), `storage` (`mutation`, `read_json`, `write_json`, `YitoError`), `search` (`rank_entries`),
`dialog_header`, `error_tracking` (`capture`), `diagnostics`, `engine` (`Engine`, `LongStep`), `operation`
(`Operation`, `monitored`, `Cancelled`), `versions`, `components`, `catalog`, `persistence`, `isolation`,
`configuration`, `configuration_ui`, `profile_management_ui`, `profile_transfer_ui`, `profile_files_ui`,
`profile_transfer`, `host_apps`, `host_apps_ui`, `host_data`, `host_exposure`, `open_with`, `open_with_ui`,
`protocols`, `protocols_ui`, `shortcuts`, `icons`, `terminal` (`prepare`), `terminal_identity`, `relocation`,
`network`, `network_ui`, `self_update`, `self_update_ui`, `update_checks`, `mcp_service`, `mcp_ui`,
`pocketools_ui`, `transfer` (`DownloadIntegrityError`), `arguments` (`split`), `bucket_resources`.

## Notas para Rust

- Crate: `mito-tui` (depende de `mito-i18n` y `mito-terminal`). Framework: `ratatui` + `crossterm`.
  El modelo de pantallas (`ModalScreen`) es una pila de vistas; los diálogos devuelven un valor
  mediante un canal (equivalente a `dismiss`).
- Los workers de hilo se sustituyen por `std::thread` o `tokio` con un canal de eventos hacia el bucle de
  la UI (equivale a `call_from_thread`). Ningún hilo toca widgets.
- `Operation` es un estado compartido (`Arc<Mutex<State>>` + `AtomicBool` de cancelación) con las mismas
  claves: `title`, `text`, `received`, `total`, `unit`, `note`, `cancellable`, `active`, `started`.
- Las cadenas de la tabla son claves: deben pasar por el catálogo i18n de `mito-i18n` con la misma
  búsqueda (idioma → inglés → texto fuente) y la interpolación posterior a la búsqueda.
- Los colores de la sección «Estilo» se reproducen como `Color::Rgb`; el borde redondeado, con
  `BorderType::Rounded`.
- Las etiquetas literales (sin traducir) deben conservarse como están, para mantener paridad.

## Dudas y ambigüedades

1. **`link-<n>` con `entries` vacío.** `ShortcutDialog.confirm` devuelve 4 valores si no hay entradas, pero
   el manejador de `link-` desempaqueta 5 (`badge, color, coupled, arguments, shortcut`) → `ValueError`.
   Sin entradas el botón puede aparecer si `can_open` es verdadero. Hay que decidir si el port exige al menos una entrada.
2. **`open-<n>` sin entradas.** `OpenApplicationDialog` devuelve 2 valores sin entradas y el manejador lee
   `result[2]` → `IndexError`. Mismo caso que el anterior.
3. **Etiquetas no traducidas.** «detached», «attached» (selector de proceso), «MCP - enabled/disabled»,
   «MCP · Jobs», «path», «Offline» y los nombres de color capitalizados son literales en el original. Confirmar si el port los traduce o los conserva.
4. **Valor inválido de `shortcut.badgeText` / `shortcut.badgeColor`** en `config.properties`: `ShortcutDialog.__init__`
   lanza `YitoError` sin capturar. Confirmar si el port debe mostrar error o usar el valor por defecto.
5. **`install_from_catalog` retorna en silencio** cuando `refresh_keys([target])` no devuelve `1`. Confirmar si
   el motor ya emite el motivo en el log.
6. **Escape en diálogos no cancelables** (`LongStepDialog`, `OperationDialog`) cae al binding de la aplicación
   («Inicio»), que cambia a la página de inicio. Confirmar si el port debe bloquearlo.
7. **Guarda de profile para `expose-<n>`**: el botón «Host · ✓/✗» no exige profile; `begin('host-expose', None, …)`
   puede ejecutarse sin profile. Confirmar el comportamiento esperado.
8. **`VersionsDialog`** recibe `history_available` pero no lo usa; el `loader` del lambda usa los nombres
   `current, wanted` aunque recibe `(found, wanted)`. Confirmar la firma real de `versions.load_versions`.
9. **Comprobación de hooks en la info**: `ComponentInfoDialog` mira `installer` de primer nivel y el detalle del
   catálogo mira primero `architecture.64bit`. Confirmar que es intencionado.
10. **`posthog.enabled`, `posthog.host`, `posthog.api_key`** aparecen en `PROPERTIES` (configuración) y chocan con
    la regla de producto «Sin telemetría remota». Confirmar si son residuales y deben omitirse en el port (ver `ui_dialogs.md`).
11. **Fase de `DownloadIntegrityDialog`**: el texto menciona «pulsa INTERNET» (F9); la fase del diálogo se marca F6 porque lo dispara la instalación.
12. **Fase del botón «Abrir»** de componentes: se marca F6 para el modo independiente y F8 para el acoplado;
    el diálogo es el mismo. Confirmar que se aceptan dos fases.
13. **Escritura de `catalog-filters.json`** con `sorted(disabled)`: el original escribe una lista JSON, no un
    objeto. Confirmar el formato de disco en `storage.md`.
