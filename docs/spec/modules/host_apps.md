# host_apps.py + host_apps_ui.py → mito_win::host_apps (lógica) y mito_tui::host_apps (diálogos)

Origen:
- `core/app/mito/host_apps.py` (194 líneas): inventario, descubrimiento, lanzamiento, accesos.
- `core/app/mito/host_apps_ui.py` (190 líneas): cuatro diálogos Textual (`HostInfoDialog`, `RemoveHostDialog`, `HostDiscoverDialog`, `HostDataConfirm`).
- `core/app/mito/native_ui.py` (15 líneas): `localize_script`, usado por `choose_file`. Los diálogos Textual generales están en `ui_dialogs.md`.
- Tests: `core/tests/test_host_apps.py` (190 líneas, 9 tests), `core/tests/test_host_data_ui.py` (120 líneas, 2 tests; afecta a `HostDataConfirm` y a los botones de fila). Las pruebas de `host_data.py` van en su contrato propio.

## Responsabilidad

Registrar ejecutables externos del host Windows (`.exe`, `.bat`, `.cmd`, `.ps1`) como referencias en MITO, sin copiarlos. Lanzarlos con el entorno del profile MITO seleccionado (`engine.environment(profile)`), en el workspace del profile. Descubrir candidatos en los accesos del menú Inicio y del escritorio del host. Crear accesos de escritorio que invocan `mito host-run`. Los datos del host (junctions de Data/Cache) se gestionan en `host_data.py`, no aquí.

## API pública del original (`host_apps.py`)

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `inventory(root)` | raíz | `dict[identity, ruta]` desde `core/state/host-applications.json` (vacío si no existe) | — |
| `add(root, filename)` | ruta absoluta | `identity` (16 hex) | `YitoError('Selecciona un archivo .exe, .bat, .cmd o .ps1 existente')` |
| `shortcut_folders()` | — | lista de `Path` (CSIDL 2, 23, 16, 25) | — |
| `uninstaller(path)` | ruta | `bool` | — |
| `discover(root, folders=None, resolve=None)` | raíz; `folders` y `resolve` para pruebas | `[(etiqueta, ejecutable)]` ordenado por etiqueta (casefold) | `OSError`/`ValueError`/`SubprocessError` capturados por la UI |
| `target(root, identity)` | identidad | `Path` | `YitoError('La aplicación del host ya no está registrada en MITO')` |
| `remove(root, identity)` | identidad | `None` | — (desenlaza datos antes de quitar la referencia) |
| `launch(engine, profile, identity, arguments=(), coupled=False, owned=False)` | — | `Popen` (independiente), `int` (owned/coupled) | ver «Lanzamiento» |
| `choose_file()` | — | `str` (JSON decodificado) o `None` | `CalledProcessError` si PowerShell falla |
| `shortcut(engine, profile, identity, badge=None, coupled=False, arguments=(), color=None)` | — | `Path` del `.lnk` | `YitoError` de `badge_text`, `badge_color`, `target` |

Constantes: `EXTENSIONS = {'.exe', '.bat', '.cmd', '.ps1'}`; `SHORTCUT_FOLDERS = (2, 23, 16, 25)`.

Identidad: `identity = sha256(str(path).casefold().encode()).hexdigest()[:16]`, con `path` ya `resolve()`. Ejemplo: `C:\Apps\Editor\editor.exe` → `2c01437f9a3ead99`.

## Formatos de disco

### `core/state/host-applications.json`

Objeto JSON que mapea identidad a ruta absoluta. Se escribe con `write_json` (UTF-8, `indent=2`, `ensure_ascii=False`) dentro de `mutation(root)`.

```json
{
  "2c01437f9a3ead99": "C:\\Apps\\Editor\\editor.exe"
}
```

- `add` es idempotente: la misma ruta da la misma identidad y reescribe el mismo valor.
- `remove` quita solo la clave. El ejecutable y los datos del host se conservan.

### Registros de accesos del host: `core/state/host-shortcuts/<tag>.json`

`tag = sha256(f"{profile}/{identity}")[:16]`. Contenido:

```json
{
  "path": "C:\\Users\\ana\\Desktop\\MITO - test - Host - editor.lnk",
  "badge": "DEV",
  "color": "naranja",
  "profile": "test",
  "application": "2c01437f9a3ead99",
  "coupled": false,
  "arguments": []
}
```

Icono generado: `core/state/icons/host-<tag>-<sha256(badge)[:8]>-mito-<digest>.ico` (ver `icons.md`).

### Accesos de escritorio

- Nombre: `MITO - {profile} - Host - {path.stem}.lnk`, con sufijo ` (2)`, ` (3)`… si colisiona con un `.lnk` ajeno.
- Destino: `pythonw.exe -B -I -X utf8 -m mito host-run <identity> --profile <profile> [--coupled] [-- args…]`.
- `WorkingDirectory` = carpeta del ejecutable del host (`path.parent`), no la raíz de MITO.
- Icono: el propio ejecutable del host (`application_icon(…, path, …)`), extraído con `icons.py`.

## Descubrimiento (`discover`)

1. Carpetas: `shortcut_folders()` lee con `SHGetFolderPathW` los CSIDL 2 (menú Inicio del usuario), 23 (menú Inicio común), 16 (escritorio del usuario) y 25 (escritorio común). No usa `APPDATA` ni `USERPROFILE`, porque dentro de un profile esas variables apuntan al home de MITO.
2. Recorre cada carpeta existente con `rglob('*.lnk')` (solo archivos). Si no hay ninguno, devuelve `[]` sin llamar a `resolve`.
3. Resuelve los destinos de todos los `.lnk` en **una sola llamada** a `resolve(links)` (por defecto `relocation.link_targets`, que pasa las rutas por stdin, nunca como código).
4. Orden de procesado: `link.stem.casefold()`. Para cada `.lnk` con destino `value` (si no hay destino, se salta):
   - se descarta si `key` (casefold del destino) ya está en `found` (dedupe: gana el primer `.lnk` en el orden de procesado);
   - se descarta si está en el inventario (`known`);
   - se descarta si empieza por `mito + '\\'` (la raíz de MITO resuelta, casefold);
   - se descarta si la extensión no está en `EXTENSIONS`;
   - se descarta si `uninstaller(path)` o `uninstaller(link)`;
   - se descarta si el destino no es archivo.
5. Resultado: `(link.stem, str(path))` por cada destino aceptado, ordenado por etiqueta (casefold).

`uninstaller(path)`: el nombre en minúsculas (`stem.casefold()`) empieza por `unins`, o contiene `uninstall` o `desinstal`.

## Lanzamiento (`launch`)

1. `path = target(root, identity)`; si el archivo no existe o la extensión no está permitida: `YitoError('El archivo de la aplicación del host ya no está disponible')`.
2. `env = engine.environment(profile)`. `system = Path(valor de SYSTEMROOT en env, sin distinguir mayúsculas)/'System32'`. Nota: `next(...)` sin valor por defecto lanza `StopIteration` si no hay `SYSTEMROOT` (ver dudas).
3. `workspace = inside(root/'workspaces', engine.profile(profile)['workspace'])`; se crea con `mkdir(parents=True, exist_ok=True)`.
4. **Modo acoplado** (`coupled=True`):
   - si `owned` también: `YitoError('Modo de lanzamiento no válido')`;
   - `terminal.launch_host(engine, profile, identity, path.stem, arguments)`; devuelve `0`.
   - La pestaña ejecuta `host-run <identity> --profile <p> --owned [-- args]`.
5. Si no es acoplado, prepara el comando:
   - `.bat`/`.cmd`: `command = jobs.batch_command(path, arguments, env)`, que devuelve una **cadena** (línea de comandos cruda de `cmd.exe /d /v:off /s /c "…"`). `flags = CREATE_NEW_CONSOLE`. En Windows, `Popen` con cadena la usa como línea de comandos.
   - `.ps1`: `[system\WindowsPowerShell\v1.0\powershell.exe, -NoProfile, -ExecutionPolicy, Bypass, -File, path, *arguments]`, `flags = CREATE_NEW_CONSOLE`.
   - otra extensión (`.exe`): `[path, *arguments]`, `flags = 0`.
