# terminal.py → mito-terminal::terminal

Origen: `core/app/mito/terminal.py` (173 líneas), `core/tests/test_terminal.py` (10 tests).

## Responsabilidad
Genera y mantiene el `settings.json` de la copia privada de Windows Terminal de la instancia (menú, perfiles, paletas, ventana), y construye y lanza los comandos `wt` que abren pestañas de esa instancia: gestor MITO, PowerShell(profile), CMD(profile) y aplicaciones/hosts/MCP.

Ejemplo de raíz usado en los JSON de este documento: `C:\MITO` (en el original es `engine.root`; los GUID y nombres dependen solo del perfil y del shell, no de la raíz; la ruta sí aparece en `commandline`, `startingDirectory` e `icon`).

## API pública del original
| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `brand_window_icons(root)` (`terminal.py:13-27`) | raíz | resultado de `ensure_branded_executable` o `None` | Propaga los de `terminal_branding` (ver su contrato). |
| `guid(profile=None, shell_type='cmd')` (`:30-31`) | perfil, shell | `'{' + uuid5 + '}'` | — |
| `child_command(engine, profile=None, shell_type='cmd')` (`:34-43`) | motor, perfil, shell | lista de argumentos | — |
| `command(engine, profile=None, shell_type='cmd', selected_profile=None)` (`:46-60`) | motor, perfil, shell, perfil seleccionado | lista argv para `subprocess` | `YitoError` `Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.` (`:49`); `YitoError` `Shell no soportada` (`:53`); `YitoError` de `engine.profile` si `selected_profile` no existe (`:58`). |
| `scheme(mode)` (`:63-71`) | `'manager'`, `'powershell'` o `'cmd'` | dict de esquema de color | `KeyError` si el modo no existe. |
| `prepare(engine, selected_profile=None)` (`:74-128`) | motor, perfil seleccionado | `None` | Propaga errores de `read_json`/`write_json` y de `migrate_shortcuts`. No hace nada si falta `WindowsTerminal.exe` (`:76-77`). |
| `launch(engine, profile=None, shell_type='cmd', selected_profile=None)` (`:131-134`) | | `0` | Los de `prepare` y `command`. |
| `launch_application(engine, profile, title, child)` (`:137-146`) | | `0` | Los de `prepare` y `command`. |
| `launch_component(engine, profile, app, title, arguments=(), shortcut=None)` (`:149-155`) | | `0` | Los de `launch_application`. |
| `launch_host(engine, profile, identity, title, arguments=())` (`:158-162`) | | `0` | Los de `launch_application`. |
| `launch_mcp(engine, pipe, owner, session)` (`:165-173`) | | `None` (no hay `return`; ver Duda) | Los de `prepare` y `command`. |

Duda: el encargo menciona una función `open`; en `terminal.py` no existe. La función que abre el terminal es `launch` (`:131`). Se documenta `launch` y se asume que `open` se refiere a ella.

## Formatos de disco
Ruta relativa a la raíz (`engine.root`). Todos los JSON se escriben con `write_json` (UTF-8 sin BOM, `indent=2`, `ensure_ascii=False`, `\n` final; ver `storage.md`).

- **`core/tools/windows-terminal/WindowsTerminal.exe`**: entrada obligatoria (`:48`, `:76`). Su ausencia desactiva `prepare` y `command`.
- **`core/tools/windows-terminal/YitoTerminal.exe`**: ejecutable derivado y brandeado (ver `terminal_branding.md`). Si existe, `command` lo usa en lugar del original (`:50-51`).
- **`core/tools/windows-terminal/.portable`**: archivo vacío que hace que Windows Terminal guarde sus ajustes en su carpeta (`:91`, `touch(exist_ok=True)`).
- **`core/tools/windows-terminal/settings/settings.json`**: escrito solo si cambia (`:125-126`). Ejemplo completo para un único perfil `java11`, con `WindowsTerminal`, `YitoTerminal` y raíz `C:\MITO`:

