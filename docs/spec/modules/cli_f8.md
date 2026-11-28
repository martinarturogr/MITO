# cli_f8: `app-shortcut`, `shortcut`, `host-run` y `run --coupled/--owned` → mito::cli + mito::app

Origen: `core/app/mito/__main__.py` (354 líneas). Parser en las líneas 20-111; despacho en 186-200 (`host-run`), 307-316 (`run`), 324-332 (`shortcut`, `app-shortcut`). Manejo de errores y pausa en 334-350 y 12-17.
Tests: `core/tests/test_cli.py` (151 líneas). Relevantes para F8: `test_owned_component_forwards_arguments_and_pauses_on_failure` (50-61), `test_owned_host_forwards_arguments_and_pauses_on_success` (71-81), `test_owned_component_without_arguments_pauses_even_with_zero_exit` (83-94), `test_independent_component_does_not_pause` (96-101), `test_owned_start_failure_pauses` (103-109), `test_component_run_forwards_selected_shortcut` (111-123), `test_application_shortcut_forwards_selected_label` (125-135), `test_application_shortcut_forwards_mode_and_arguments` (137-147). Además, `test_terminal.py:139-143` (argv de `launch_component`) y `test_components.py:239` (`--coupled` en argv).

## Responsabilidad

Definir y ejecutar los subcomandos que crean accesos de escritorio (`app-shortcut`, `shortcut`) y los que los ejecutan (`host-run`, y los modos `--coupled`/`--owned` de `run`). Los accesos generados invocan `pythonw.exe -B -I -X utf8 -m mito …`, por eso estos comandos son parte del contrato de los `.lnk` (ver `shortcuts.md`, `host_apps.md`).

## Parser (argparse, `__main__.py`)

### `run` (modificadores F8, líneas 87-95)

| Opción | Tipo | Notas |
|---|---|---|
| `--profile` | requerido | |
| `--app` | requerido | |
| `--bin` | opcional | excluyente con `--shortcut` en la lógica (`Modo de lanzamiento no válido`) |
| `--shortcut` | opcional | etiqueta de acceso declarado |
| `--coupled` | `store_true` | grupo excluyente con `--owned` |
| `--owned` | `store_true`, `help=SUPPRESS` | oculto; lo usa la pestaña acoplada |
| `arguments` | `REMAINDER` | conserva `--` |

### `app-shortcut` (96-103)

- `help='Crear un acceso de aplicación en el escritorio'`.
- `installation` (posicional, ID de instalación de `mito installed`).
- `--profile` requerido; `--badge`; `--color`; `--shortcut`; `--coupled` (`store_true`, sin ayuda).
- `arguments` (posicional `nargs='*'`).

### `shortcut` (104-107)

- `help='Acceso a MITO en el escritorio'`.
- `--profile` requerido; `--badge`; `--color`. **No** tiene `--coupled` ni argumentos.

### `host-run` (36-42)

- `help='Abrir un ejecutable externo con un profile de MITO'`.
- `identity` (posicional).
- `--profile` requerido.
- Grupo excluyente: `--coupled` (`store_true`, sin ayuda) y `--owned` (`help=SUPPRESS`).
- `arguments` (posicional `nargs='*'`).

### Separador `--`

argparse elimina el primer `--` de los posicionales `nargs='*'`. Verificado por `test_cli.py:76-80` y `137-147`: `['--', '--port', '8080']` llega como `['--port', '8080']`. En `run` (`REMAINDER`) el `--` se conserva, por eso `__main__.py:311` quita un `--` inicial a mano (`arguments[1:] if arguments[:1] == ['--']`).

## Ejecución

### Antes del despacho (común a todos)

1. `root = root_path()`, `Engine(root)`, `report_threads(root)`.
2. `diagnostics.write(root, 'info', 'cli.<comando>', 'Command started', runtime=…)`.
3. `logs`: imprime las carpetas de diagnóstico y sale con 0 (no aplica a F8).
4. Solo para `ui`, `start` o sin comando: comprobación de longitud de ruta (`path_limit.problem`). No aplica a F8.
5. `engine.initialize()`. `ensure_default_profile()` solo para `ui`/`start`.
6. Revisión de traslado (`relocation.check`) para todos excepto `bootstrap`; los mensajes se imprimen si el comando no es `ui` ni `None`.
7. `terminal_identity.apply_safely(root)` si el comando es `shell`, `mcp-server` o tiene `owned` (`run --owned`, `host-run --owned`).

