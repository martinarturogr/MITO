# __main__.py + arguments.py + startup.py → mito (crate `mito`, binario)

Origen: `core/app/mito/__main__.py` (354 líneas), `core/app/mito/arguments.py` (26 líneas),
`core/app/mito/startup.py` (28 líneas), `core/tests/test_cli.py` (12 tests).
Scripts de entrada relacionados: `mito.cmd` (raíz, 24 líneas), `core/commands/mito.cmd` (2 líneas).

## Responsabilidad
Interfaz de línea de comandos del binario `mito`: analiza argv con `argparse`, construye el
`Engine` (salvo validaciones previas), ejecuta el subcomando, traduce errores de dominio a
`MITO: <mensaje>` en stderr y devuelve el código de salida. `startup.py` es la guarda de
arranque que atrapa fallos de importación. `arguments.py` tokeniza cadenas de argumentos
según las reglas de Windows para los subcomandos que reciben una línea de comandos.

## Árbol de subcomandos (API pública del original)

Parser raíz: `prog='mito'`, descripción `Tu entorno de desarrollo en una carpeta`
(`__main__.py:21`), `dest='command'` (`:22`). Sin subcomando se ejecuta `ui` (`:127`, `:150`).

Leyenda de fase: F4 (`installed`, `logs`, `doctor`, `bucket`, `search`, `profile create/delete/list/show`,
`install`, `activate`, `deactivate`, `shell`, `run`), F5 (`start`, `bootstrap`, `terminal`, `ui`),
F8 (`shortcut`, `app-shortcut`, `host-run`, `run --coupled`), F10 (`pocketool-run`),
F11 (`profile export/import`, `migrate-storage`, `release`), F12 (`mcp-server`).
Las filas de otras fases solo documentan la firma.

| Subcomando | Argumentos y opciones (nombre exacto) | Obligatorio | Default | Choices | Ayuda (`help=`) | Fase | Origen |
|---|---|---|---|---|---|---|---|
| (ninguno) / `ui` | `--profile` | no | `None` | — | `Abrir la interfaz Textual` (solo en `ui`, `:23`) | F5 | `:23`, `:186-188` |
| `start` | `--profile` | no | `None` | — | `Abrir MITO en Windows Terminal portable` | F5 | `:24`, `:189-191` |
| `bootstrap` | (ninguno) | — | — | — | `Preparar herramientas privadas` | F5 | `:25`, `:182-185` |
| `installed` | (ninguno) | — | — | — | `Listar todos los componentes instalados` | F4 | `:26`, `:204-206` |
| `posthog-check` | (ninguno) | — | — | — | `Enviar una excepción de diagnóstico y mostrar la respuesta HTTP` | **Duda** (no asignada) | `:27`, `:139-149` |
| `logs` | (ninguno) | — | — | — | `Mostrar las carpetas de diagnóstico local` | F4 | `:28`, `:122-125` |
| `pocketool-run` | `--profile` · `--package` · `--entry` (las tres obligatorias) · `--owned` (oculta, flag) · `--arguments64` (oculta) · `arguments` (REMAINDER) | sí las tres | — | — | `Ejecutar un comando Pocketool en una pestaña de MITO` (`--owned` y `--arguments64` con `SUPPRESS`) | F10 | `:29-35` |
| `host-run` | `identity` (posicional) · `--profile` · grupo excluyente `--coupled` / `--owned` (oculta) · `arguments` (`nargs='*'`) | `identity`, `--profile` | — | — | `Abrir un ejecutable externo con un profile de MITO` | F8 | `:36-42` |
| `doctor` | (ninguno) | — | — | — | `Comprobar runtime y estado` | F4 | `:43`, `:212-229` |
| `mcp-server` | `--pipe` · `--owner` (`int`) · `--session` (las tres obligatorias) | sí | — | — | oculto (`SUPPRESS`) | F12 | `:44-47` |
| `migrate-storage` | (ninguno) | — | — | — | `Convertir instalaciones antiguas a rutas estables, con las aplicaciones cerradas` | F11 | `:48`, `:201-203` |
| `bucket import` | `name` · `source` | sí | — | — | — | F4 | `:49-52` |
| `bucket refresh` | `name` (posicional opcional) | no | `None` | — | — | F4 | `:53-54` |
| `search` | `query` (posicional opcional) · `--compatible` (flag) | no | `query=''` | — | — | F4 | `:55-57` |
| `profile create` | `name` · `--workspace` | `name` | `workspace=None` | — | — | F4 | `:59-61` |
| `profile delete` | `name` | sí | — | — | — | F4 | `:62` |
| `profile export` | `name` · `output` | sí | — | — | — | F11 | `:63-65` |
| `profile import` | `source` · `--name` (obligatoria) | sí | — | — | — | F11 | `:66-68` |
| `profile list` | (ninguno) | — | — | — | — | F4 | `:69` |
| `profile show` | `name` | sí | — | — | — | F4 | `:70-71` |
| `install` | `app` · `--profile` | `app` | `profile=None` | — | — | F4 | `:72-74` |
| `activate` | `installation` (`help='ID que muestra mito installed'`) · `--profile` | sí ambas | — | — | — | F4 | `:75-78` |
| `deactivate` | igual que `activate` | sí ambas | — | — | — | F4 | `:75-78` |
| `shell` | `--profile` · `--type` · `arguments` (REMAINDER) | `--profile` | `--type=cmd` | `cmd`, `powershell` | — | F4 | `:79-82` |
| `terminal` | `--profile` · `--type` · `--new` (flag) | `--profile` | `--type=cmd` | `cmd`, `powershell` | — | F5 | `:83-86` |
| `run` | `--profile` · `--app` · `--bin` · `--shortcut` · grupo excluyente `--coupled` / `--owned` (oculta) · `arguments` (REMAINDER) | `--profile`, `--app` | `bin=None`, `shortcut=None` | — | — | F4 (`--coupled`: F8; `--owned`: **Duda**) | `:87-95` |
| `app-shortcut` | `installation` · `--profile` · `--badge` · `--color` · `--shortcut` · `--coupled` (flag) · `arguments` (`nargs='*'`) | `installation`, `--profile` | `None` salvo flag | — | `Crear un acceso de aplicación en el escritorio` | F8 | `:96-103` |
| `shortcut` | `--profile` · `--badge` · `--color` | `--profile` | `None` | — | `Acceso a MITO en el escritorio` | F8 | `:104-107` |
| `release` | `kind` (posicional opcional) · `--local` (flag) · `--output` | no | `kind=None` (se trata como `hotfix`) | `mayor`, `minor`, `hotfix` | `Incrementar versión y publicar en GitHub` | F11 | `:108-112` |

