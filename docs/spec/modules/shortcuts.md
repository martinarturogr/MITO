# shortcuts.py → mito_win::shortcuts

Origen: `core/app/mito/shortcuts.py` (233 líneas), `core/tests/test_shortcuts.py` (261 líneas, 16 tests). El icono con insignia está en `icons.md`; los diálogos y el `choose_file` de accesos de host en `host_apps.md`.

## Responsabilidad

Crear, actualizar y migrar accesos directos `.lnk` en el escritorio de Windows para tres orígenes: aplicaciones de componentes (`application_link`), la propia terminal de MITO (`mito_link`) y ejecutables del host (`host_apps.shortcut`, que reutiliza `write_link`, `application_icon` y `desktop_path`). Cada acceso se escribe con PowerShell/`WScript.Shell` y se registra en `core/state/` para poder actualizarlo después.

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `badge_text(engine, value=None)` | `value` o, si es `None`, `shortcut.badgeText` de `config.properties` | `str` en mayúsculas, 1-4 caracteres | `YitoError('La etiqueta admite de 1 a 4 letras o números')` |
| `badge_color(engine, value=None)` | igual con `shortcut.badgeColor` | clave de `BADGE_COLORS` en minúsculas | `YitoError('Elige un color de la paleta del icono')` |
| `application_icon(engine, name, badge=None, base=None, color=None)` | nombre del lanzador, icono base (por defecto `core/assets/branding/mito.ico`) | `Path` del `.ico` | errores de `icons.md`; si falla con `base` distinto del de MITO, reintenta con el icono de MITO |
| `literal(value)` | cualquier valor | `'…'` con `'` doblada | — |
| `component_icon_source(engine, lock, profile, shortcut=None)` | componente seleccionado | `Path` (icono declarado o ejecutable) | `YitoError` de `component_path`/`inside` si la ruta sale del payload |
| `write_link(path, argv, root, icon)` | `argv[0]` destino, `argv[1:]` argumentos | `None` | `subprocess.CalledProcessError` (no `YitoError`) |
| `retarget_link(path, old_targets, old_arguments, argv)` | `.lnk` existente | `bool` | `CalledProcessError`. Solo lo usa `pocketool_execution.py:115-132` |
| `desktop_path()` | — | `Path` del escritorio (Known Folder `FOLDERID_Desktop`) | `YitoError('No se pudo localizar el escritorio de Windows')` |
| `application_link(engine, profile, installation, badge=None, coupled=False, arguments=(), shortcut=None, color=None)` | `installation` = ID de instalación | `Path` del `.lnk` | ver «Casos límite» |
| `migrate_application_links(engine, old_id, new_id, lock, backups, log)` | `old_id`, `lock` nuevo; `backups` dict | `None` | `YitoError('Acceso registrado no válido')` |
| `mito_link(engine, profile, badge=None, color=None)` | profile | `Path` del `.lnk` | `YitoError` de `terminal.command` (`Falta Windows Terminal portable…`) |

Constantes de `shortcuts.py`: ninguna propia. Usa `icons.BADGE_COLORS` (lista de claves, en el orden de `icons.md`) y `icons.DEFAULT_BADGE_COLOR = 'naranja'`. Por defecto la etiqueta es `MITO`.

## Formatos de disco

### 1. Ubicación y nombres de los `.lnk`