6. **Modo owned** (`owned=True`): `group = ProcessJob()`; `process = group.spawn(command, cwd=workspace, env=env, creationflags=0)`; `result = process.wait()`; `group.wait_empty()`; devuelve `result`; `finally: group.close()`.
7. **Modo independiente**: `return subprocess.Popen(command, cwd=workspace, env=env, creationflags=flags)`. El `Popen` no se espera.

`host-run` (CLI) convierte ese resultado: si es `owned` devuelve el código; si no, devuelve `0`.

Invariantes de entorno: `env` es el de `engine.environment(profile)` (test: `expected = engine.environment('one')`, igual al entorno pasado). Para `.bat`, `batch_command` añade `MITO_COMMAND_ARG_<n>` a `env`.

## Accesos del host (`shortcut`)

1. `with mutation(engine.root)`; `profile = identifier(profile)`; `engine.profile(profile)` (debe existir).
2. `path = target(...)`; si no es archivo: `YitoError('El archivo de la aplicación del host ya no está disponible')`.
3. `badge = badge_text(engine, badge)`; `color = badge_color(engine, color)`.
4. `tag = sha256(f"{profile}/{identity}")[:16]`; `icon = application_icon(engine, f"host-{tag}-{sha256(badge)[:8]}", badge, path, color=color)`.
5. `desktop = desktop_path()`; `mkdir`. `registry = core/state/host-shortcuts/<tag>.json`; `previous = read_json(registry)` si existe.
6. `stem = f"MITO - {profile} - Host - {path.stem}"`; `link = desktop/f"{stem}.lnk"`, con numeración si colisiona (salvo que sea `previous['path']`).
7. `argv = [pythonw.exe, -B, -I, -X, utf8, -m, mito, host-run, identity, --profile, profile]`; `--coupled` si aplica; `-- args` si hay argumentos.
8. `write_link(link, argv, path.parent, icon)`; `write_json(registry, {path, badge, color, profile, application, coupled, arguments})`; devuelve `link`.

## Eliminación (`remove`)

1. `mutation(root)`.
2. Para cada `kind` en `host_data.KINDS`: `unlink_locked(root, identity, kind)` (quita los junctions de Data/Cache). Ver `host_data.md`.
3. Quita la clave de `host-applications.json` y escribe.

No borra `core/state/host-shortcuts/<tag>.json` ni el `.lnk` del escritorio (ver dudas).

## Selector de archivo nativo (`choose_file`)

Lanza `powershell.exe -NoLogo -NoProfile -STA -Command <script>` con `encoding='utf-8'`, `check=True`, `capture_output=True`, `CREATE_NO_WINDOW`. Antes de ejecutar, `localize_script(script, 'MITO - Agregar aplicación del host', 'Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)|*.exe;*.bat;*.cmd;*.ps1')`.

`localize_script` (`native_ui.py`) reemplaza solo los literales PowerShell **completos**: cada etiqueta se busca como `'<texto>'` y se sustituye por `'<tr(texto)>'`, con las comillas simples duplicadas (`''`). Así una traducción con comillas sigue siendo un literal válido.

El script, literal (las dos cadenas que se localizan están marcadas con `[L]`):

```powershell
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'MITO - Agregar aplicación del host'                                  # [L] título
$dialog.Filter = 'Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)|*.exe;*.bat;*.cmd;*.ps1'   # [L] filtro
$dialog.InitialDirectory = [Environment]::GetFolderPath('UserProfile')
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.FileName -Compress
    }
} finally { $dialog.Dispose() }
```

Nota: los comentarios `# [L]` son solo marca de este documento; el script real no los tiene.

Salida: `json.loads(stdout)` (una cadena con la ruta) o `None` si stdout está vacío (cancelado). La ruta que devuelve el diálogo siempre es absoluta.

Con el idioma inglés, el título visible es `MITO - Add host application` (ver `test_i18n.py:193-201`).

## Lanzamiento del host y `mito host-run`

`__main__.py:192-200`:
```
intercept(engine.root, engine.environment(profile))   # red: proxy
result = host_apps.launch(engine, profile, identity, arguments, coupled=..., owned=...)
if owned: pause_owned_exit(result); return result
return 0
```