Textos de ayuda de `release` (`:109-112`):
- `kind`: `mayor: X.0.1; minor: M.X.1; hotfix: M.m.X (defecto)`
- `--local`: `Sólo generar ZIP local, sin incrementar ni publicar`
- `--output`: `Directorio de salida (defecto: releases)`

Notas sobre `arguments` (REMAINDER / `nargs='*'`):
- `run`, `shell`, `pocketool-run` usan `argparse.REMAINDER`: argparse conserva el `--` y el
  código lo elimina a mano solo si es el primer elemento (`__main__.py:210`, `:301`, `:311`).
- `host-run` y `app-shortcut` usan `nargs='*'`: no hay eliminación manual de `--` en el código;
  los tests de `host-run` esperan que `['--', '--port', '8080']` llegue como `['--port', '8080']`
  (`test_cli.py:76-80`), lo que implica que argparse consume el `--` en ese modo.
  **Duda:** depende del comportamiento de argparse de la versión de Python.
- `--owned` y `--coupled` (y `--shortcut`) se documentan como `help=SUPPRESS` en `owned` (`:33`, `:41`, `:94`);
  `--coupled` de `run` y `host-run` son excluyentes entre sí (`:39-41`, `:92-94`).

### Validaciones previas a la ejecución
- `release --local` con `kind` (cualquiera de los tres) → `parser.error(...)` (`:114-115`). Salida
  exacta en stderr: `mito: error: --local no admite mayor, minor ni hotfix: empaqueta la versión actual`
  (prefijo `usage: ...` de argparse antes), código de salida **2**. Ocurre antes de construir `Engine`.
- Errores de argparse (subcomando desconocido, obligatorio ausente, `choices` inválido, argumento
  extra): mensaje estándar de argparse en stderr, código **2**, sin construir `Engine`.

## Salida exacta por comando

Convención: `print` escribe a stdout salvo que se indique `file=sys.stderr`. "→ 0" es el código
de salida en el caso de éxito. Los errores de dominio se describen en la sección de errores.