### `host-run` (186-200)

```
intercept(engine.root, engine.environment(args.profile))      # red: proxy del profile
result = host_apps.launch(engine, args.profile, args.identity, args.arguments,
                          coupled=args.coupled, owned=args.owned)
if args.owned:
    pause_owned_exit(result)
    return result
return 0
```

- Sin `--owned`, devuelve 0 aunque el proceso hijo siga vivo (`Popen`).
- Con `--coupled`, `launch` abre una pestaña (`terminal.launch_host`) y devuelve 0.
- Con `--owned`, espera al árbol (`ProcessJob`), pausa y devuelve el código del lanzador.

### `run` (307-316)

```
if not args.bin:
    intercept(engine.root, engine.environment(args.profile))
arguments = args.arguments[1:] if args.arguments[:1] == ['--'] else args.arguments
result = engine.launch(args.profile, args.app, args.bin, arguments,
                       coupled=args.coupled, owned=args.owned, shortcut=args.shortcut)
if args.owned:
    pause_owned_exit(result)
return result
```

- `engine.launch` (`engine.py:613-690`): sin `--owned`, `subprocess.Popen` y devuelve 0 (aplicaciones) o `subprocess.call` (`--bin`, devuelve el código del comando). Con `--coupled`, `launch_component` devuelve 0.
- `--coupled` con `--bin`, `--new_terminal` o `--owned` → `Modo de lanzamiento no válido`.

### `app-shortcut` (328-332)

```
with mutation(engine.root):
    print(application_link(engine, args.profile, args.installation, args.badge,
                           coupled=args.coupled, arguments=args.arguments,
                           shortcut=args.shortcut, color=args.color))
return 0
```

- Imprime la ruta absoluta del `.lnk` en stdout (una línea) y sale con 0.
- Argumentos de la aplicación: `args.arguments` (sin `--`, ya quitado por argparse).

### `shortcut` (324-327)

```
with mutation(engine.root):
    print(mito_link(engine, args.profile, args.badge, color=args.color))
```

Imprime la ruta del `.lnk` de la terminal de MITO y sale con 0.

### Lock de `mutation`

`app-shortcut` y `shortcut` escriben en `core/state/` dentro de `mutation(engine.root)` (ver `storage.md`). Con `wait=0`, si otra instancia tiene el lock: `YitoError('Otra instancia está modificando MITO. Espera a que termine.')`, que la CLI imprime como `MITO: …` y sale con 1.

## Salida, códigos de salida y pausa

| Situación | stdout | stderr | Código |
|---|---|---|---|
| Éxito, comando normal | — o la ruta del `.lnk` (`app-shortcut`, `shortcut`) | — | 0 |
| `YitoError`, `OSError`, `ValueError` | — | `MITO: <mensaje>` | 1 |
| Con `--owned`, cualquier error de arriba | — | `MITO: <mensaje>` y luego la pausa (abajo) | 1 |
| `KeyboardInterrupt` | — | `Operación interrumpida` | 130 |
| Otra excepción | — | traza de Python (`capture` y re-raise) | 1 |
| Error de uso de argparse (`--profile` ausente, `--coupled` y `--owned` juntos) | ayuda | `usage: …` / `error: …` (en inglés, de argparse) | 2 |

Pausa `pause_owned_exit(exit_code)`:

```
print(f'La aplicación terminó con el código {exit_code}.')
try:
    input('Pulsa Intro para cerrar esta pestaña.')
except (EOFError, KeyboardInterrupt):
    pass
```

Se llama solo en `run --owned` y `host-run --owned` (también en error, con código 1). `pocketool-run --owned` usa la misma pausa (fuera de F8).

Textos de `__main__` que no pasan por `tr`: `La aplicación terminó con el código {n}.`, `Pulsa Intro para cerrar esta pestaña.`, `Operación interrumpida`, `MITO: {exc}`. Las ayudas de argparse (`help=`) también son literales en español sin `tr`.

## Formato de los argumentos en los accesos

- `app-shortcut` escribe en el `.lnk`: `… run [--coupled] --profile <p> --app <key> [--shortcut <etiqueta>] [-- <args…>]`.
- `shortcut` escribe el `ui --profile <p>` de la terminal (no pasa por esta tabla, ver `shortcuts.md`).
- `host_apps.shortcut` escribe `host-run <identity> --profile <p> [--coupled] [-- <args…>]`.
- Los argumentos en `Arguments` del `.lnk` se separan con `list2cmdline`, así que un argumento con espacios viaja como `"two words"`.