- Carpeta: escritorio del usuario resuelto con `SHGetKnownFolderPath(FOLDERID_Desktop = {B4BFCC3A-DB2C-424C-B029-7FE99A87C641})`. Respeta OneDrive y redirecciones. Se crea con `mkdir(parents=True, exist_ok=True)`.
- Aplicación de componente: `{profile} - {app}.lnk`, donde `app` es la clave `bucket/app` con `/` sustituido por `-`. Ejemplo: `test - extras-app.lnk`.
- Con etiqueta de acceso declarado (`--shortcut`): `{profile} - {app} - {label}.lnk`. La etiqueta se sanea: cada carácter de `<>:"/\|?*` o de código < 32 pasa a `_`; después `strip(' .')` y recorte a 80 caracteres. Las etiquetas pueden traer subcarpetas del menú Inicio de Scoop, nunca rutas de escritorio.
- Terminal de MITO: `MITO - {profile}.lnk`.
- Host: `MITO - {profile} - Host - {nombre del exe sin extensión}.lnk` (lo genera `host_apps.shortcut`).
- Colisión: si el nombre existe y no es la ruta ya registrada, se añade ` (2)`, ` (3)`… Un `.lnk` ajeno nunca se sobrescribe.

### 2. Registros en `core/state/`

Se escriben con `write_json` de `storage` (UTF-8, `indent=2`, `ensure_ascii=False`, salto de línea final). El orden de claves es el de Python (inserción).

| Ruta | Clave del nombre | Contenido |
|---|---|---|
| `core/state/application-shortcuts/<tag>.json` | `tag = sha256(f"{profile}/{app}")[:16]`, o `sha256(f"{profile}/{app}/{shortcut}")[:16]` con etiqueta | `path, profile, app, badge, color, coupled, arguments, shortcut` |
| `core/state/mito-shortcuts/<profile>.json` | nombre del profile (`identifier`, minúsculas) | `path, profile, badge, color` |
| `core/state/host-shortcuts/<tag>.json` | `tag = sha256(f"{profile}/{identity}")[:16]` | `path, badge, color, profile, application, coupled, arguments` |

Ejemplo real de `core/state/application-shortcuts/25caa424deb27105.json` (`profile=test`, `app=extras/app`, sin etiqueta):

```json
{
  "path": "C:\\Users\\ana\\Desktop\\test - extras-app.lnk",
  "profile": "test",
  "app": "extras/app",
  "badge": "DEV",
  "color": "naranja",
  "coupled": false,
  "arguments": [],
  "shortcut": null
}
```

`25caa424deb27105` es `sha256("test/extras/app")[:16]`. Los registros de versiones antiguas usan la clave `installation` en lugar de `app`; `migrate_application_links` la lee como respaldo.

Registro de MITO (`core/state/mito-shortcuts/test.json`):

```json
{
  "path": "C:\\Users\\ana\\Desktop\\MITO - test.lnk",
  "profile": "test",
  "badge": "MITO",
  "color": "naranja"
}
```

### 3. Lanzador de icono y `.lnk`

- Identificador del lanzador (`launcher_id`, pasa por `identifier` y queda en minúsculas):
  - aplicación: `app-{tag}-{sha256(badge)[:8]}`. Con `badge=DEV`: `app-25caa424deb27105-4030fe15`;
  - terminal de MITO: `mito-{profile}`;
  - host: `host-{tag}-{sha256(badge)[:8]}`.
- Icono generado: `core/state/icons/<launcher_id>-mito-<digest>.ico` (ver `icons.md`).
- `.lnk`: Shell Link binario escrito por `WScript.Shell.CreateShortcut`. Campos que escribe MITO:
  - `TargetPath`: `argv[0]`;
  - `Arguments`: `subprocess.list2cmdline(argv[1:])`;
  - `WorkingDirectory`: `root` (la raíz de MITO para aplicaciones y terminal; la carpeta del ejecutable para el host);
  - `IconLocation`: ruta del `.ico`, sin índice (`,0`).
  - No escribe `Description`, `WindowStyle`, `Hotkey` ni `AppUserModelID`. Quedan en su valor por defecto.

`argv` de aplicación (`application_link`), con los argumentos fijos de MITO:

```
TargetPath   C:\…\MITO\core\tools\python\pythonw.exe
Arguments    -B -I -X utf8 -m mito run [--coupled] --profile test --app extras/app [--shortcut "Tool Help"] [-- run "two words"]
WorkingDirectory C:\…\MITO
IconLocation C:\…\MITO\core\state\icons\app-25caa424deb27105-4030fe15-mito-<16 hex>.ico
```