| Subcomando | stdout | stderr | Código |
|---|---|---|---|
| `installed` | por cada entrada de `engine.installed()`: `f'{install_id:65} {lock["version"]}'` (`:206`) → el ID (clave del catálogo, p. ej. `extras/tool`; `engine.py:136-137`) rellenado a **65** caracteres a la izquierda, un espacio y la versión | — | 0 |
| `logs` | una ruta por línea, de `diagnostics.locations(root)` (`:123-124`). Son dos: `<root>\logs` y `<TEMP>\MITO-logs\<hash16>` | — | 0 (sale antes de `diagnostics.write`, sin entrada `Command started`) |
| `doctor` | ver bloque siguiente | — | 0 si no hay excepción |
| `search [query] [--compatible]` | por cada resultado de `rank_entries`: `f'{key:44} {str(manifest.get("version", "?")):20} {status}'` (`:247`). Con `--compatible` solo se imprimen los de `status == 'Compatible'` (`:246`). Vacío si no hay resultados | — | 0 |
| `bucket import <name> <source>` | `{n} manifiestos importados` (`:233`) | — | 0 |
| `bucket refresh` | sin nombre: una línea `{name}: {n} manifiestos` por bucket (`:240-241`); con nombre: solo ese bucket | — | 0 |
| `profile create <name> [--workspace]` | `Profile creado: {valor devuelto por create_profile}` (`:250`) | — | 0 |
| `profile delete <name>` | `Profile eliminado: {name}` (`:253`) | — | 0 |
| `profile list` | nombres ordenados, uno por línea (`:283`). Si no hay ninguno: `Todavía no hay profiles` | — | 0 |
| `profile show <name>` | JSON de `engine.profile(name)`: `json.dumps(..., ensure_ascii=False, indent=2)` (`:285`) | — | 0 |
| `profile export <name> <output>` | valor devuelto por `export_profile` (`:256`) | — | 0 |
| `profile import <source> --name N` | ver «Diálogos interactivos» | — | 0 |
| `install <app> [--profile]` | lo que imprima `engine.install` (log por defecto `print`, `engine.py:300`; **Duda:** texto no documentado aquí). Si hay descargas sin hash, ver «Diálogos» | — | 0 |
| `activate` / `deactivate` | ninguna (`:207-208`) | — | 0 |
| `shell` | lo que imprima el shell hijo; devuelve el código de `engine.shell` (`:211`) | — | código del shell |
| `terminal` | ninguna propia; `launch` devuelve **0** tras `Popen` sin esperar (`terminal.py:131-134`) | — | 0 |
| `start` | ninguna propia | — | 0 |
| `run` | lo que imprima la app; devuelve `engine.launch(...)` (`:312-316`) | — | código de la app |
| `run --owned` | `La aplicación terminó con el código {N}.` (`pause_owned_exit`, `:13`) | — | código de la app |
| `host-run` | sin `--owned`: nada propio | — | **0 siempre** (`:200`) |
| `host-run --owned` | `La aplicación terminó con el código {N}.` | — | código de `launch` |
| `pocketool-run` | sin `--owned`: lo que devuelva `launch` (`:306`) | — | código de `launch` |
| `pocketool-run --owned` | `La aplicación terminó con el código {N}.` | — | código de `execute` |
| `app-shortcut` | la ruta devuelta por `application_link` (en el test es `tool.lnk`) (`:331-332`) | — | 0 |
| `shortcut` | la ruta devuelta por `mito_link` (`:327`) | — | 0 |
| `release --local` | la ruta devuelta por `build_release(engine, output)` (`:320`) | — | 0 |
| `release` (publicar) | lo que devuelva `publish(engine, output, kind=kind or 'hotfix')` (`:323`) | — | 0 |
| `migrate-storage` | ninguna propia (`:201-203`) | — | 0 |
| `bootstrap` | lo que imprima `core/bootstrap.py`; devuelve su código (`:183-185`) | — | código de `bootstrap.py` |
| `mcp-server` | devuelve `run(engine, pipe, owner, session)` (`:181`) | — | código de `run` |
| `ui` / sin subcomando | la interfaz Textual; al salir, `return 0` (`:333`) | — | 0 |
| `posthog-check` | `PostHog: HTTP {status}. Excepción de diagnóstico enviada. Comprueba el evento $exception con mito_diagnostic=true en tu proyecto; la respuesta HTTP no confirma su procesamiento en Error Tracking.` (`:146-148`) | en fallo: `PostHog: envío fallido ({TipoExcepción}): {mensaje}` (`:144`) | 0 / **1** |