```json
{
  "theme": "dark",
  "launchMode": "maximized",
  "showTabsInFullscreen": true,
  "alwaysShowTabs": true,
  "copyOnSelect": false,
  "defaultProfile": "{d58b5131-c397-5636-be24-8e58dc2f02ca}",
  "firstWindowPreference": "defaultProfile",
  "newTabMenu": [
    {
      "type": "profile",
      "profile": "{d58b5131-c397-5636-be24-8e58dc2f02ca}"
    },
    {
      "type": "profile",
      "profile": "{5b8b1a8a-7a45-50a3-ba42-002700a6d109}"
    },
    {
      "type": "profile",
      "profile": "{bf682261-68d8-5c8b-b536-29838d41c9a6}"
    }
  ],
  "disabledProfileSources": [
    "Windows.Terminal.Wsl",
    "Windows.Terminal.Azure",
    "Windows.Terminal.PowershellCore",
    "Windows.Terminal.SSH",
    "Windows.Terminal.VisualStudio"
  ],
  "profiles": {
    "defaults": {
      "font": {
        "face": "Cascadia Mono",
        "size": 11
      },
      "padding": "12",
      "opacity": 94,
      "useAcrylic": true,
      "suppressApplicationTitle": true,
      "closeOnExit": "graceful"
    },
    "list": [
      {
        "guid": "{d58b5131-c397-5636-be24-8e58dc2f02ca}",
        "name": "CMD (java11)",
        "tabTitle": "CMD (java11)",
        "commandline": "C:\\MITO\\core\\tools\\python\\python.exe -B -I -X utf8 -m mito shell --profile java11",
        "startingDirectory": "C:\\MITO",
        "icon": "C:\\MITO\\core\\assets\\branding\\mito.ico",
        "colorScheme": "MITO CMD",
        "tabColor": "#9a7735",
        "hidden": false
      },
      {
        "guid": "{5b8b1a8a-7a45-50a3-ba42-002700a6d109}",
        "name": "PowerShell (java11)",
        "tabTitle": "PowerShell (java11)",
        "commandline": "C:\\MITO\\core\\tools\\python\\python.exe -B -I -X utf8 -m mito shell --type powershell --profile java11",
        "startingDirectory": "C:\\MITO",
        "icon": "C:\\MITO\\core\\assets\\branding\\mito.ico",
        "colorScheme": "MITO PowerShell",
        "tabColor": "#317ca4",
        "hidden": false
      },
      {
        "guid": "{bf682261-68d8-5c8b-b536-29838d41c9a6}",
        "name": "MITO",
        "tabTitle": "MITO",
        "commandline": "C:\\MITO\\core\\tools\\python\\python.exe -B -I -X utf8 -m mito ui",
        "startingDirectory": "C:\\MITO",
        "icon": "C:\\MITO\\core\\assets\\branding\\mito.ico",
        "colorScheme": "MITO Forest",
        "tabColor": "#368448",
        "hidden": false
      },
      {
        "guid": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
        "hidden": true
      },
      {
        "guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
        "hidden": true
      },
      {
        "guid": "{87bca5b6-a16a-40f6-8eb6-d20a946776c2}",
        "hidden": true
      }
    ]
  },
  "schemes": [
    {
      "name": "MITO Forest",
      "background": "#102820",
      "foreground": "#e7efdf",
      "cursorColor": "#b6e568",
      "selectionBackground": "#456451",
      "black": "#14241e",
      "red": "#e27878",
      "green": "#8ccf58",
      "yellow": "#e4c465",
      "blue": "#69acce",
      "purple": "#be94ca",
      "cyan": "#63c6b3",
      "white": "#d5dfd5",
      "brightBlack": "#72877b",
      "brightRed": "#ffa09a",
      "brightGreen": "#b5ef75",
      "brightYellow": "#ffe38a",
      "brightBlue": "#8fcfee",
      "brightPurple": "#d7b4e3",
      "brightCyan": "#91edda",
      "brightWhite": "#f5faef"
    },
    {
      "name": "MITO PowerShell",
      "background": "#102332",
      "foreground": "#e7efdf",
      "cursorColor": "#b6e568",
      "selectionBackground": "#456451",
      "black": "#14241e",
      "red": "#e27878",
      "green": "#8ccf58",
      "yellow": "#e4c465",
      "blue": "#69acce",
      "purple": "#be94ca",
      "cyan": "#63c6b3",
      "white": "#d5dfd5",
      "brightBlack": "#72877b",
      "brightRed": "#ffa09a",
      "brightGreen": "#b5ef75",
      "brightYellow": "#ffe38a",
      "brightBlue": "#8fcfee",
      "brightPurple": "#d7b4e3",
      "brightCyan": "#91edda",
      "brightWhite": "#f5faef"
    },
    {
      "name": "MITO CMD",
      "background": "#282319",
      "foreground": "#e7efdf",
      "cursorColor": "#b6e568",
      "selectionBackground": "#456451",
      "black": "#14241e",
      "red": "#e27878",
      "green": "#8ccf58",
      "yellow": "#e4c465",
      "blue": "#69acce",
      "purple": "#be94ca",
      "cyan": "#63c6b3",
      "white": "#d5dfd5",
      "brightBlack": "#72877b",
      "brightRed": "#ffa09a",
      "brightGreen": "#b5ef75",
      "brightYellow": "#ffe38a",
      "brightBlue": "#8fcfee",
      "brightPurple": "#d7b4e3",
      "brightCyan": "#91edda",
      "brightWhite": "#f5faef"
    }
  ]
}
```