## Invariantes

1. Los `.lnk` de aplicaciones usan `run` sin `--owned`. Solo la pestaña acoplada usa `--owned` (`terminal.launch_component`).
2. `host-run` sin `--owned` nunca pausa ni devuelve el código del hijo.
3. `--coupled` y `--owned` son excluyentes en `run` y `host-run`; `app-shortcut` solo admite `--coupled`.
4. Cada comando que escribe en `core/state/` lo hace dentro de `mutation`.
5. `app-shortcut` y `shortcut` imprimen exactamente una línea (la ruta).
6. La pausa se hace siempre con `--owned`, incluso si el comando devuelve 0 (test `test_owned_component_without_arguments…`).

## Casos límite y errores

- `run --coupled --owned`: error de argparse, código 2, antes de cualquier efecto.
- `run --bin X --shortcut Y`: `Modo de lanzamiento no válido`.
- `app-shortcut <installation>` con un profile inexistente: `engine.profile` lanza `YitoError`.
- `app-shortcut` con un componente no activo en el profile: `El componente debe estar activo en el profile`.
- `host-run` con un identity inexistente: `La aplicación del host ya no está registrada en MITO` (en `host_apps.target`).
- `host-run --owned` con un error de inicio (por ejemplo `OSError`): imprime `MITO: …`, pausa y sale con 1.
- `run` sin comando (`--app` ausente): argparse, código 2.
- `host-run` sin `--profile`: argparse, código 2.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_owned_component_forwards_arguments_and_pauses_on_failure` | `test_cli.py:50-61` | `run --owned` con `-- --name 'two words'`: `engine.launch` recibe `['--name', 'two words']`, `coupled=False`, `owned=True`; devuelve 7; la pausa pide el Intro. |
| `test_owned_host_forwards_arguments_and_pauses_on_success` | `test_cli.py:71-81` | `host-run host-id --profile one --owned -- --port 8080`: `host_apps.launch` recibe `['--port','8080']`; devuelve 0 y pausa. |
| `test_owned_component_without_arguments_pauses_even_with_zero_exit` | `test_cli.py:83-94` | Sin argumentos y con código 0, la pausa imprime `La aplicación terminó con el código 0.`. |
| `test_independent_component_does_not_pause` | `test_cli.py:96-101` | `run` sin `--owned` no llama a `input`. |
| `test_owned_start_failure_pauses` | `test_cli.py:103-109` | `engine.launch` lanza `OSError`: imprime error, pausa y devuelve 1. |
| `test_component_run_forwards_selected_shortcut` | `test_cli.py:111-123` | Para `[]`, `--coupled` y `--owned`, `--shortcut 'Tool Help'` y `-- two words` llegan a `engine.launch`. |
| `test_application_shortcut_forwards_selected_label` | `test_cli.py:125-135` | `app-shortcut extras/tool --profile one --shortcut 'Tool Help'`: `application_link` recibe `coupled=False`, `arguments=[]`, `shortcut='Tool Help'`, `color=None`. |
| `test_application_shortcut_forwards_mode_and_arguments` | `test_cli.py:137-147` | `app-shortcut installation --profile one --badge DEV --coupled -- --name 'two words'`: `coupled=True`, `arguments=['--name','two words']`, `badge='DEV'`. |
| `test_owned_launch_assigns_before_running_and_waits_for_job_tree` (indirecto) | `test_shortcuts.py:169-177` | `engine.launch(owned=True)` ↔ `ProcessJob`. |
| `test_launch_component` (indirecto) | `test_terminal.py:139-143` | `launch_component` construye `run --owned --profile … --app … -- run 'two words'`. |

## Dependencias

- `engine.launch`, `engine.initialize`, `engine.profile`, `engine.environment`, `engine.ensure_default_profile`.
- `host_apps.launch`, `host_apps.target`, `host_apps.shortcut`.
- `shortcuts.application_link`, `shortcuts.mito_link`.
- `network.intercept`; `storage.mutation`, `storage.root_path`, `storage.YitoError`; `diagnostics`; `error_tracking` (`capture`, `flush`, `resume`, `report_threads`); `terminal_identity.apply_safely`; `relocation.check`.
- `terminal.launch_component` y `terminal.launch_host` (lo que ejecutan las pestañas acopladas).

## Estado en Rust

`crates/mito/src/cli.rs` ya define los cuatro comandos con los mismos nombres y opciones:

- `Command::Shortcut` (`cli.rs:119-129`): `--profile`, `--badge`, `--color`.
- `Command::AppShortcut` (`cli.rs:352-371`): `installation`, `--profile`, `--badge`, `--color`, `--shortcut`, `--coupled`, `arguments` con `trailing_var_arg`.
- `Command::HostRun` (`cli.rs:199-213`): `identity`, `--profile`, `--coupled` (conflicto con `--owned`), `--owned` oculto, `arguments`.
- `Run` (`cli.rs`, alrededor de 325-345): `--shortcut`, `--coupled` (conflicto con `--owned`), `--owned` oculto, `arguments`.
- `Command::name()` (`cli.rs:144-162`): `host-run`, `app-shortcut`, `shortcut`.
- Tests de parseo en `cli.rs` (440-520): `host_run_consume_el_separador` (460), `app_shortcut_acepta_modo_y_argumentos` (480), `coupled_y_owned_son_excluyentes` (503).

Ejecución:

- `app.rs:87-99` y `app.rs:392-399`: `host-run`, `app-shortcut` y `shortcut` responden `Comando no disponible todavía: <nombre>` con código 1. Es el estado actual (`crates/mito/tests/cli.rs:249-285`).
- `run --coupled` y `run --owned` responden `Modo de lanzamiento no soportado todavía: --coupled` (`crates/mito/tests/cli.rs:689-702`). El original sí lo soporta (ver «Dudas»).
- Ayudas de cada subcomando: snapshots `insta` (`crates/mito/tests/cli.rs:742-786`, incluye `host-run`, `app-shortcut`, `shortcut`). La ayuda oculta de `--owned` está en `cli.rs:788-800`.
- Pausa `--owned` en Rust: `app.rs:33` (`Pausa de --owned`) y `app.rs:98-103` (`is_owned`). Cubre `Run`, `HostRun` y `PocketoolRun`.

## Notas para Rust

- Los textos de ayuda de Python son literales en español (no pasan por `tr`). Los snapshots de ayuda de Rust (`crates/mito/tests/cli.rs:742-786`) fijan el texto que genera clap: si cambian las descripciones, hay que regenerar los snapshots.
- La pausa usa `input` de la consola. Mantener los textos literales en la versión Rust.
- El `--` de argparse: en clap, `trailing_var_arg` + `allow_hyphen_values` conserva el `--`. Verificar que `host-run` y `app-shortcut` (posicional `nargs='*'` en Python) consumen el primer `--`, y que `run` (REMAINDER) lo quita en `app.rs`.
- Códigos de salida: 0, 1 (`YitoError`/`OSError`/`ValueError`), 2 (argparse), 130 (`Ctrl+C`). Ver `crates/mito/src/app.rs`.
- Cuando F8 esté portado, `shortcut`, `app-shortcut` y `host-run` deben usar `mutation` igual que Python y responder con la ruta del `.lnk` en stdout.

## Dudas

1. `run --coupled` en Rust devuelve `no soportado todavía`, pero en el original es un flujo completo (pestaña + `run --owned`). ¿Confirmar que la fase de F8 lo cubre?
2. Python devuelve el resultado de `engine.launch` directamente desde `run`, pero `host-run` lo convierte en 0 sin `--owned`. ¿Debe mantenerse esa asimetría en Rust?
3. `pause_owned_exit` usa `input()`, que devuelve `EOFError` sin consola. El original lo traga. ¿El mismo comportamiento en Rust?
4. Los errores de argparse (código 2) salen en inglés (`usage:`, `error:`). ¿Se traducen en Rust? Clap los muestra en inglés por defecto.
5. El orden de comprobaciones de `host-run` es: `intercept` antes de `launch`. Si el perfil no existe, `engine.environment` lanza antes de que `launch` valide el identity. ¿Es el orden deseado?
6. `shortcut` no acepta `--coupled` (el acceso de MITO siempre abre `ui` sin modo). Confirmar que es intencional en el contrato.
7. Si la pestaña `--owned` falla antes de imprimir (por ejemplo, por una excepción no `YitoError`), Python no pausa: sale con traza. ¿Debe pausarse también en ese caso?