Argumentos se añaden con `--` tras el resto (`argv += ['--', *arguments]`), y `--coupled` va tras `run`, antes de `--profile`.

`argv` de terminal (`mito_link`): lo devuelve `terminal.command(engine, selected_profile=name)` y acaba en `… -m mito ui --profile <name>`. `TargetPath` es `YitoTerminal.exe` si existe, si no `WindowsTerminal.exe`.

`argv` de host (`host_apps.shortcut`, ver `host_apps.md`): `pythonw.exe -B -I -X utf8 -m mito host-run <identity> --profile <profile> [--coupled] [-- args]`, con `WorkingDirectory` = carpeta del ejecutable del host.

### 4. Script PowerShell de `write_link`

Las líneas se unen con `;\n` y se pasan con `powershell.exe -NoLogo -NoProfile -NonInteractive -Command`, `check=True`, `CREATE_NO_WINDOW`:

```powershell
$ws = New-Object -ComObject WScript.Shell
$lnk = $ws.CreateShortcut('<path>')
$lnk.TargetPath = '<argv[0]>'
$lnk.Arguments = '<list2cmdline(argv[1:])>'
$lnk.WorkingDirectory = '<root>'
$lnk.IconLocation = '<icon>'
$lnk.Save()
```

`literal(v)` = `'` + `v` con cada `'` sustituido por `''` + `'`. No escapa `$` ni comillas dobles, porque el literal es de comillas simples.

`retarget_link` añade `$ErrorActionPreference = 'Stop'` y la lógica siguiente, que solo cambia un `.lnk` generado por MITO:

1. Si `TargetPath` ya es `argv[0]` (sin distinguir mayúsculas) y `Arguments` coincide (distinguiendo mayúsculas): escribe `updated` y sale (recuperación de una ejecución anterior que guardó el `.lnk` pero no el registro).
2. Si `TargetPath` no está entre `old_targets` o `Arguments` no es `list2cmdline(old_arguments)`: sale sin escribir (el usuario lo personalizó). Devuelve `False`.
3. Si no: cambia `TargetPath` y `Arguments`, `Save()`, escribe `updated`. Icono, directorio de trabajo y estilo se conservan.

## Invariantes

1. Un valor explícito gana siempre a la configuración. Solo se lee `config.properties` (con BOM `utf-8-sig`) cuando el parámetro es `None`. Un valor vacío o solo espacios da el valor por defecto (`MITO`, `naranja`).
2. `badge_text`: `strip().upper()`; longitud 1-4; `isalnum()` (acepta letras Unicode, p. ej. `Ñ`). Espacios y símbolos se rechazan.
3. `badge_color`: `strip().lower()`; debe existir en `BADGE_COLORS`.
4. Un acceso solo se crea si el componente está activo en el profile (`installation in data['apps']`). La comprobación ocurre antes de tocar el escritorio (`desktop_path` no se llama).
5. Crear un profile, activar o desactivar un componente no publica accesos ni cambia iconos ya generados.
6. La identidad del acceso no depende de la versión: el tag usa `{profile}/{app}`, no el ID de instalación. Tras una actualización, `application_link` con el nuevo ID reutiliza el mismo `.lnk` y el mismo registro.
7. El comando del `.lnk` no contiene la versión: `mito run --app <key>` resuelve el componente seleccionado en cada lanzamiento.
8. `application_link` valida el objetivo (`engine.application_target`) antes de generar el icono o escribir archivos.
9. El icono es el declarado por el manifiesto (`shortcuts[n][3]`) si existe y es válido; si no, el ejecutable. Si el icono declarado produce `YitoError`, se usa el icono base de MITO.
10. `migrate_application_links` solo modifica registros cuyo `app` (o `installation`) coincide con `old_id`, cuyo `.lnk` existe, no es enlace simbólico y termina en `.lnk`. Conserva nombre, insignia y color. Los perfiles borrados se conservan sin tocar y se registra un aviso.
11. Un acceso no se sobrescribe si su nombre pertenece a otro archivo (colisión numerada).
12. Los argumentos del `.lnk` se escriben con `list2cmdline`. El lanzador usa `pythonw.exe` (sin consola) con `-B -I -X utf8`.