- **Esquemas de color**: las tres paletas comparten todos los colores de ANSI y de texto (`foreground` `#e7efdf`, `cursorColor` `#b6e568`, `selectionBackground` `#456451`, `black` `#14241e` … `brightWhite` `#f5faef`). Solo cambian el nombre y el fondo (`terminal.py:10`, `:65-71`):

| Modo | `name` | `background` | `tabColor` (`:9`) | Uso |
|---|---|---|---|---|
| `manager` | `MITO Forest` | `#102820` | `#368448` (verde) | pestaña MITO |
| `powershell` | `MITO PowerShell` | `#102332` | `#317ca4` (azul) | PowerShell(profile) |
| `cmd` | `MITO CMD` | `#282319` | `#9a7735` (ámbar) | CMD(profile) |

- **Opacidad, acrílico y fuente** (`:116-118`): `opacity` 94, `useAcrylic` true, `font` Cascadia Mono 11, `padding` `"12"` (cadena), `suppressApplicationTitle` true, `closeOnExit` `"graceful"`.
- **Ajustes previos**: `core/state/terminal-settings-before-branding.json` guarda una copia de `settings.json` tal como estaba antes de la primera escritura de MITO, solo si no existe (`:96-98`) y solo si el archivo previo no estaba vacío.
- **Selección**: `core/state/terminal-profile.json` contiene el nombre del perfil seleccionado como cadena JSON, o `null`. Ejemplo: `"java11"` seguido de `\n`. Se escribe solo si cambia (`:85-86`).
- **Configuración de shell**: `config.properties`, clave `terminal.defaultShell`, leída con `network.properties` (`:87`).
- **Ruta de entrada de perfiles**: `profiles/<nombre>.json`, leída indirectamente por `engine.profiles()` (`engine.py:85`, ordenado).
- **Iconos**: `core/tools/windows-terminal/Images/terminal_contrast-black.ico` y `terminal_contrast-white.ico` (idénticos a `core/assets/branding/mito.ico`; ver `terminal_branding.md`).

## Comandos que lanza el terminal

### Argumentos de `child_command` (`terminal.py:34-43`)
Siempre `<raíz>\core\tools\python\python.exe -B -I -X utf8 -m mito` seguido de:

| Caso | Resto de argumentos |
|---|---|
| Sin perfil (gestor) | `ui` |
| Perfil, shell `cmd` | `shell --profile <perfil>` |
| Perfil, shell `powershell` | `shell --type powershell --profile <perfil>` |

En `settings.json` el campo `commandline` es `subprocess.list2cmdline` de esa lista (`:105`). Las rutas con espacios se entrecomillan.

### Argumentos de `wt` (`command`, `terminal.py:46-60`)
Ejemplo literal de lista `argv` para el gestor (`command(engine)`), con raíz `C:\MITO`:

```
["C:\\MITO\\core\\tools\\windows-terminal\\YitoTerminal.exe",
 "--maximized",
 "-w", "mito-<12 hex>",
 "new-tab",
 "--profile", "{bf682261-68d8-5c8b-b536-29838d41c9a6}",
 "--title", "MITO",
 "--startingDirectory", "C:\\MITO",
 "C:\\MITO\\core\\tools\\python\\python.exe", "-B", "-I", "-X", "utf8", "-m", "mito", "ui"]
```

- Ejecutable: `YitoTerminal.exe` si existe, si no `WindowsTerminal.exe` (`:50-51`).
- `--maximized` va antes de `-w` (`:60`).
- `-w mito-<hex>`: `hex` = primeros 12 caracteres de SHA-256 de `str(engine.root).casefold()` codificado en UTF-8 (`:54`). Todas las pestañas de una misma raíz comparten esa ventana.
- `--title`: `MITO` sin perfil (`:55`); `PowerShell (<perfil>)` o `CMD (<perfil>)` con perfil (`:55`). `launch_application` y `launch_mcp` lo sustituyen (`:142`, `:170`).
- `--profile`: GUID del perfil (`guid(profile, shell_type)`, `:60`). Sin perfil, el GUID del gestor.
- `--startingDirectory`: la raíz.
- Si se pasa `selected_profile`, se llama `engine.profile(selected_profile)` (valida) y se añade `--profile <selected>` **al final del child** (`:57-59`), no a los argumentos de `wt`.