### Bloque de `doctor` (`:212-229`)

Líneas en stdout, en este orden (las de `Python`, `Pillow`, `Textual` dependen del runtime Python):

```
MITO: <engine.root>
Python: <versión> | Pillow: <versión> | Textual: <versión>
Home único: <engine.root>\home
MCP SDK: <versión> | activación manual; sin servidor al iniciar
```
o, si falta `mcp`: `MCP SDK: falta; ejecuta mito.cmd bootstrap antes de habilitar MCP`.

Después, para `windows-terminal` y `7zip` (`:222`):
- `<tool>: <version>` si existe `core/tools/<tool>/.mito-tool.json` (lee `version`, `:225`).
- `Falta <tool>; ejecuta bootstrap` si no existe.

Luego:
```
Catálogo: <N> apps | Profiles: <M>
OK profile <nombre>        (una línea por profile, tras engine.environment(name) sin error)
```

**Duda:** `doctor` es específico del runtime Python (Pillow, Textual, `mcp` SDK, `bootstrap`). En Rust no
existen esas dependencias; decidir qué líneas se conservan y con qué texto.

### Diálogos interactivos (entrada estándar)

`input()` con las preguntas literales:
- `profile import`: `ask(lines)` imprime las líneas y pregunta `¿Continuar? [s/N] `.
  - Líneas de revisión (`:266-272`): `{key} {version}` (+ ` (retenida)` si `pinned`); si hay archivos
    de configuración, la cabecera `Archivos de configuración:` y, por cada uno,
    `  {target} · {estado}` donde estado es `propio del nuevo profile` (si `own`) o, según
    `state`: `nuevo`, `idéntico, se omite`, `ya existe con otro contenido`.
  - Sobrescritura, archivo a archivo (`:278-279`): `¿Sobrescribir {target}?` seguido de `¿Continuar? [s/N] `.
  - Descargas sin hash (`:281`): `Descargas sin hash:` y la lista, luego `¿Continuar? [s/N] `.
  - Respuesta afirmativa solo con `s` (tras `.strip().lower()`). EOF o Ctrl+C → `no`.
- `install`: imprime `Descargas sin hash: no se podrá comprobar su integridad.` y la lista de
  descargas unidas por `\n`, luego pregunta `¿Descargar e instalar? [s/N] ` (`:287-293`). Solo `s` confirma.
- `pause_owned_exit` (`:12-17`): imprime `La aplicación terminó con el código {N}.` y espera
  `Pulsa Intro para cerrar esta pestaña.` (EOF/Ctrl+C no fallan).
- Ruta larga (`:131-137`): `Pulsa Intro para cerrar` (traducido con `tr`).

## Errores y su presentación

Cadena de manejo (`:334-345`):

| Excepción | Acción | stderr | Código |
|---|---|---|---|
| `YitoError`, `OSError`, `ValueError` | `capture(root, exc, operation='cli.<cmd>')`; si `--owned`, `pause_owned_exit(1)` | `MITO: {str(exc)}` | **1** |
| `KeyboardInterrupt` | nada más | `Operación interrumpida` | **130** |
| Cualquier otra `Exception` | `capture(..., handled=False)`; `raise` | traceback de Python | **1** (salida no controlada) |
| Error de argparse | — | uso + mensaje de argparse | **2** |
| `SystemExit` de argparse dentro de `main` | — | — | 2 |

- `<cmd>` es el subcomando o `ui` si no hay (`'cli.' + (args.command or 'ui')`).
- `str(exc)` de un `YitoError` es el mensaje en español ya traducido (p. ej. `Bucket no configurado`,
  `No existe el profile {name}`), ver `storage.py`.
- Un `OSError` se muestra con el texto de Python (`[Errno 2] ...` si lo tiene).
- `posthog-check` no pasa por esta cadena: tiene su propio `try` y sale con 1 (`:141-145`).
- Fallo al importar `mito.__main__` (ver `startup.py`) → stderr `MITO: {exc}\nLog: {primera ruta de diagnostics.locations}`, código 1.
- Ruta larga (`:127-138`): stderr `\nMITO: {message}\n`, espera Intro y código 1. Solo aplica a
  `ui`, `start`, `bootstrap` y sin subcomando.