## Casos límite y errores

- `shortcut.badgeText` en `config.properties` con más de 4 caracteres o no alfanumérico: error en cada uso. La lectura exige `startswith('shortcut.badgeText=')` exacto, sin espacios delante.
- `shortcut.badgeColor` en mayúsculas (`Azul`) se normaliza a `azul`. Un color fuera de la paleta da error.
- `desktop_path` falla: `YitoError('No se pudo localizar el escritorio de Windows')`.
- Un `.lnk` registrado que ya no existe en disco se trata como no creado (`migrate` lo salta sin error).
- `component_icon_source` con icono fuera del payload (`C:/outside.ico`): `YitoError` de `inside`; no se captura.
- `application_link` con etiqueta inexistente o ambigua: `engine.application_target` lanza `Shortcut unavailable or ambiguous: {shortcut}` sin pasar por `tr`.
- Manifiesto sin ejecutable utilizable: `La app no declara un ejecutable disponible; úsala desde la terminal`.
- `migrate` con `path` que no termina en `.lnk` o que es enlace simbólico: `Acceso registrado no válido` (aborta la migración con los `backups` ya guardados).
- `migrate` con `arguments` que no es lista de cadenas: `Acceso registrado no válido`.
- Error de PowerShell en `write_link` o `retarget_link`: `CalledProcessError`. La CLI solo captura `YitoError`, `OSError` y `ValueError`, así que este error sale como excepción no controlada (ver dudas).
- Registro JSON corrupto: `ValueError` de `json` (no pasa por `tr`).
- `retarget_link` con un `.lnk` personalizado: devuelve `False` y no modifica el archivo.

## Casos de test del original