Detalles de la CLI en `cli_f8.md`.

## Diálogos (`host_apps_ui.py`)

Todas las clases heredan de `localized_ui.ModalScreen`. Los `BINDINGS` (`escape` → `Cerrar`/`Cancelar`) se traducen **por instancia** con `translate_bindings`, no con `tr` directo. Los textos de cada widget pasan por `tr`, salvo que se indique.

### `HostInfoDialog(path)` — `ModalScreen`

- Binding: `escape` → `Cerrar`.
- `DialogHeader(tr('APLICACIÓN DEL HOST'), 'host-info-close')`.
- Texto (`markup=False`): `tr('{p0}\n{p2}\nSe ejecuta con el entorno del profile de MITO seleccionado, incluido APPDATA.', p0=nombre, p2=ruta)`.
- Contenedor `#host-location`: texto (`markup=False`) `tr('Carpeta del ejecutable\n{p1}', p1=carpeta)`; botón `tr('Abrir')` con id `host-folder-open`, deshabilitado si la carpeta no existe.
- Si el archivo no existe: `Static(tr('El archivo ya no está disponible.'))`.
- Acciones: `host-info-close` cierra; `host-folder-open` llama `persistence.open_location(carpeta)`. Si lanza `OSError` o `YitoError`: `notify(str(exc), severity='error')`.
- Estilo: `#host-info`, ancho 90 %, máximo 110, borde redondeado `#b5dc64`, fondo `#153429`.

### `RemoveHostDialog(path)` — `ModalScreen[bool]`

- Binding: `escape` → `Cancelar` (cancela, `dismiss(False)`).
- Texto: `tr('¿Quitar {p1} de MITO?', p1=nombre)` (`markup=False`).
- Texto: `tr('Se eliminan la referencia y sus enlaces de caché/datos. El ejecutable del host y sus datos se conservan.')`. **Aquí no hay `markup=False`**, a diferencia de los demás diálogos.
- Botones: `tr('Cancelar')` id `host-remove-cancel` (con foco al montar); `tr('Eliminar')` id `host-remove-confirm`, variante `error`.
- Resultado: `dismiss(id == 'host-remove-confirm')`.
- Estilo: `#remove-host`, ancho 70.

### `HostDiscoverDialog(found)` — `ModalScreen[object]`

- Binding: `escape` → `Cancelar`.
- Cabecera: `DialogHeader(tr('AGREGAR APLICACIÓN DEL HOST'), 'host-discover-close')`.
- Texto (`markup=False`): `tr('Aplicaciones encontradas en el menú Inicio y el escritorio del host. Si falta alguna, usa Examinar.')`.
- `Input` id `host-discover-filter`, placeholder `tr('Filtrar…')`, con foco al montar.
- `OptionList` id `host-discover-list` (alto 16).
- Botones: `tr('Examinar…')` id `host-discover-browse`; `tr('Cancelar')` id `host-discover-cancel`; `tr('Agregar')` id `host-discover-add`, variante `success`.
- Lista vacía: opción `tr('No se ha encontrado ninguna aplicación.')`; `host-discover-add` deshabilitado.
- Texto de cada opción: `f'{etiqueta}  ·  {ruta}'` (dos espacios, `·` U+00B7, dos espacios). No pasa por `tr`.
- Filtro: palabras en casefold (`split()`); una opción aparece si todas las palabras están contenidas en `f'{etiqueta} {ruta}'.casefold()`.
- `Input.Changed` refiltra; `Input.Submitted` y `OptionList.OptionSelected` devuelven la ruta seleccionada.
- Resultado: la ruta (`str`), `'browse'` para `host-discover-browse`, o `None` (cancelar, cerrar, Escape).

### `HostDataConfirm(message, label=None)` — `ModalScreen[bool]`

- Binding: `escape` → `Cancelar` (`dismiss(False)`).
- Texto: `Static(message, markup=False)`. El mensaje lo construye `ui.py` con `tr` (`ui.py:1684-1800`), ver abajo.
- Botones: `tr('Cancelar')` id `host-data-cancel` (con foco al montar); `label` id `host-data-accept`, variante `warning`. Si `label` es `None`, la etiqueta es `tr('Enlazar')`.
- Resultado: `dismiss(id == 'host-data-accept')`.