- Fallo de la revisión de traslado (`:167-172`): no impide arrancar; se registra
  `No se pudo completar la revisión del traslado de MITO: {p1}` y se reintenta en el siguiente arranque.

## Códigos de salida (resumen)

| Código | Cuándo |
|---|---|
| 0 | Éxito; `ui`; `host-run` sin `--owned`; `start`. |
| 1 | `YitoError`/`OSError`/`ValueError`; ruta larga; `posthog-check` fallido; `startup.py` no importa; `--owned` con error (pausa incluida); `doctor` con `ImportError` cae en salida no controlada. |
| 2 | Errores de argparse y `release --local <kind>`. |
| 130 | Ctrl+C (`KeyboardInterrupt`). |
| otro | Código devuelto por lanzadores (`run`, `shell`, `terminal`→0, `bootstrap`, `mcp-server`, `pocketool-run`) o por la aplicación con `--owned`. |

## Flujo de arranque (`startup.py`)

1. `root = Path(__file__).resolve().parents[3]` (`startup.py:5`): raíz del proyecto, 3 niveles sobre `core/app/mito/startup.py`.
2. Inserta `root/core/app` al principio de `sys.path` (`:6`) e importa `mito.diagnostics` (`:7`). **Esta importación no está protegida**: si falla, el proceso falla con traceback.
3. `main()` (`:10-24`): intenta `from mito.__main__ import main as cli`.
   - Si falla (`except Exception`): `diagnostics.write(root, 'error', 'startup.import', diagnostics.message(root, exc), runtime=diagnostics.environment(), exception=<tipo>)`; intenta `capture(root, exc, handled=False, operation='startup.import')` y `flush()` (fallos ignorados); imprime `MITO: {exc}` y `Log: {diagnostics.locations(root)[0]}` en stderr; devuelve 1.
   - Si funciona: `return cli()`, que usa `sys.argv[1:]`.
4. `raise SystemExit(main())`.

Se invoca como `python -B -I -X utf8 -m mito.startup <args>` desde `mito.cmd` (ver abajo).

## Tokenizado de argumentos (`arguments.py`)

Función `split(value)` (`:6-26`). Solo la usan, para subcomandos internos, `engine.py:444,475`, `hooks.py:173`,
`shortcuts.py:60` y `ui.py:230,325`. **La CLI no la usa**: `argparse` recibe `sys.argv` ya separado por el runtime.

Reglas:
1. `value` vacío o `None` → lista vacía (`:7-8`).
2. `value` es lista → cada elemento convertido con `str` (`:9-10`).
3. No Windows → `shlex.split(value)` (`:11-12`). **No se porta** (ADR 0008, punto 3: solo Windows x64).
4. Windows (`:13-26`): `CommandLineToArgvW('mito ' + value)` (se antepone `mito ` para que el primer token sea el nombre del programa, que se descarta con el índice 1). Devuelve `argv[1:count]`. Si devuelve NULL, lanza `WinError(get_last_error())` (`:21-22`). Libera con `LocalFree` (`:25-26`).

Reglas de `CommandLineToArgvW` (verificadas ejecutando la función real en este equipo, Windows 11):
- Espacio o tabulador separan argumentos fuera de comillas.
- `"` alterna el modo «entre comillas» y no aparece en el resultado.
- Dentro de comillas, `""` produce un `"` literal y **se mantiene** dentro de comillas.
- Barras invertidas: si hay `2n` seguidas de `"` → `n` barras y la comilla alterna el modo; si hay `2n+1` seguidas de `"` → `n` barras y `"` literal; si no van seguidas de `"`, son literales.
- Un argumento `""` produce una cadena vacía.
- Cadena vacía o solo espacios → lista vacía.
- Comilla sin cerrar: el resto de la cadena pertenece al último argumento.

Ejemplos (entrada → salida, tal como devuelve la función):