| Test (`test_shortcuts.py`) | Líneas | Qué comprueba |
|---|---|---|
| `test_retarget_link_preserves_icon_and_ignores_customized_links` | 37-65 | Solo Windows. `retarget_link` cambia destino y argumentos y conserva icono, directorio de trabajo y estilo. Repetirlo da `True`. Un `.lnk` con argumentos propios da `False` y sus bytes no cambian. |
| `test_creating_profile_does_not_publish_shortcuts` | 67-70 | Crear un profile no crea `.lnk` ni `core/state/shortcuts`. |
| `test_desktop_link_pins_profile_and_uses_badged_icon` | 72-83 | `--profile` y `--app` en el argv; nombre `test - extras-app.lnk`; el icono tiene tamaño 256. |
| `test_badge_color_is_saved_and_changes_generated_icon` | 85-97 | El color de `config.properties` se usa; el icono cambia con el color; el registro guarda `color`; color inválido da error. |
| `test_badge_color_changes_background_and_keeps_white_letters` | 99-106 | `_add_mito_badge` con cada color: el píxel (31,31) de un frame de 32 px es el color exacto con alfa 255, y aparece blanco `(255,255,255,255)`. |
| `test_coupled_shortcut_records_mode_and_opens_terminal_runner` | 108-118 | `--coupled` aparece en argv; los argumentos van tras `--`; el registro guarda `coupled` y `arguments`. |
| `test_application_name_uses_manifest_shortcut_label` | 120-121 | Nombre de la aplicación desde la etiqueta del manifiesto. |
| `test_declared_icon_is_used_and_refreshed_for_current_registry` | 123-148 | El icono declarado se usa; al cambiar el origen se regenera el icono y se reescribe el `.lnk` con `migrate`; queda un solo `.lnk`. |
| `test_declared_icon_profile_scope_fallback_and_confinement` | 150-167 | Icono en el payload del profile; un icono inválido cae al icono de MITO; si falta, usa el ejecutable; ruta fuera da `YitoError`. |
| `test_owned_launch_assigns_before_running_and_waits_for_job_tree` | 169-177 | Con `ProcessJob` simulado: `spawn` recibe `creationflags=0`, `wait_empty` y `close` se llaman una vez. |
| `test_inactive_or_nonlaunchable_components_rejected` | 179-188 | `can_open` falso sin ejecutable; componente inactivo da error sin llamar a `desktop_path`. |
| `test_shortcut_identity_stays_stable_after_version_change` | 190-212 | Tras cambiar de versión, el mismo `.lnk` y un solo registro (`app` = clave); el `.lnk` no contiene `@`; el lanzamiento resuelve el ejecutable de la nueva versión. |
| `test_badge_default_limit_and_profile_changes_keep_icon` | 214-223 | Por defecto `MITO`; `ab12` → `AB12`; `ABCDE`, `A B` y `!` dan error; desactivar el componente no cambia el icono. |
| `test_badge_preserves_icon_and_native_sizes` | 225-233 | El `.ico` contiene exactamente 16, 32 y 256; el píxel (0,0) conserva el fondo; `DEV` genera un archivo distinto de `MITO`. |
| `test_mito_shortcut_opens_manager_with_selected_profile` | 235-249 | Argv termina en `ui --profile test`; `terminal.command` no usa `shell`; el icono existe. |
| `test_delete_profile_preserves_data_and_last_profile` | 251-261 | Borrar un profile conserva su carpeta de workspace y la aplicación; no se puede borrar el último. |

## Dependencias

- `i18n.tr`; `storage`: `YitoError`, `identifier`, `inside`, `read_json`, `write_json`.
- `icons`: `create_mito_shortcut_icon`, `BADGE_COLORS`, `DEFAULT_BADGE_COLOR`.
- `engine`: `root`, `profile`, `selected`, `application_target`, `component_path`, `profiles`, `set_active`, `install_id`, `environment`.
- `catalog.effective`; `isolation.payload_path`; `arguments.split`.
- `terminal.prepare`, `terminal.command` (y `terminal_branding` vía `YitoTerminal.exe`).
- Llamadores: `components.py:307-309` (`migrate_application_links`), `storage_migration.py:56-58`, `pocketool_execution.py:115-132` (`retarget_link`), `host_apps.py:165-194` (`write_link`, `application_icon`, `desktop_path`), `__main__.py:324-332` y `:104-107`, `ui.py:1356-1365`, `:1449-1459`, `:1488-1492`, `:1840-1852`.

## UI (ui.py)

Diálogo `ShortcutDialog` (`ui.py:249-350`): `application=True` para aplicaciones y host; `application=False` para la terminal de MITO.

| Elemento | Id | Texto |
|---|---|---|
| Título | — | `tr('Generar acceso directo')` |
| Selector de entrada (si hay varias) | `shortcut-entry` | etiquetas del manifiesto |
| Argumentos | `shortcut-arguments` | `tr('Argumentos')` |
| Modo | `shortcut-coupled` (`Select`: `detached`/`attached`) | `tr('Proceso')`; las opciones `detached`/`attached` no pasan por `tr` |
| Letras | `shortcut-badge` (`max_length=4`) | `tr('Letras del icono (máximo 4)')` |
| Color | `shortcut-color-<clave>` | `tr('Color del fondo')`; las etiquetas son `name.capitalize()` y no pasan por `tr` |
| Error | `badge-error` | mensaje de `YitoError` |
| Botones | `shortcut-cancel` / `shortcut-confirm` | `tr('Cancelar')` / `tr('Generar')` |