### Lanzamiento
- `launch` (`:131-134`): ejecuta `prepare` y después `subprocess.Popen(argv, cwd=root)`. Retorna `0`.
- `launch_application` (`:137-146`): `prepare(engine)`; toma `command(engine, profile)` (shell `cmd`); reemplaza el valor después de `--title` por `title`; reemplaza todo lo que sigue a `-m mito` por `child` (`argv[module_index+2:] = child`). Retorna `0`.
- `launch_component` (`:149-155`): `child = run --owned --profile <p> --app <app> [--shortcut <s>] [-- <args...>]`.
- `launch_host` (`:158-162`): `child = host-run <identity> --profile <p> --owned [-- <args...>]`.
- `launch_mcp` (`:165-173`): `prepare`; título `MITO MCP`; child `mcp-server --pipe <pipe> --owner <owner> --session <session>`. Sin perfil ni `selected_profile`. No retorna valor.

## Qué ejecuta cada perfil
- Gestor `MITO` → `python.exe -m mito ui` (original).
- `PowerShell (<perfil>)` → `python.exe -m mito shell --type powershell --profile <perfil>` (original).
- `CMD (<perfil>)` → `python.exe -m mito shell --profile <perfil>` (original; sin `--type`).
- Objetivo Rust (según el encargo): `mito.exe shell --type <cmd|powershell> --profile <perfil>`, `mito.exe ui`. Duda: en Rust, ¿el CMD debe llevar `--type cmd` explícito? El original lo omite porque `cmd` es el valor por defecto; el resultado es el mismo, pero el contrato debe fijarlo.

## Invariantes
1. `guid()` es estable entre ejecuciones y máquinas: `uuid5(NAMESPACE, 'manager')` para el gestor y `uuid5(NAMESPACE, '<perfil>/<shell>')` para cada entrada de perfil, con `NAMESPACE = 87bca5b6-a16a-40f6-8eb6-d20a946776c2` (`:8`, `:31`). GUID de referencia:

| Entrada | GUID |
|---|---|
| gestor (`manager`) | `{bf682261-68d8-5c8b-b536-29838d41c9a6}` |
| `java11/powershell` | `{5b8b1a8a-7a45-50a3-ba42-002700a6d109}` |
| `java11/cmd` | `{d58b5131-c397-5636-be24-8e58dc2f02ca}` |
| `java21/powershell` | `{3210f90d-0a12-58f5-bd6a-9ffa9d5c819e}` |
| `java21/cmd` | `{a9b4fa81-6ba6-55da-80a1-1a7afd1ffd25}` |

2. `profiles.list` y `newTabMenu` tienen la entrada predeterminada en primer lugar (`:112`, `:113`). El orden del resto es: para cada perfil (en orden de `engine.profiles()`), PowerShell y luego CMD; al final el gestor (`:108-110`).
3. `defaultProfile` = GUID de la entrada predeterminada (`:90`). La entrada predeterminada es `guid(active, mode)`, o el gestor si `mode == 'mito'` o no hay perfil activo.
4. `newTabMenu` contiene solo las entradas visibles (`:113`), sin los tres GUID integrados que se ocultan.
5. Los tres GUID integrados (`{61c54bbd-…}`, `{0caa0dad-…}`, `{87bca5b6-…}`) se añaden a `profiles.list` con `hidden: true` al final (`:115`).
6. `prepare` es idempotente: si el JSON resultante es igual al del disco, no se escribe (`:125-126`).
7. `terminal-settings-before-branding.json` se crea como mucho una vez (`:97`).
8. `schemes` conserva los esquemas ajenos y reemplaza los tres de MITO (`:119`, `:124`).
9. Los nombres de esquema de MITO son exactamente `MITO Forest`, `MITO PowerShell` y `MITO CMD`.
10. La selección sigue el orden: perfil pedido (si existe) → perfil previo (si existe) → primer perfil → ninguno (`:82-84`).
11. `terminal.defaultShell` válido es `cmd`, `powershell` o `mito`; cualquier otro valor se trata como `cmd` (`:88-89`).
12. Sin perfiles, `defaultProfile` es el gestor (test `test_no_profiles_defaults_to_manager`).