| Entrada | Salida |
|---|---|
| `a b` | `['a', 'b']` |
| `"two words"` | `['two words']` |
| `--name "two words"` | `['--name', 'two words']` (caso de `test_cli.py:56-57`) |
| `a"b c"d` | `['ab cd']` |
| `"a\"b"` | `['a"b']` |
| `"C:\Program Files\"` | `['C:\Program Files"']` (la última comilla queda escapada) |
| `"C:\Program Files\\"` | `['C:\Program Files\']` |
| `a\\b` | `['a\\b']` (barras sin comilla: literales) |
| `a\\\"b` | `['a\"b']` |
| `""` | `['']` |
| `"a""b"` | `['a"b']` |
| `   ` | `[]` |
| `"x y" z\"w` | `['x y', 'z"w']` |

## Casos límite y errores
- Sin subcomando equivale a `ui` con `--profile` ausente (`getattr(args, 'profile', None)`, `:188`).
- `ensure_default_profile()` solo para sin subcomando, `ui` y `start` (`:155-157`). Nunca borra el último profile.
- `relocation.check` se ejecuta en todos los subcomandos salvo `bootstrap` (`:158-175`). En `ui` no imprime; en los demás imprime cada mensaje.
- `apply_safely(root)` solo para `shell`, `mcp-server` o cualquier comando con `--owned` (`:176-178`).
- `resume(root)` solo para sin subcomando y `ui` (`:150-153`). Los atajos (shims como git) no reintentan eventos pendientes.
- `flush()` se llama siempre en `finally` (`:346-347`); restaura `threading.excepthook` si se cambió (`:348-350`).
- `bucket`, `shortcut` y `app-shortcut` se ejecutan dentro de `mutation(engine.root)` (`:231`, `:326`, `:330`).
- `run` sin `--bin` llama a `network.intercept` antes de lanzar (`:308-310`). `host-run` lo llama siempre (`:195`).
- `shell` y `run` quitan un `--` inicial solo si es el primer argumento (`:210-211`, `:311`).
- **Duda:** `terminal --new` se parsea (`:86`) pero nunca se usa: `launch(engine, args.profile, args.type)` (`:297`) no lo pasa. ¿Es un bug del original o `--new` está reservado?
- **Duda:** `mito.cmd` comprueba `if not errorlevel 0` (línea 10), que nunca es verdadero, así que el `goto result` de esa rama es código muerto.
- **Duda:** `mito.cmd` sin argumentos ejecuta `start` (`mito.cmd:12-13`), pero el ADR 0008 (punto 2) dice que el ejecutable «muestra la bienvenida». Hay que decidir cuál.

## Invariantes
1. Los argumentos de `release` se validan antes de construir `Engine` (`:113-115`). Sin esa validación `Engine(root)` podría crear estado.
2. `Engine(root)` se construye antes que cualquier otra cosa, salvo la validación de argparse.
3. El código de salida de `run`, `shell`, `terminal`, `start`, `pocketool-run` (sin `--owned`) y `mcp-server` es exactamente el que devuelve el lanzador.
4. `host-run` sin `--owned` siempre devuelve 0 aunque la aplicación falle (`:200`).
5. Con `--owned` siempre hay pausa, incluido código 0 (`test_cli.py:83-94`), y también en error (`:337-338`).
6. Los comandos que modifican el estado compartido (`bucket`, `shortcut`, `app-shortcut`) toman `mutation`.
7. `KeyboardInterrupt` siempre sale con 130.
8. Un error de dominio nunca produce traceback: se imprime `MITO: ...` y sale con 1.
9. `logs` no escribe en el diagnóstico (sale antes de `diagnostics.write`, `:122-125`).
10. `startup.py` nunca imprime traceback para fallos de importación de `mito.__main__`; imprime `MITO: ...` y la ruta del log.

## Casos de test del original

Son los 12 tests de `core/tests/test_cli.py`. Todos cubren `__main__.py` y dependen de `Engine` y de otros módulos mockeados. Ninguno cubre `installed`, `search`, `profile`, `doctor`, `bucket`, `install`, `bootstrap`, `arguments.py` ni `startup.py`.