Notas del diálogo: el valor por defecto de la insignia es `badge_text(engine)`; el color por defecto es `badge_color(engine)`. Enter en la insignia pasa el foco a los argumentos (solo en modo aplicación); Enter en otros campos confirma. El resultado es `(badge, color, coupled, arguments[, entry])` para aplicaciones y `(badge, color)` para la terminal.

`OpenApplicationDialog` (`ui.py:192-247`), al pulsar «Acc. directo» en lugar de abrir: no aplica aquí; sus textos son `Abrir`, `Argumentos`, `Proceso`, `Cancelar`.

Botones de fila (`ui.py:1120`, `1150`): `tr('Acc. directo')` con clase `app-shortcut`, ids `link-<índice>` y `host-link-<identity>`.

Mensajes de log al crear (`ui.py`): `tr('Acceso directo creado en el escritorio: {p1}')` (1852), `tr('Acceso a MITO creado en el escritorio: {p1}')` (1844), `tr('Acceso directo al host creado: {p1}')` (1805). Todos reciben el nombre del `.lnk` en `p1`.

## CLI

Los subcomandos `app-shortcut` y `shortcut` están en `cli_f8.md`. Ambos ejecutan dentro de `mutation(engine.root)` y escriben la ruta del `.lnk` en stdout.

## Notas para Rust

- Estado actual: no portado (PARITY: `shortcuts.py` ⬜). `mito-win` ya tiene `taskbar.rs` (`set_app_user_model_properties`), `links.rs` (junctions) y `resources.rs` (iconos PE), reutilizables.
- Escribir el `.lnk` con `IShellLinkW` (crate `windows`, ya en el workspace con `Win32_UI_Shell` y `Win32_System_Com`) en lugar de PowerShell. Es un cambio de implementación que produce el mismo contenido, pero habría que registrarlo en un ADR. Si se mantiene PowerShell, respetar `literal` y el orden de comandos.
- Registros con `serde_json` (feature `preserve_order`, ya activa) y mismas claves en el mismo orden. Hashes: SHA-256 de bytes UTF-8, hex en minúsculas, primeros 16 o 8 caracteres.
- Escritorio: `SHGetKnownFolderPath` con `FOLDERID_Desktop`, no `%USERPROFILE%`.
- `list2cmdline` debe reproducirse con las reglas de MSVCRT (`"` y backslashes antes de comillas).
- Usar `dunce` para rutas absolutas sin prefijo `\\?\` al guardarlas en los registros.

## Dudas

1. `migrate_application_links` escribe el `.lnk` con `write_link`, que reemplaza el archivo entero. `retarget_link`, que conserva propiedades, solo se usa en `pocketool_execution`. ¿Es intencional que la migración de aplicaciones pierda propiedades personalizadas del `.lnk`?
2. Cuando ya existe el registro nuevo (`new_registry`), `migrate` reescribe el registro antiguo (con el tag antiguo) y su `app`. ¿Debe conservarse así? El comentario dice «un acceso independiente para la versión nueva sigue registrado», pero la lógica deja dos registros con tags distintos apuntando a versiones distintas.
3. `pythonw.exe -B -I -X utf8 -m mito`: con `-I` no entran el directorio actual ni `PYTHONPATH`. El original no dice cómo resuelve `mito` (ver `hooks.md`, duda 2).
4. `CalledProcessError` de PowerShell no se captura en `__main__` (solo `YitoError`, `OSError`, `ValueError`). ¿Debe mostrarse como `MITO: …`?
5. `application_target` lanza un mensaje sin `tr` (`Shortcut unavailable or ambiguous`). ¿Se traduce en la versión Rust?
6. `badge_text` acepta letras Unicode (`isalnum`). Con fuentes que no tengan el glifo, el resultado es distinto. ¿Restringir a ASCII?
7. El test `test_retarget_link_preserves…` solo corre en Windows. En la versión Rust, ¿se prueba con `windows` directo?