### Mensajes de `ui.py` que usan estos diálogos

- Botón `#host-add` en la lista de componentes: `tr('Agregar aplicación del host')`.
- Fila de cada host (`ui.py:1139-1154`): nombre `f'{path.stem} · Host'`, con `tr(' · No disponible')` si falta el archivo; botones `host-open-<id>` (`tr('Abrir')`), `host-cache-<id>` (`tr('Cache · ')` + ✓/✗), `host-data-<id>` (`tr('Data · ')` + ✓/✗), `host-link-<id>` (`tr('Acc. directo')`), `host-info-<id>` (`tr('Info')`), `host-remove-<id>` (`tr('Eliminar')`).
- Registro (log) de `host-add` (`ui.py:1725-1745`): `tr('Buscando aplicaciones en el menú Inicio y el escritorio del host…')`, `tr('No se pudieron revisar los accesos del host: {p1}')`, `tr('Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado.')`, `tr('Buscando carpetas del host: {p1}…')`.
- Otros mensajes: `tr('Selecciona un profile')` (aviso si falta profile para «Abrir» o «Acc. directo»); `tr('Referencia eliminada. El ejecutable y los datos del host se conservan.')`; `tr('Acceso directo al host creado: {p1}')`.
- Diálogo de `host-open`: `OpenApplicationDialog` de `ui.py:192-247`, no de `host_apps_ui`.
- Diálogo de `host-link`: `ShortcutDialog(application=True)` de `ui.py:249-350`, ver `shortcuts.md`.

### Flujo de `host-add` con `HostDataConfirm`

Tras `add`, para cada `kind` en (`data`, `cache`) sin entradas, `offer_host_data` pregunta con `HostDataConfirm(message)`. El mensaje, `tr`, ui.py:1689: `'{p0}: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nCompartido por todos los profiles. La aplicación podrá modificar los datos del host. Cierra la aplicación antes de enlazar.'`. Etiqueta por defecto `Enlazar`; las etiquetas de `Host data`/`Host cache` se calculan con `tr`.

## Invariantes

1. `add` solo acepta rutas absolutas existentes con extensión en `EXTENSIONS`. Se resuelven (`resolve()`) antes de calcular la identidad.
2. La identidad depende de la ruta en minúsculas (casefold): dos rutas que solo difieren en mayúsculas son la misma aplicación.
3. `remove` nunca borra el ejecutable ni los datos del host. Solo la referencia y los junctions de MITO.
4. Un host registrado usa el entorno del profile seleccionado en cada lanzamiento, nunca el del sistema (test: `APPDATA`/`LOCALAPPDATA` apuntan a `root/home/AppData`).
5. El lanzamiento en workspace del profile; `cwd` nunca es la carpeta del ejecutable.
6. `discover` no devuelve aplicaciones dentro de MITO, aplicaciones ya registradas, desinstaladores ni destinos que no sean `.exe`/`.bat`/`.cmd`/`.ps1`.
7. `shortcut` valida la existencia del archivo y el nombre del profile antes de tocar el escritorio.
8. Un acceso del host nunca sobrescribe un `.lnk` ajeno (colisión numerada).

## Casos límite y errores

- `add` con `.txt`: `Selecciona un archivo .exe, .bat, .cmd o .ps1 existente`.
- Identidad sin entrada en el inventario (quitada desde otra instancia): `La aplicación del host ya no está registrada en MITO`.
- Archivo borrado después de registrarse: `El archivo de la aplicación del host ya no está disponible` (en `launch` y `shortcut`).
- `launch` con `coupled` y `owned`: `Modo de lanzamiento no válido`.
- `launch` con `SYSTEMROOT` ausente en el entorno del profile: `StopIteration` no capturado (ver dudas).
- `choose_file` cancelado: stdout vacío → `None`.
- `choose_file` con error de PowerShell: `CalledProcessError`. La llamada en `ui.py:1736` no está dentro de `try`, así que el error no lo captura la UI en ese punto.
- `discover` con un `.lnk` cuyo destino no se puede resolver: `targets.get(...)` devuelve `''`, se salta.
- `discover` sin `.lnk` en las carpetas: `[]` sin llamar a `resolve`.
- `remove` de un host con junctions activos: se quitan antes que la referencia.
- Escapar comillas o saltos de línea en `.bat`/`.cmd` (argumentos o ruta): `INVALID_ARGUMENT: BAT/CMD argv cannot contain quotes or newlines; use an explicit script`.
- `HostInfoDialog` con carpeta inexistente: botón `host-folder-open` deshabilitado.