| # | Test | Qué comprueba | Fase |
|---|---|---|---|
| 1 | `test_release_forwards_kind_and_output` | `release` sin `kind`, con `hotfix`, `minor` y `mayor` reenvía a `publish(engine, 'out', kind=…)` y devuelve 0. Sin `kind` se usa `hotfix`. | F11 |
| 2 | `test_local_release_only_builds_current_version` | `release --local --output out` llama a `build_release(engine, 'out')` y no a `publish`. Devuelve 0. | F11 |
| 3 | `test_invalid_release_arguments_fail_before_engine_initialization` | `['invalid']`, `['mayor','1.0.1']`, `['--local','mayor']`, `['minor','--local']`, `['hotfix','--local']` salen con código 2 y `Engine` no se instancia. | F11 |
| 4 | `test_owned_component_forwards_arguments_and_pauses_on_failure` | `run --owned … -- --name 'two words'` reenvía `['--name','two words']`, `coupled=False`, `owned=True`, `shortcut=None`. Si `launch` devuelve 7, `main` devuelve 7 y pausa con el prompt. | F4 (`--owned`: **Duda**) |
| 5 | `test_shims_do_not_retry_pending_diagnostic_events` | `run` sin `--owned` no llama a `resume`. | F4 |
| 6 | `test_owned_host_forwards_arguments_and_pauses_on_success` | `host-run host-id --profile one --owned -- --port 8080` llama a `host_apps.launch(engine,'one','host-id',['--port','8080'],coupled=False,owned=True)`, devuelve 0 y pausa. | F8 |
| 7 | `test_owned_component_without_arguments_pauses_even_with_zero_exit` | `run --owned` sin argumentos y `launch` devuelve 0: devuelve 0, imprime `La aplicación terminó con el código 0.` y pausa. | F4 |
| 8 | `test_independent_component_does_not_pause` | `run` sin `--owned` devuelve 0 y no pausa. | F4 |
| 9 | `test_owned_start_failure_pauses` | `launch` lanza `OSError('Cannot start')` con `--owned`: `main` devuelve 1 y pausa. | F4 |
| 10 | `test_component_run_forwards_selected_shortcut` | Para `[]`, `['--coupled']` y `['--owned']`, `run … --shortcut 'Tool Help' -- 'two words'` reenvía `shortcut='Tool Help'` y `coupled`/`owned` correctos. Devuelve 0. | F4 (`--coupled`: F8; `--owned`: **Duda**) |
| 11 | `test_application_shortcut_forwards_selected_label` | `app-shortcut extras/tool --profile one --shortcut 'Tool Help'` llama a `application_link(engine,'one','extras/tool',None,coupled=False,arguments=[],shortcut='Tool Help',color=None)`. Devuelve 0. | F8 |
| 12 | `test_application_shortcut_forwards_mode_and_arguments` | `app-shortcut installation --profile one --badge DEV --coupled -- --name 'two words'` llama con `badge='DEV'`, `coupled=True`, `arguments=['--name','two words']`, `shortcut=None`. Devuelve 0. | F8 |

Duda: los tests parchean `mito.__main__.mutation` y `mito.network.intercept` (módulos del original). En Rust
habrá que inyectar esas dependencias de otra forma. No hay test de sintaxis de `mito.cmd`.

## Dependencias
- `engine.Engine` (construcción, `initialize`, `installed`, `set_active`, `shell`, `launch`, `install`, `catalog`, `profiles`, `profile`, `create_profile`, `delete_profile`, `environment`, `ensure_default_profile`).
- `i18n.tr` (`__main__.py:6`; mensajes traducibles: `Pulsa Intro para cerrar`, `No se pudo completar la revisión…`, `No existe el profile`).
- `storage`: `YitoError`, `mutation`, `root_path`, `read_json`.
- `error_tracking`: `capture`, `flush`, `resume`, `report_threads`, `check` (solo `posthog-check`).
- `diagnostics`: `write`, `environment`, `locations`, `message`.
- `path_limit.problem`, `relocation.check/save_notice`, `terminal_identity.apply_safely`.
- `terminal.launch` (`start`, `terminal`), `host_apps.launch`, `network.intercept`.
- `storage_migration.migrate`, `ui.YitoApp`, `pocketool_execution` (`decoded_arguments`, `execute`, `launch`).
- `release.build_release`, `publish.publish`, `shortcuts.mito_link/application_link`, `profile_transfer.export_profile/import_profile`, `search.rank_entries`, `mcp_server.run`.
- Librerías: `argparse`, `json`, `sys`, `subprocess`, `importlib.metadata`, `PIL` (solo `doctor`), `threading`.

## Notas para Rust