## Casos límite y errores
- **Sin `WindowsTerminal.exe`**: `prepare` retorna sin escribir nada (`:76-77`). `command` lanza `YitoError` con el mensaje exacto de `:49`.
- **Shell no soportada** en `command` (solo `cmd` y `powershell`; `mito` es válido solo para `defaultShell`): `YitoError('Shell no soportada')` (`:53`).
- **Perfil seleccionado inexistente**: no se usa; cae al previo o al primero (`:82-84`). Si `selected_profile` se pasa a `command`, `engine.profile` lanza `YitoError` (`engine.py:88-92`).
- **Sin perfiles**: `active` es `None`, `default_guid` es el gestor, no hay entradas de perfil y `newTabMenu` solo tiene MITO.
- **`settings.json` ausente**: se parte de `{}`; no se crea backup (`:97`).
- **`settings.json` con `profiles.defaults` previos**: se conservan las claves ajenas y se sobrescriben las de MITO (`:116-118`).
- **`terminal-profile.json` con `null`**: se interpreta como sin selección previa (`:81`).
- **Ruta de raíz con espacios**: `list2cmdline` la entrecomilla dentro de `commandline`; `argv` no se entrecomilla (lista).
- **Errores de E/S** (`read_json` sobre JSON inválido, escritura fallida): no se capturan en `terminal.py`.
- **Migración de atajos**: `prepare` termina llamando a `migrate_shortcuts(engine)` (`:127-128`); un fallo ahí se propaga.

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_profiles_menu_colors_fullscreen_and_update` | Con `java11` y `java21`: nombres visibles en orden `CMD (java11)`, `PowerShell (java11)`, `PowerShell (java21)`, `CMD (java21)`, `MITO`; `defaultProfile` = `guid('java11')`; `launchMode` maximized; `showTabsInFullscreen`; `opacity` 94; tres esquemas distintos; `commandline` de PowerShell con `--type powershell --profile java11`; tras añadir un perfil `newTabMenu` tiene 7 entradas; segunda ejecución no cambia bytes. |
| `test_default_follows_selection_and_survives_background_prepare` | `prepare(selected='java21')` y luego `prepare()` sin selección conservan la predeterminada `java21` en `defaultProfile`, `newTabMenu[0]` y `profiles.list[0]`; al borrar `java21` vuelve a `guid('java11')`. |
| `test_default_shell_choices_and_invalid_value_fallback` | Para `powershell`, `mito`, `cmd` e `invalid`: la predeterminada es `guid('java21','powershell')`, `guid()`, `guid('java21')` y `guid('java21')`; `newTabMenu[0]` y `profiles.list[0]` coinciden; si el modo no es `mito`, la última entrada del menú es el gestor. |
| `test_no_profiles_defaults_to_manager` | Sin perfiles, `defaultProfile` = `guid()`. |
| `test_manager_launch_keeps_explicit_manager_and_selected_default` | `launch(selected='java21')`: el `--profile` de `wt` es el gestor (`guid()`), mientras `defaultProfile` en settings es `guid('java21')`. |
| `test_prepare_brands_portable_terminal_window_icons` | Tras `prepare` (con `ensure_branded_executable` parcheado), los dos `.ico` de `Images` son idénticos a `mito.ico` y el `icon` de cada entrada visible es la ruta a `mito.ico`. |
| `test_bootstrap_tools_brand_before_publishing_first_run_marker` | `ensure_tools` deja `bootstrap-tools.json`, `YitoTerminal.exe` distinto del original, `command(engine)[0]` = `YitoTerminal.exe`, e `Images` iguales a `mito.ico`. Ejercita `bootstrap_tools` (no portado aquí). |
| `test_commands_reuse_instance_window_and_set_shell` | `command` del gestor incluye `--maximized`; gestor y PowerShell comparten el valor tras `-w`; el GUID de `java21/powershell` aparece en el argv; los últimos 5 elementos son `shell --type powershell --profile java21`. |
| `test_coupled_application_gets_named_tab_and_owned_runner` | `launch_component(...'My Editor', ['run','two words'])`: `--title` = `My Editor`; los últimos 9 elementos son `run --owned --profile java21 --app extras/editor -- run "two words"`. |
| `test_powershell_receives_activated_environment` | Ejercita `Engine.shell` (`engine.py:692`), no `terminal.py`: `powershell.exe` con `-NoProfile`, `MITO_PROFILE=java21` y `USERPROFILE` de la instancia en el entorno. Duda: pertenece al contrato de `engine`. |

## Dependencias
- `i18n.tr` (`mito-i18n`): mensajes `Falta Windows Terminal portable…`, `Shell no soportada`, y `No existe el profile {p1}` de `engine.profile`.
- `storage`: `YitoError`, `read_json`, `write_json` (`storage.md`).
- `engine` (`engine.py`): `root`, `profiles()` (`:85`), `profile(name)` (`:88`), `shell()` (`:692`, solo en tests).
- `network.properties(root)`: lee `config.properties` y devuelve `terminal.defaultShell` (`network.py:18`).
- `terminal_branding.ensure_branded_executable` (`terminal_branding.md`).
- `pocketool_execution.migrate_shortcuts(engine)` (`pocketool_execution.py:112`).
- `bootstrap_tools.ensure_tools` (solo en test).
- `uuid`, `hashlib`, `subprocess` de la biblioteca estándar.

## Notas para Rust
- **GUID**: `uuid` con feature `v5` y el mismo namespace; los valores de la tabla de invariantes deben coincidir byte a byte. Confirmar la codificación: la cadena usa `str(UUID)` (minúsculas, con guiones) y se envuelve en llaves.
- **JSON**: `serde_json` con `PrettyFormatter` de 2 espacios y `preserve_order` para conservar el orden de claves insertadas. Las claves ajenas de `settings.json` deben conservarse (`settings.update` no las borra).
- **Procesos**: `std::process::Command` con argumentos como lista; `cwd` = raíz. En Windows, `list2cmdline` debe replicarse para `commandline` (las reglas de entrecomillado de MSVCRT).
- **Hash de la ventana**: SHA-256 (crate `sha2`) de `raíz.to_lowercase()` en UTF-8. `casefold()` de Python y `to_lowercase()` de Rust pueden diferir en casos especiales. Duda: aceptable para rutas Windows.
- **Firmas sugeridas**:
  - `fn command(engine: &Engine, profile: Option<&str>, shell: Shell, selected: Option<&str>) -> Result<Vec<OsString>, TerminalError>`
  - `fn prepare(engine: &Engine, selected: Option<&str>) -> Result<(), TerminalError>`
  - `fn launch(...) -> Result<(), TerminalError>` (en Python retorna `0`; en Rust `()` basta).
  - `enum Shell { Cmd, PowerShell }` para `shell_type`; `mito` solo existe como valor de `defaultShell`.
- **Errores**: `YitoError` equivale a un tipo de error de dominio (`thiserror`); conservar los mensajes de `tr`.
- **Duda sobre `launch_mcp`**: el original no retorna valor; en Rust se propone `Result<(), _>`.
- **Duda sobre `open`**: el encargo menciona `open`, no existe en el original (se usa `launch`).
- **Duda sobre el GUID `{87bca5b6-…}`**: coincide con `NAMESPACE`. Está en la lista de ocultos sin comentario que lo explique; se replica tal cual. Duda: ¿es un error o un ID intencional?
- **Duda sobre los GUID `{61c54bbd-…}` y `{0caa0dad-…}`**: el original los llama "entradas integradas de Terminal" sin nombrarlas. No se atribuyen nombres aquí; confirmar.
- **Duda sobre el CMD sin `--type`**: ver "Qué ejecuta cada perfil".
- **Duda sobre `ARCHITECTURE.md` (línea 523)**: dice que "los perfiles llaman a `shell --type powershell|cmd`", pero el código no pasa `--type` para CMD (`terminal.py:37-40`). El código manda.
- **Duda sobre la ruta de la raíz**: `command` usa `str(engine.root)` sin `resolve()` para la ventana (`:54`), mientras `terminal_identity` sí usa `root.resolve()`. Si `engine.root` fuera relativo o tuviera otra forma, el hash no coincidiría. Confirmar si en Rust la raíz siempre es absoluta y canónica.
- **Duda sobre `wt` y el comando final**: el argv de `launch_application` pone un comando después de `new-tab --profile <guid>`. Se asume que el comando final sustituye el `commandline` del perfil y que el color y el icono del perfil se mantienen. Confirmar con la documentación de Windows Terminal.
- **Duda sobre `ARCHITECTURE.md` (línea 533)**: "entre varios gestores prevalece la última selección" depende de que `prepare` lea `terminal-profile.json` en cada llamada. Se replica tal cual.
- **Duda sobre el orden de claves de `settings.json` cuando ya existía**: `settings.update` mantiene el orden previo y añade las nuevas al final. Replicar exactamente requiere `preserve_order`.