## Casos de test del original

### `test_host_apps.py`

| Test | Líneas | Qué comprueba |
|---|---|---|
| `test_registration_deduplicates_and_removal_only_removes_reference` | 23-36 | Registrar dos veces da la misma identidad y una sola entrada; `remove` no borra el archivo; `launch` tras quitarlo da error; un `.txt` se rechaza. |
| `test_external_application_uses_selected_profile_environment` | 38-50 | El entorno pasado a `Popen` tiene `MITO_PROFILE`, la variable propia del profile, `APPDATA`, `LOCALAPPDATA` y el shim del profile en `PATH`. |
| `test_exe_launch_uses_profile_workspace_and_environment` | 52-61 | `Popen` recibe `[ruta]`, `cwd` = `workspaces/one` y `env` = `engine.environment('one')`. |
| `test_batch_paths_with_metacharacters_and_powershell_execute` | 63-82 | Ejecución real de `.cmd`/`.bat` con `&`, `!`, `%` y `^` en el nombre; `APPDATA` correcto; `.ps1` con comillas escribe `result.json` con `AppData`, `LocalAppData` y `Profile`. |
| `test_shortcut_pins_profile_and_badge` | 84-98 | `host-run <identity> --profile one` en argv; colisión con un `.lnk` ajeno se respeta; el icono usa `('TEST', application)`. |
| `test_coupled_launch_and_shortcut_use_terminal_mode` | 100-115 | `launch(coupled=True)` llama a `terminal.launch_host(engine, 'one', identity, 'Host App', args)` y devuelve 0. El acceso tiene `--coupled`, argumentos tras `--`, y el registro guarda `arguments`. |
| `HostDiscoveryTests.test_discover_filters_and_deduplicates` | 144-158 | Dedupe (`Tools/Editor (copia)` no repite el destino); se descartan desinstaladores, PDF, destino inexistente, dentro de MITO y ya registrado. `Tools/Run.cmd` sí aparece. Resultado `[('Editor', …), ('Run', …)]`. |
| `HostDiscoveryTests.test_discover_without_shortcuts_does_not_resolve` | 160-163 | Sin `.lnk`, no se llama a `resolve`. |
| `HostDiscoverDialogTests.test_filter_add_and_browse` | 167-190 | Filtro `view` deja un resultado; `Agregar` devuelve la ruta; `Examinar` devuelve `'browse'`. |

### `test_host_data_ui.py` (afecta a `HostDataConfirm` y a los botones de fila)

| Test | Líneas | Qué comprueba |
|---|---|---|
| `test_add_offers_roaming_then_local_and_accepts_independently` | 16-65 | Tras `#host-add`, aparece `HostDataConfirm` con `Host data:`; aceptar y luego `Host cache:`; escape deja solo `data` enlazado; los seis botones de fila están a la misma altura y dentro del ancho. |
| `test_manual_link_cancel_replace_unlink_and_buttons_remain` | 67-120 | Botón `#host-data-<id>` con ✗; cancelar no enlaza; aceptar enlaza (✓); tras olvidar metadatos, la etiqueta sigue `Data · ✓`; desenlazar pide confirmación y deja la carpeta del host intacta. |

## Dependencias