Propuestas (sin código):
- **Parser**: `clap` con derive. Un enum `Command` con `#[command(subcommand)]`. Las opciones ocultas con `hide = true` (`--owned`, `--arguments64`, `mcp-server`). Las validaciones de `release` (`--local` con `kind`) como comprobación posterior al parseo que termine con código 2, para que coincida con `parser.error` (`:114-115`).
- **REMAINDER**: `arguments` con `trailing_var_arg = true` y `allow_hyphen_values = true`. Conservar el `--` inicial y quitarlo a mano en `shell`, `run` y `pocketool-run`, igual que el original. Para `host-run` y `app-shortcut` usar `last` o equivalente, y **verificar que el `--` se consume** como en argparse (`test_cli.py:76-80`).
- **Códigos de salida**: `std::process::ExitCode`. Usar 2 para errores de uso, 130 para Ctrl+C (`ctrlc` u otra), 1 para errores de dominio.
- **Mensajes**: replicar `MITO: {mensaje}` en stderr. El texto de los errores de argparse no será idéntico al de Python; decidir si se acepta.
- **Tokenizado**: en Windows, `std::env::args_os()` ya parsea argv. Para `arguments.rs`, `windows-sys` con `CommandLineToArgvW`, o una implementación propia verificada con la tabla de ejemplos de arriba. La tabla es la fuente de verdad.
- **Ruta del proyecto**: `Layout::from_executable` (ADR 0008, punto 2) en lugar de `parents[3]` (`startup.py:5`).
- **Pausa de `--owned`**: `std::io::stdin().read_line` sobre la misma cadena.
- **`mito.cmd`**: en Rust llamará a `core\bin\mito.exe`. Ver tabla siguiente.
- **Duda:** `doctor` y `posthog-check` dependen de Python/Pillow/Textual/mcp. Decidir qué se conserva.
- **Duda:** `bootstrap` en Python ejecuta `core/bootstrap.py`. El ADR 0008 dice que el ejecutable prepara las herramientas; hay que decidir si `bootstrap` sigue como subcomando.
- **Duda:** `start` sin argumentos: el original abre Windows Terminal (`mito.cmd:13`), el ADR dice bienvenida. Elegir.
- **Duda:** `terminal --new` no se usa en el original (`:86`, `:297`).
- **Duda:** `run --owned` y `pocketool-run --owned`: fase F4 o F8/F10. Confirmar.
- **Duda:** `posthog-check` sin fase asignada.
- **Duda:** `profile show` de un profile inexistente lanza `No existe el profile {name}` (`engine.py:91`); el mensaje exacto en Rust debe coincidir.

### mito.cmd y core/commands/mito.cmd

`mito.cmd` (raíz, 24 líneas, CRLF). Qué hace hoy:
1. Línea 3-6: si falta `core\tools\python\mito-ready.json`, ejecuta `powershell.exe … -File core\bootstrap.ps1` y sale con 1 si falla.
2. Línea 7-11: si falta `core\state\bootstrap-tools.json`, ejecuta `python -B -I -X utf8 -m mito.startup bootstrap`. Si falla (`errorlevel 1`) va a `:result`. La línea 10 es código muerto.
3. Línea 12-16: sin argumentos ejecuta `python -m mito.startup start`; con argumentos, `python -m mito.startup %*`.
4. Línea 17-21: `mito_exit` = código de salida. Si es negativo o `9009` (comando no encontrado), salta a `:native_failure`; si no, `exit /b %mito_exit%`.
5. Línea 22-24: `:native_failure` ejecuta `powershell.exe … core\bootstrap.ps1 -DiagnosticExitCode %mito_exit%` y sale con ese código.

`core/commands/mito.cmd` (2 líneas): `@echo off` y `call "%~dp0..\..\mito.cmd" %*`. Es un atajo para ejecutar desde `core\commands`; reenvía todos los argumentos a la raíz.

Propuesta Rust (ADR 0008, punto 2): `mito.cmd` de la raíz pasa a ser `"%~dp0core\bin\mito.exe" %*` con `exit /b` del código devuelto. Se eliminan las líneas de `bootstrap.ps1`, `python.exe` y `mito-ready.json`, porque la preparación de herramientas la hace el ejecutable. `core\commands\mito.cmd` puede quedar igual. Los tests del original que dependen de `bootstrap.ps1` no aplican.
**Duda:** `-DiagnosticExitCode` (`native_failure`) no tiene equivalente en Rust. Decidir si el exe lo acepta.