- `i18n.tr`; `storage`: `YitoError`, `inside`, `identifier`, `mutation`, `read_json`, `write_json`.
- `engine`: `root`, `environment`, `profile`, `selected`.
- `jobs.batch_command` (ver `processes.md` y `launch.rs`); `processes.ProcessJob`.
- `terminal.launch_host` (acoplado); `network.intercept` (`__main__`, no aquí).
- `host_data`: `KINDS`, `unlink_locked`, `candidates`, `entries`, `plan`, `link`, `choose_folder`.
- `relocation.link_targets` (resolución de `.lnk`).
- `shortcuts`: `application_icon`, `badge_text`, `badge_color`, `desktop_path`, `write_link`.
- `native_ui.localize_script`; `persistence.open_location`; `dialog_header.DialogHeader`; `localized_ui.ModalScreen`.
- Llamadores: `ui.py:1139-1154`, `ui.py:1330-1370`, `ui.py:1676-1682`, `ui.py:1725-1806`, `__main__.py:192-200`, `test_i18n.py:193-201`.

## Notas para Rust

- PARITY asigna `host_apps.py` a `mito-win` y `host_apps_ui.py` a `mito-tui`. Propuesta: la lógica (inventario, descubrimiento, accesos) en `mito_win::host_apps`; el lanzamiento con entorno en `mito_engine::launch` (ya tiene `LaunchCommand` y `batch_command`); los diálogos en `mito_tui::host_apps`.
- `discover`: los CSIDL se obtienen con `SHGetFolderPathW` (o `SHGetKnownFolderPath`, más moderno, con los GUID `FOLDERID_Programs`, `FOLDERID_CommonPrograms`, `FOLDERID_Desktop`, `FOLDERID_PublicDesktop`). Los destinos de `.lnk` se leen con `IShellLinkW::GetPath`, sin proceso externo, en lugar de `relocation.link_targets`. Es un cambio de implementación; documentarlo en ADR.
- `host-applications.json` con `serde_json` (preserve_order) y claves en el orden de inserción.
- `identity`: `sha256` del texto en minúsculas. Usar `to_lowercase()`, no `casefold` completo, o documentar la diferencia (en Windows son equivalentes para rutas ASCII).
- `localize_script` en Rust: reemplazo literal de cadenas `'…'` completas, igual que Python (las comillas simples se duplican antes de buscar).
- `choose_file` puede usar el diálogo Win32 `IFileOpenDialog` en lugar de PowerShell. Mismo filtro y mismo título, pero documentar que el script ya no se ejecuta.
- Diálogos Textual → `ratatui` en `mito-tui`. Mantener los textos, los ids (`host-discover-filter`, `host-data-accept`…) como claves de acción y el orden de botones.
- `CREATE_NEW_CONSOLE` (0x10) para `.bat`/`.cmd`/`.ps1` en el lanzamiento independiente. Lo usa `mito-engine` solo con `new_terminal`; añadir la regla para hosts.

## Dudas

1. `launch` usa `next(...)` sobre las variables de entorno para `SYSTEMROOT`, sin valor por defecto. Si el entorno del profile no tiene `SYSTEMROOT`, lanza `StopIteration` (no es `YitoError`). ¿Debe ser un error claro?
2. `launch` independiente devuelve un `Popen`, pero `__main__` y la UI lo ignoran y devuelven 0. El tipo de retorno es inconsistente. ¿Debe devolver `int` (0) en Rust?
3. `remove` no borra los `host-shortcuts/<tag>.json` ni los `.lnk` del escritorio. Esos accesos siguen existiendo y ejecutan `host-run <identity>`, que falla con «ya no está registrada». ¿Es intencional?
4. `RemoveHostDialog` muestra el segundo texto sin `markup=False`. Si el texto contiene corchetes, Textual intenta interpretarlo. ¿Es un descuido?
5. `choose_file` ejecuta el script con `-Command`, sin el mecanismo de entrada que usa `host_data.choose_folder`. ¿Es intencional?
6. `discover` solo usa `.lnk` de cuatro carpetas. No incluye `Start Menu\Programs\Startup` ni accesos de la barra de tareas (`Quick Launch`). ¿Es intencional?
7. El `.ps1` del host usa `SYSTEMROOT\System32\WindowsPowerShell\v1.0\powershell.exe`, mientras que `run` de un componente usa `powershell.exe` del PATH del profile. ¿Debe unificarse?
8. `identity` se calcula con `casefold()`: en rutas con caracteres no ASCII, `casefold` y `lower` pueden dar resultados distintos. Confirmar la intención antes de fijar el algoritmo en Rust.
