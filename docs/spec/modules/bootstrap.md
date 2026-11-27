# bootstrap (`core/bootstrap.ps1`, `core/bootstrap.py`, `core/app/mito/bootstrap_tools.py`, `core/bootstrap.lock.json`) → `mito.exe` (primer arranque)

Origen:
- `core/bootstrap.ps1` (698 líneas): lanzador en Windows PowerShell 5.1. Bienvenida, comprobación de ruta, proxy, log, telemetría y creación del runtime Python.
- `core/bootstrap.py` (64 líneas): se ejecuta con el Python privado. Instala las ruedas y llama a `ensure_tools`.
- `core/app/mito/bootstrap_tools.py` (56 líneas): `ensure_tools(root)`. Prepara 7-Zip, innounp y Windows Terminal.
- `core/bootstrap.lock.json`: hashes y URLs fijados de todo lo descargable.
- Tests del original que dependen de este bloque: `core/tests/test_diagnostics.py` (10 tests: 5 del bootstrap y 5 del log `diagnostics.py`, que el bootstrap usa) y `core/tests/test_bootstrap_proxy.py` (2 tests). Los 4 tests de `test_path_limit.py` están en `path_limit.md`. Ver sección 9.

Decisión que delimita este contrato (ADR 0005 y 0008): en Rust no hay `bootstrap.ps1`, ni `bootstrap.py`, ni Python. `mito.exe` muestra la bienvenida y prepara las herramientas privadas (7-Zip, innounp, Windows Terminal). Esta especificación documenta el comportamiento del original que debe conservarse, indica qué no se porta y deja las dudas marcadas.

Convención de referencias: `bootstrap.ps1:NN` indica línea del `.ps1`; `bootstrap.py:NN` y `bootstrap_tools.py:NN` las de los otros archivos; `__main__.py:NN` y `self_update.py:NN` son los puntos de llamada, fuera del alcance de este contrato.

## Responsabilidad

Preparar, una sola vez y tras confirmación del usuario, las herramientas privadas que MITO necesita y que no se pueden empaquetar en el ZIP mínimo: descargarlas desde URLs inmutables, verificar su SHA-256, extraerlas dentro de la carpeta de MITO y dejar marcadores para no repetir el trabajo en arranques posteriores. En el original también prepara el runtime Python (no se porta).

Flujo general del original:

1. `bootstrap.ps1` valida la ruta, escribe el log y pide confirmación (salvo `-Approved`).
2. Descarga y verifica el ZIP de Python embebido, lo extrae en una carpeta de staging y lo ejecuta.
3. `bootstrap.py` instala las ruedas (no se porta), llama a `ensure_tools` (sí se porta, con 7-Zip, innounp y Windows Terminal) y escribe `mito-ready.json`.
4. `bootstrap.ps1` publica el staging como `core/tools/python`.

## API pública del original

No hay API de biblioteca. Las interfaces son:

- Línea de comandos del lanzador: `bootstrap.ps1 [-Approved] [-PreviewWelcome] [-DiagnosticExitCode N]` (sección 3.5).
- Línea de comandos de Python: `bootstrap.py --target <carpeta>` (`bootstrap.py:21-23`), no se porta.
- Función `ensure_tools(root)` (`bootstrap_tools.py:12`): prepara las herramientas del lock dentro de `root`. Sección 5.3 y 6.
- Lectura del lock: `read_json(root / 'core/bootstrap.lock.json')` (`bootstrap_tools.py:13`).

## Formatos de disco

Resumen. Detalle en la sección 2 (lock), 5.4 (marcadores) y 5.5 (log):

- `core/bootstrap.lock.json` (entrada, sección 2).
- `core/tools/bootstrap.lock` (bloqueo vacío de exclusión, sección 1).
- `core/tools/<id>/.mito-tool.json`, `core/tools/python/mito-ready.json`, `core/state/bootstrap-tools.json` (sección 5.4).
- `logs/mito.log` (JSON por línea) y `logs/pending/<guid>.json` (cola de telemetría, sección 5.5 y 5.8).

## 1. Archivos, carpetas y orden de ejecución

| Paso | Quién | Entrada | Salida |
|---|---|---|---|
| Vista previa de bienvenida | `bootstrap.ps1 -PreviewWelcome` | `bootstrap.lock.json` | Ninguna (sin log) |
| Comprobación de ruta | `bootstrap.ps1` (salvo `-Approved`), `__main__.py` (ui/start/bootstrap) | Ruta de la instancia | Ver `path_limit.md` |
| Confirmación | `bootstrap.ps1` (solo si no existe `mito-ready.json`) | Respuesta del usuario | Continuar o salir con código 2 |
| Bloqueo de preparación | `bootstrap.ps1` | `core/tools/bootstrap.lock` | Exclusión entre procesos |
| Descarga y extracción de Python | `bootstrap.ps1` | `$lock.python` | `core/tools/bootstrap-<guid>/` (staging) |
| Herramientas privadas | `bootstrap.py` → `ensure_tools` | `tools` del lock | `core/tools/7zip/`, `core/tools/windows-terminal/`, `core/tools/innounp/` |
| Marcador Python | `bootstrap.py:52` | — | `mito-ready.json` dentro del staging |
| Publicación | `bootstrap.ps1:678-685` | staging | `core/tools/python/` |

Carpetas que aparecen en el disco:

- `core/tools/` (`bootstrap.ps1:586`, `$tools = Join-Path $PSScriptRoot 'tools'`). Contiene todo lo preparado.
- `core/tools/bootstrap.lock`: archivo de bloqueo que `bootstrap.ps1` abre con `FileShare.None` (`bootstrap.ps1:637`) para impedir dos preparaciones a la vez. Se cierra en `finally` (`:696`).
- `core/tools/bootstrap-<guid32>/`: staging de Python (`bootstrap.ps1:619`). Contiene `python.zip` mientras se descarga (`:645`), luego el runtime. Si falla, **se conserva** para diagnóstico (`:697`).
- `core/tools/python/`: runtime Python publicado. Si ya existía sin `mito-ready.json`, se mueve a `core/tools/python-backup-<guid32>/` (`:680-682`). Ningún código borra después esa copia de seguridad.
- `core/tools/7zip/`, `core/tools/windows-terminal/`, `core/tools/innounp/` (`bootstrap_tools.py:17`, `destination = root / 'core/tools' / tool['id']`).
- `core/tools/<tmp>/`: directorio temporal de `ensure_tools` creado con `tempfile.TemporaryDirectory(dir=destination.parent)` (`bootstrap_tools.py:25`). Lo borra Python al salir, también si falla.
- `core/wheels/`: caché de ruedas Python (`bootstrap.py:29-30`). No se porta.
- `core/state/bootstrap-tools.json` (`bootstrap_tools.py:56`) y `core/state/mutation.lock` (`bootstrap_tools.py:14`, ver `storage.md`).
- `logs/mito.log` y `logs/pending/` (ver sección 5.5 y 5.8).

## 2. `bootstrap.lock.json`: formato exacto

### 2.1 Estructura de primer nivel

```json
{
  "schema": 1,
  "python": { "url": "...", "sha256": "..." },
  "wheels": [ { "filename": "...", "url": "...", "sha256": "..." }, "..." ],
  "tools": [ { "id": "...", "version": "...", "required": ["..."], "artifacts": [ { "fileName": "...", "url": "...", "sha256": "...", "install": { "type": "...", ... } } ] } ]
}
```

- `schema`: `1`. Ningún código del original lo valida. Duda: ¿Rust debe comprobarlo y rechazar otro valor?
- `python`, `wheels`: solo para el runtime Python. Ver 2.3.
- `tools`: el único bloque que se porta. El orden del array es el orden de preparación en `bootstrap_tools.py:15` y el orden de la bienvenida (`bootstrap.ps1:10`).
- Campos de una herramienta: `id`, `version`, `required` (rutas relativas al destino que deben existir como fichero), `artifacts` (lista en orden de descarga).
- Campos de un artefacto: `fileName` (nombre local dentro del staging), `url`, `sha256` (en minúsculas, 64 caracteres hex), `install`.
- Tipos de `install.type` que reconoce el código (`bootstrap_tools.py:34-46`):
  - `file`: copia el archivo descargado a `payload/<target>` con `copy2`. Si no hay `target`, usa `fileName` (`:35`).
  - `zip`: extrae el ZIP en `stage/expanded` y copia `expanded/<source>` a `payload` con `copytree(dirs_exist_ok=True)` (`:36-40`). `source` por defecto `.`.
  - `sevenZip`: ejecuta `payload/<executable> x <archivo> -o<payload> -y` con `check=True`, `capture_output=True` y `CREATE_NO_WINDOW` (`:41-44`).
  - Cualquier otro valor: `YitoError('Formato de herramienta core desconocido')` (`:45-46`).
- Los campos `destination` que aparecen en el JSON no los lee ningún código. Duda: ¿se mantienen en el formato o se eliminan? (ver sección 11).

### 2.2 Entradas de herramientas (se portan)

Copia literal de `core/bootstrap.lock.json`, bloque `tools`. No se ha modificado ningún valor.

**7-Zip** (`tools[0]`, `id` = `7zip`, `version` = `26.02`). Dos artefactos: `7zr.exe` (instala con `file`) y el instalador `7z2602-x64.exe` (extrae con `sevenZip` usando `7zr.exe`).

```json
{
  "id": "7zip",
  "version": "26.02",
  "required": [
    "7z.exe",
    "7z.dll"
  ],
  "artifacts": [
    {
      "fileName": "7zr.exe",
      "url": "https://github.com/ip7z/7zip/releases/download/26.02/7zr.exe",
      "sha256": "56b8cc9f4971cef253644fafe54063ed7fdca551d4dee0f8c6baa81b855acd72",
      "install": {
        "type": "file",
        "destination": ".",
        "target": "7zr.exe"
      }
    },
    {
      "fileName": "7z2602-x64.exe",
      "url": "https://github.com/ip7z/7zip/releases/download/26.02/7z2602-x64.exe",
      "sha256": "6745fa76dc2ea031596d8678f6f6b99c3c1b435b4164a63485adbbc7b8d82ef0",
      "install": {
        "type": "sevenZip",
        "destination": ".",
        "executable": "7zr.exe"
      }
    }
  ]
}
```

**Windows Terminal** (`tools[1]`, `id` = `windows-terminal`, `version` = `1.24.11911.0`). Un artefacto ZIP con subcarpeta `terminal-1.24.11911.0`.

```json
{
  "id": "windows-terminal",
  "version": "1.24.11911.0",
  "required": [
    "WindowsTerminal.exe",
    "wt.exe"
  ],
  "artifacts": [
    {
      "fileName": "Microsoft.WindowsTerminal_1.24.11911.0_x64.zip",
      "url": "https://github.com/microsoft/terminal/releases/download/v1.24.11911.0/Microsoft.WindowsTerminal_1.24.11911.0_x64.zip",
      "sha256": "7691efeb71c8dd0b95536c84e366fa4cf809a42c534912f9cefa1056534383bd",
      "install": {
        "type": "zip",
        "destination": ".",
        "source": "terminal-1.24.11911.0"
      }
    }
  ]
}
```

Paso específico tras copiar: si `id` es `windows-terminal` se crea `payload/.portable` vacío (`bootstrap_tools.py:49-50`).

**innounp** (`tools[2]`, `id` = `innounp`, `version` = `2.71.1`). Un artefacto ZIP, sin `source` ni `target` (se copia la raíz del ZIP, `source` = `.`).

```json
{
  "id": "innounp",
  "version": "2.71.1",
  "required": [
    "innounp.exe",
    "innounp.htm"
  ],
  "artifacts": [
    {
      "fileName": "innounp-2.71.1.zip",
      "url": "https://raw.githubusercontent.com/jrathlev/InnoUnpacker-Windows-GUI/6fb49264aacf512a093e7b4fc6fb3dd266dad31a/innounp-2/bin/innounp-2.zip",
      "sha256": "f2f037fdbc63de31248efae9ccb294398d160dc0cbad5f36d14c2f159e17bbf5",
      "install": {
        "type": "zip",
        "source": "."
      }
    }
  ]
}
```

Resumen de la tabla de herramientas:

| id | version | Artefactos | Destino (`core/tools/<id>/`) | Extracción | `required` |
|---|---|---|---|---|---|
| `7zip` | 26.02 | `7zr.exe` (SHA-256 `56b8cc9f...acd72`), `7z2602-x64.exe` (SHA-256 `6745fa76...82ef0`) | `core/tools/7zip/` | `7zr.exe` se copia; el instalador se extrae con `7zr.exe x` | `7z.exe`, `7z.dll` |
| `windows-terminal` | 1.24.11911.0 | `Microsoft.WindowsTerminal_1.24.11911.0_x64.zip` (SHA-256 `7691efeb...383bd`) | `core/tools/windows-terminal/` | ZIP nativo, se copia `terminal-1.24.11911.0/`; se crea `.portable` | `WindowsTerminal.exe`, `wt.exe` |
| `innounp` | 2.71.1 | `innounp-2.71.1.zip` (SHA-256 `f2f037fd...7bbf5`) | `core/tools/innounp/` | ZIP nativo, se copia la raíz | `innounp.exe`, `innounp.htm` |

Tamaño: el lock no tiene ningún campo de tamaño. Duda: ¿hay que añadir `size` a cada artefacto para verificar antes del hash, o basta el SHA-256?

Nota sobre ADR 0005: en Rust, el ZIP de Windows Terminal y el de innounp pueden extraerse con la crate nativa (`mito_transfer::archives`). El instalador 7z sigue necesitando `7zr.exe`, como en el original, porque `7z2602-x64.exe` no es un ZIP.

### 2.3 Entradas de Python y ruedas (no se portan)

- `python`: `url` = `https://www.python.org/ftp/python/3.14.7/python-3.14.7-embed-amd64.zip`, `sha256` = `d297e5ff019966817ad8502465176139f2d3d840fa4ed84b13bed399a6ab1f15`. Usada por `bootstrap.ps1:650-659`.
- `wheels`: 39 entradas (`linkify_it_py-2.2.0`, `markdown_it_py-4.2.0`, `mdit_py_plugins-0.6.1`, `mdurl-0.1.2`, `pillow-12.3.0-cp314`, `platformdirs-4.11.7`, `pygments-2.21.0`, `rich-15.0.0`, `textual-8.2.8`, `typing_extensions-4.16.0`, `mcp-1.30.0`, `httpx-0.28.1`, `httpcore-1.0.9`, `pydantic-2.13.5`, `pydantic_core-2.46.5`, `annotated_types-0.8.0`, `anyio-4.15.1`, `h11-0.16.0`, `httpx_sse-0.4.3`, `idna-3.19`, `jsonschema-4.26.0`, `attrs-26.1.0`, `jsonschema_specifications-2025.9.1`, `pydantic_settings-2.15.0`, `pyjwt-2.14.0`, `cryptography-50.0.1`, `cffi-2.1.1`, `python_dotenv-1.2.3`, `python_multipart-0.0.32`, `pywin32-312`, `referencing-0.37.0`, `rpds_py-2026.6.3`, `sse_starlette-3.4.11`, `starlette-1.6.0`, `typing_inspection-0.4.4`, `uvicorn-0.52.4`, `click-8.5.0`, `certifi-2026.7.22`, `pycparser-3.0`). Cada una tiene `filename`, `url` (files.pythonhosted.org) y `sha256`. Se usan solo en `bootstrap.py:31-41`.
- Su consumidor (Textual, Pillow, SDK MCP, ruta `._pth`) desaparece en Rust. Las dependencias de la interfaz están compiladas en `mito.exe`.

## 3. Bienvenida y confirmación

### 3.1 Forma

Es una consola, no una ventana WinForms. `Write-Host` escribe en la consola de Windows PowerShell que lanza `mito.cmd` y la respuesta llega con `Read-Host` (`bootstrap.ps1:24`). Colores: título en cian (`-ForegroundColor Cyan`, `:6`), aviso de Internet en amarillo (`:21`). Los demás mensajes van sin color.

Ningún `System.Windows.Forms` aparece en `bootstrap.ps1`. Los diálogos WinForms del original están en otros módulos (`host_apps.py`, `host_data.py`, `network.py`, `profile_files_ui.py`, `profile_transfer_ui.py`), no en el bootstrap.

### 3.2 Texto literal

Salida exacta, en orden, para el lock actual. Las líneas en blanco son `Write-Host ''`. Los números de versión salen de `$tool.version`.

```
(línea vacía)
Bienvenido a MITO (My Integrated Terminal Orchestrator)
(línea vacía)
Para preparar MITO se descargarán las siguientes herramientas:
  - Python
  - 7-Zip 26.02
  - Windows Terminal 1.24.11911.0
  - innounp (extracción de instaladores Inno Setup) 2.71.1
  - Textual, Pillow, SDK MCP y sus bibliotecas necesarias
(línea vacía)
Es necesaria una conexión a Internet.
Las herramientas se guardarán dentro de esta carpeta de MITO.
(línea vacía)
¿Deseas continuar? [s/N]
```

Origen de cada línea: `bootstrap.ps1:5-24`. Las líneas `  - Python` (`:9`) y `  - Textual, Pillow, SDK MCP ...` (`:19`) son fijas. Las de herramientas salen de `:10-18`. La etiqueta de cada herramienta: `7zip` → `7-Zip`, `windows-terminal` → `Windows Terminal`, `innounp` → `innounp (extracción de instaladores Inno Setup)`; cualquier otro `id` se muestra tal cual (`:11-16`).

Duda: en Rust, las líneas de Python y de Textual/Pillow/SDK MCP no tienen sentido. ¿Se muestra solo la lista de tres herramientas, o se mantiene el texto literal por compatibilidad visual? Recomendación: suprimirlas, porque mostrar Python sería falso.

### 3.3 Respuestas

- Entrada `Read-Host` (`:24`). Se acepta, sin distinguir mayúsculas ni espacios alrededor (`:26`): `s`, `si`, `sí`. Cualquier otra cosa cuenta como no, incluida la cadena vacía. La opción por defecto es **no** (`[s/N]`).
- Si `Read-Host` lanza excepción (no hay consola interactiva), la función devuelve `$false` (`:25`), es decir, se cancela.
- `-Approved` salta la pregunta por completo (`bootstrap.ps1:627`, `if (-not $Approved -and -not (Confirm-YitoBootstrap $lock))`).

### 3.4 Qué pasa al cancelar

1. Registro: `info`, fase `confirmation`, mensaje `Bootstrap cancelled before downloading` (`:628`).
2. Consola: `Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.` (`:629`).
3. Código de salida `2` (`:630`).
4. No se crea `core/tools` (`:635` no se ha ejecutado todavía), no se toca la red, no se escriben pendientes de telemetría. Lo verifica el test `test_bootstrap_cancellation_creates_no_tools_or_pending_events` (`test_diagnostics.py:90`).

### 3.5 Parámetros

| Parámetro | Efecto | Origen |
|---|---|---|
| `-Approved` | Salta la bienvenida, salta la comprobación de ruta (`:604`) y hace el proxy no interactivo (`:650`: `Interactive = -not $Approved`). Lo usa el actualizador (`self_update.py:266`), que prepara en una carpeta temporal del host. | `:2`, `:604`, `:627`, `:649-650` |
| `-PreviewWelcome` | Lee el lock, muestra la bienvenida y pregunta. La respuesta se descarta. Escribe `Vista previa terminada. No se ha descargado ni preparado ninguna herramienta.` y sale con `0`. No escribe log ni crea nada. | `:588-593` |
| `-DiagnosticExitCode N` | Si `N` no es `0`: registra error de fase `runtime-native-exit` con el mensaje `Python could not start or terminated abnormally. Exit code: N`, escribe `MITO: log: <ruta>`, envía pendientes y sale con `1`. Lo llama el ejecutable nativo cuando Python falla al arrancar. | `:2`, `:597-603` |

Duda: `-DiagnosticExitCode` solo tiene sentido con un Python que falla al arrancar. En Rust no existe ese proceso. ¿Se elimina o se conserva para diagnósticos de `mito.exe`? Recomendación: no portar, documentar como no aplica.

### 3.6 Cuándo se muestra y cuándo se repite

- Se muestra solo si no existe `core/tools/python/mito-ready.json` (`bootstrap.ps1:623`, comprobación antes de `:626`) y no se pasó `-Approved`. Se comprueba una segunda vez tras tomar el bloqueo (`:639-643`), por si otra instancia terminó mientras esta esperaba.
- Se repite en cada ejecución mientras `mito-ready.json` no exista. Esto incluye: tras cancelar (código 2), tras un fallo (código 1) y tras borrar la carpeta `core/tools/python`.
- Si `mito-ready.json` existe, la bienvenida no aparece y se registra `info`, fase `ready`, mensaje `Python already prepared` (`:640`), con código `0`.
- Si la comprobación de ruta falla (`:604-615`), la bienvenida no se llega a mostrar (ver `path_limit.md`).

Duda: en Rust, ¿la bienvenida debe depender solo de `mito-ready.json` (como el original) o de que falte alguna herramienta de `tools`? El original, con Python ya listo, nunca vuelve a preguntar aunque falte una herramienta (ver sección 6).

## 4. Comprobación de la longitud de la raíz

Se ejecuta antes de nada (`bootstrap.ps1:604-615`), salvo con `-Approved`. La regla completa, el umbral de 32 caracteres, la lectura de `LongPathsEnabled`, el mensaje literal y los comandos que bloquea están en `path_limit.md`. Aquí solo se recuerda el resultado del lanzador: registro `error`, fase `path-limit` (`:608`), consola `MITO: <mensaje>` en rojo (`:610`), `Pulsa Intro para cerrar` (`:612`) y salida con código `3` (`:613`).

## 5. Flujo de preparación

### 5.1 Inicialización

Orden exacto en `bootstrap.ps1:595-617`:

1. `Initialize-YitoDiagnostics $root` (`:595`): raíz = padre de `core/` (`:585`); carpetas de log (ver 5.5); identidad del log = los 16 primeros caracteres hex del SHA-256 de la raíz en minúsculas (`:77`); `config.properties` leído a un diccionario, ignorando líneas que empiezan por `#` o `!` (`:83-86`); versión leída de `core/app/mito/__init__.py` con `__version__ = "X.Y.Z"` (`:88-89`), por defecto `unknown` (`:80`).
2. `Net.ServicePointManager` con `Tls12` (`:596`).
3. `-DiagnosticExitCode` (`:597`).
4. Comprobación de ruta (`:604`).
5. Registro `info`, fase `startup`, mensaje `Bootstrap started` con `windows` (versión de Windows), `os64`, `bits`, `powershell` (`:617`).

### 5.2 Pasos principales (try, `:620-694`)

| Fase | Acción | Error literal si falla |
|---|---|---|
| `startup` | Si no es 64 bits: `throw 'MITO requires Windows x64'` (`:621`). | `MITO requires Windows x64` (inglés, literal) |
| `read-bootstrap-lock` | Lee `bootstrap.lock.json` con `ConvertFrom-Json` (`:625`). | Error de JSON, mensaje de .NET |
| `confirmation` | Pregunta (sección 3). | — (cancela con código 2) |
| `create-tools-directory` | `New-Item -Force` para `core/tools` (`:635`). | — |
| `bootstrap-lock` | Abre `core/tools/bootstrap.lock` con `FileShare.None` (`:637`). | — |
| (re-comprobación) | Si `mito-ready.json` existe: `Python already prepared`, código `0` (`:639-643`). | — |
| (staging) | Crea `core/tools/bootstrap-<guid>` (`:644`), escribe `MITO: preparando Python portable, Textual y Pillow...` (`:646`). | — |
| `proxy` | `Initialize-YitoProxy` (sección 5.7). Devuelve los argumentos de proxy de `Invoke-WebRequest`. | Ver 5.7 |
| `download-python` | `Invoke-WebRequest -UseBasicParsing -Uri <python.url> -OutFile python.zip` (`:653`). | Error de red de PowerShell |
| `verify-python-hash` | SHA-256 del ZIP y comparación con `python.sha256` (`:654-659`). | `Hash de Python incorrecto` (literal) |
| `extract-python` | `ZipFile.ExtractToDirectory(archive, stage)` y borra `python.zip` (`:660-664`). | Error de .NET |
| `run-python-bootstrap` | `python.exe -B -I -X utf8 bootstrap.py --target <stage>` con stdout y stderr enviados al log (`:671-675`). | `No se pudo preparar el runtime. Python exit code: N` (literal, `:677`) |
| `publish-python` | Si existe `core/tools/python`: se mueve a `python-backup-<guid>`; luego se mueve el staging a `core/tools/python` (`:678-685`). | `Unexpected Python destination` (`:681`), `Unexpected Python staging path` (`:684`) |
| `complete` | Registro `info` `Bootstrap completed` (`:686`) y envío de pendientes (`:687`). | — |

Salida al terminar bien: código `0` implícito del script (no hay `exit`). Duda: el original no escribe nada en consola al completar. ¿Mostrar un mensaje de éxito en Rust?

Error: el bloque `catch` (`:688-694`) guarda el error en el log con la fase actual, escribe en rojo `MITO: <mensaje>`, escribe `MITO: log: <ruta del log activo>`, envía pendientes y sale con código `1`. El staging **no** se borra (`:697`).

### 5.3 Preparación de herramientas (`ensure_tools`, dentro de `bootstrap.py`)

`bootstrap.py` se ejecuta con el Python del staging (`bootstrap.ps1:671`) y, al final, llama a `ensure_tools(core.parent)` (`bootstrap.py:50`). Por cada herramienta del lock en orden (`bootstrap_tools.py:15`):

1. Registro `info`, operación `bootstrap.tools`, mensaje `Preparing tool`, componente = `id`. Se escribe **antes** de comprobar nada, así que aparece en cada llamada (`:16`).
2. Si `core/tools/<id>/.mito-tool.json` existe, su campo `version` coincide con la del lock y todos los `required` existen como fichero dentro de `core/tools/<id>/`: la herramienta se da por buena y se pasa a la siguiente (`:18-20`).
3. Si `core/tools/<id>/` existe (pero no pasó el paso 2): `YitoError('Herramienta incompleta: <ruta>. Conservada para revisar antes de sustituirla.')` (`:21-22`). Se aborta toda la preparación. Lo es también un cambio de versión.
4. Si no existe: imprime `Preparando <id> <version>…` (`:23`), crea el padre y un directorio temporal `tmpXXXX` dentro de `core/tools/` (`:24-25`).
5. Dentro del temporal: `payload/` (`:26-28`). Por cada artefacto en orden:
   - Descarga y verificación: `download(url, stage/<fileName>, sha256, root=root)` dentro de `step(root, 'bootstrap.tools', 'download-and-hash', <id>)` (`:30-32`). El SHA-256 se comprueba en `mito.transfer.download` (fuera de alcance; ver Duda).
   - Extracción según `install.type` (sección 2.1), dentro de `step(..., 'extract-zip' | 'extract-7zip', <id>)` (`:38`, `:43`).
6. Si falta algún `required` en `payload`: `YitoError('Herramienta incompleta: <id>')` (`:47-48`).
7. Solo para `windows-terminal`: crea `payload/.portable` (`:49-50`).
8. Escribe `payload/.mito-tool.json` con `{"version": "<version>"}` (`:51`).
9. `payload.rename(destination)` (`:52`). Es el único paso que publica la herramienta.

Tras el bucle, dentro del mismo bloqueo `mutation(root)` (`:14`):

10. `brand_window_icons(root)` (`terminal.py`, `:54-55`). Fuera de alcance.
11. Escribe `core/state/bootstrap-tools.json` con `{"lock_sha256": "<SHA-256 del bootstrap.lock.json>"}` (`:56`).

Mensajes de consola: solo `Preparando <id> <version>…` (con el carácter de elipsis `…`, U+2026), y solo cuando hay descarga.

### 5.4 Marcadores

| Archivo | Lo escribe | Contenido (formato de `write_json`) | Lo lee |
|---|---|---|---|
| `core/tools/python/mito-ready.json` | `bootstrap.py:52` (en el staging, antes de publicarlo) | `{"schema": 1, "lock_sha256": "<hex>"}` | `bootstrap.ps1:623`, `:639` (solo existencia) |
| `core/tools/<id>/.mito-tool.json` | `bootstrap_tools.py:51` | `{"version": "<version>"}` | `bootstrap_tools.py:19` (versión), `__main__.py:224-225` (`doctor`, para `windows-terminal` y `7zip`) |
| `core/state/bootstrap-tools.json` | `bootstrap_tools.py:56` | `{"lock_sha256": "<hex>"}` | Ningún código dentro del alcance. `self_update.py:183` y `:417` lo tratan como artefacto preparado. |

Formato de `write_json` (ver `storage.md`): UTF-8 sin BOM, `indent=2`, fin de línea `\n`, claves en orden de inserción, `ensure_ascii=False`. Ejemplo exacto de `.mito-tool.json` para 7-Zip:

```
{
  "version": "26.02"
}
```

(seguido de `\n`). `mito-ready.json` escribe primero `schema` y luego `lock_sha256`.

Duda: `mito-ready.json` guarda el hash del lock, pero ningún código lo compara. Si cambia el lock (por ejemplo, una versión nueva de 7-Zip), `bootstrap.ps1` no vuelve a preparar Python, y `ensure_tools` solo detecta el cambio por `version` de cada herramienta. Ver sección 6.

### 5.5 Registro (log)

Formato de cada línea (`bootstrap.ps1:112-114`): JSON compacto, una línea por evento, terminada en `\n`, UTF-8 sin BOM. Campos: `time` (UTC ISO 8601 con `o`), `level` (`info`, `warning` o `error`), `pid`, `operation` (`bootstrap.powershell` en el lanzador, `bootstrap.python` y `bootstrap.tools` en Python), `phase`, `message` (ya saneado), `version`, y los `Details` del evento. Ejemplo de la forma (valores ficticios):

```
{"time":"2026-11-27T10:00:00.0000000Z","level":"info","pid":1234,"operation":"bootstrap.powershell","phase":"startup","message":"Bootstrap started","version":"0.3.1","windows":"10.0.26300.0","os64":true,"bits":64,"powershell":"5.1.26100.1"}
```

Carpetas de log, en este orden (`:78`): `<raíz>/logs/` y `%TEMP%\MITO-logs\<id16>/`. Si la primera no se puede escribir (cerrojo `.diagnostics.lock` con 10 intentos de 20 ms, `:95-102`), se usa la siguiente. Cuando se usa la alternativa, la primera vez se escribe en `stderr` `MITO: log alternativo: <ruta>` (`:131`). Si ninguna funciona, `MITO: cannot write local or temporary diagnostic log.` en `stderr` (`:136`). Escribir el log nunca lanza excepción (`:137`).

Rotación (`:119-125`): si el fichero actual más la línea nueva superan 1 MiB, se borra `mito.log.3`, se mueve `.2` a `.3`, `.1` a `.2` y `mito.log` a `.1`. Se conservan tres copias.

Eventos registrados por el lanzador (todos `bootstrap.powershell`):

| Fase | Nivel | Mensaje | Línea |
|---|---|---|---|
| `startup` | info | `Bootstrap started` (+ windows, os64, bits, powershell) | `:617` |
| `path-limit` | error | Mensaje de `path_limit.md` | `:608` |
| `runtime-native-exit` | error | `Python could not start or terminated abnormally. Exit code: N` | `:598-599` |
| `confirmation` | info | `Bootstrap cancelled before downloading` | `:628` |
| `confirmation` | info | `Bootstrap approved` | `:632` |
| `ready` | info | `Python already prepared` | `:640` |
| `proxy` | info | `Proxy disabled by config.properties` | `:455` |
| `proxy` | info | `No system proxy detected; direct connection` | `:471` |
| `proxy` | info | `Proxy <url> from <origen>: <tipo>` (+ status, authenticate, network_error) | `:501` |
| `proxy` | warning | `Ignoring HTTPS_PROXY with credentials or invalid URL` | `:467` |
| `proxy` | info | `Detected proxy saved to config.properties` | `:538` |
| `proxy` | warning | Mensaje de excepción al guardar config | `:541` |
| `proxy-authentication` | info | `Portal authentication verified` | `:571` |
| `proxy-authentication` | warning | `Portal authentication not verified` | `:575` |
| `proxy-authentication` | warning | Mensaje de excepción del portal | `:566` |
| `download-python` | info | `Downloading pinned Python archive` | `:652` |
| `extract-python` | info | `Extracting verified Python archive` | `:661` |
| `run-python-bootstrap` | info | `Starting private Python` | `:666` |
| `python-output` | info | Cada línea de la salida de `bootstrap.py` | `:672` |
| `complete` | info | `Bootstrap completed` | `:686` |
| (la fase en curso) | error | Mensaje de excepción, con `event_id`, `exception`, `trace`, `windows`, `bits`, `powershell` | `:140-144` |
| `posthog.queue` | warning | Excepción al encolar | `:176` |
| `posthog` | info | `Event accepted: <uuid>` | `:222` |
| `posthog` | warning | Mensaje de excepción al enviar | `:224`, `:229` |

Eventos del bloque Python (`operation` = `bootstrap.python` o `bootstrap.tools`):

- `bootstrap.python`: `Bootstrap Python started` con `runtime` (`bootstrap.py:25`); un evento por rueda con el nombre de fichero como mensaje y fase `download-wheel` (`:34`); `Bootstrap Python completed` (`:53`). No se porta.
- `bootstrap.tools`: `Preparing tool` por herramienta en cada llamada (`bootstrap_tools.py:16`); y las fases `download-and-hash`, `extract-zip`, `extract-7zip` por `step()` (`:31`, `:38`, `:43`). El comportamiento exacto de `step()` está en `diagnostics.py` (fuera de alcance).
- Errores de Python: `capture(...)` con `operation='bootstrap.python'`, `phase` y `component` (`bootstrap.py:59-61`), que va a la cola de errores de Python. No se porta (ver 5.8).

### 5.6 Errores y mensajes literales

Todos los mensajes que el usuario ve, por orden de aparición en el código:

| Mensaje | Origen | Cuándo |
|---|---|---|
| `Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.` | `bootstrap.ps1:629` | Respuesta no afirmativa a la bienvenida |
| `La carpeta de MITO tiene una ruta demasiado larga ...` | `:48-50`, ver `path_limit.md` | Ruta > 32 sin `LongPathsEnabled` |
| `Pulsa Intro para cerrar` | `:612` | Tras el mensaje de ruta |
| `MITO requires Windows x64` | `:621` | Sistema de 32 bits (en inglés, literal) |
| `Hash de Python incorrecto` | `:659` | SHA-256 del ZIP no coincide |
| `No se pudo preparar el runtime. Python exit code: <N>` | `:677` | `bootstrap.py` sale con código distinto de `0` |
| `Unexpected Python destination` | `:681` | La ruta destino no es `core\tools\python` |
| `Unexpected Python staging path` | `:684` | El staging no está dentro de `core/tools` |
| `MITO: <mensaje>` (en rojo) | `:691` | Cualquier excepción del bloque `try` |
| `MITO: log: <ruta>` | `:692`, `:600` | Al terminar con error o con `-DiagnosticExitCode` |
| `Herramienta incompleta: <ruta>. Conservada para revisar antes de sustituirla.` | `bootstrap_tools.py:22` | Destino existe sin marcador válido |
| `Herramienta incompleta: <id>` | `bootstrap_tools.py:48` | Tras extraer, falta un `required` |
| `Formato de herramienta core desconocido` | `bootstrap_tools.py:46` | `install.type` no reconocido |
| `Preparando <id> <version>…` | `bootstrap_tools.py:23` | Inicio de la descarga de una herramienta |
| `Preparando <fichero>` | `bootstrap.py:37` | Descarga de una rueda (no se porta) |
| `Child process failed with exit code <N>` | `diagnostics.message` (test `test_diagnostics.py:62`) | Error de 7zr (`CalledProcessError`) sin mostrar argumentos |

Nota: los `CalledProcessError` de `7zr` no se envuelven en `YitoError`. El mensaje que llega al usuario depende de `diagnostics.message`, que no muestra los argumentos del comando (test `test_diagnostics.py:62-64`).

### 5.7 Proxy

Python no evalúa PAC ni WPAD. El lanzador resuelve el proxy con Windows y lo entrega a Python por `config.properties` o por variables de entorno (`bootstrap.ps1:232-234`). Resumen de comportamiento:

- Si `config.properties` contiene `https_proxy` o `http_proxy` (`:447`):
  - Si no hay `https_proxy`: devuelve sin proxy (`:450`).
  - Si `https_proxy` está vacío: desactiva el proxy de Windows (`[Net.WebRequest]::DefaultWebProxy = $null`, `:454`), registra `Proxy disabled by config.properties` y sale sin proxy (`:455-456`).
  - Si tiene valor: se valida como URL HTTP/HTTPS sin credenciales (`Test-YitoHttpUrl`, `:458`, mensaje `Proxy: usa una URL HTTP/HTTPS sin credenciales`).
- Si no hay configuración de proxy (`:460-478`):
  1. Variable de entorno `HTTPS_PROXY` (`:462-468`). Si tiene credenciales o no es URL válida, se ignora con un aviso.
  2. Si no, `Get-YitoSystemProxy` (`:469`, `:258-275`): comprueba la lista de exclusión con `GetSystemWebProxy().IsBypassed`, obtiene el proxy de Windows y clasifica el origen como `configuración manual de Windows` (`ProxyEnable=1` y `ProxyServer`), `script PAC <url>` (`AutoConfigURL`) o `autodetección WPAD`. Las claves salen de `HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings` (`:261`).
  3. Si no hay proxy: `No system proxy detected; direct connection` y sale sin proxy (`:471-472`).
  4. Si hay: `MITO: proxy detectado: <url> (<origen>)` en cian (`:477`).
- La lista `NO_PROXY` se construye con `localhost`, `127.0.0.1`, `::1` y los sufijos de `ProxyOverride` que no tengan `*` ni `/` (`ConvertTo-YitoNoProxy`, `:244-256`).
- Sonda: `Invoke-YitoHttp` a `network.check_url` (por defecto `https://www.gstatic.com/generate_204`, `:444`) con `network.check_status` (por defecto `204`, `:445`). Sin seguir redirecciones (`:281`), con `Timeout` y `ReadWriteTimeout` de 8000 ms (`:282-283`) y `User-Agent: MITO-bootstrap` (`:284`).
- Clasificación (`Get-YitoProxyKind`, `:327-333`): `ok` si el estado es el esperado; `auth` si 407; `portal` si 301/302/303/307/308 con `Location`; `unreachable` si 0; `blocked` en el resto. Si la URL es HTTPS y no es `ok`, se reintenta por HTTP para detectar portales (`:483-491`). Si el 407 anuncia `NTLM` o `Negotiate`, se reintenta con las credenciales de Windows (`:492-495`) y el resultado es `windows`.
- Mensajes por tipo (`:503-508`):
  - `ok`: `MITO: el proxy permite el acceso a Internet.`
  - `windows`: `MITO: el proxy acepta tu sesión de Windows (NTLM/Kerberos). Python no puede usarla: algunas descargas podrían fallar con HTTP 407.`
  - `auth`: `MITO: el proxy exige autenticación HTTP 407 (<cabecera>), que MITO no implementa.`
  - `portal`: `MITO: el proxy redirige a un portal de autenticación: <url sin credenciales ni query>`
  - `blocked`: `MITO: la comprobación de Internet a través del proxy respondió HTTP <N>.`
  - otro: `MITO: no se pudo comprobar Internet a través del proxy (<error>).`
- Guardado (solo si es interactivo y se detectó un proxy nuevo, `:511-552`): muestra las claves que se añadirán (`:526-530`), pide `¿Guardar esta configuración de red? [S/n]` (`:533`; por defecto **sí**; acepta `s`, `si`, `sí`, `y`, `yes`) y escribe con `Set-YitoProperties` (`:535`). Se escriben `http_proxy`, `https_proxy` y `no_proxy`, y si hay portal, `enable_proxy_authentication=true` y `proxy_authentication_url`, más los nombres de campo si no son `username` ni `password` (`:512-522`). Las contraseñas **nunca** se guardan.
- Si no se guarda: `MITO: se usará el proxy detectado sólo durante esta preparación.` (`:546`) y se fijan `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` para el proceso (`:548-550`).
- Portal (`:555-577`, solo interactivo, con `enable_proxy_authentication=true` y `proxy_authentication_url`, y si el estado no es `ok` ni `windows`): hasta 3 intentos. Pide `Usuario` (`:559`); si se deja vacío, sale. Pide `Contraseña` como `SecureString` (`:561`). Si `Invoke-YitoPortalLogin` devuelve `$true`: `MITO: autenticación correcta.` (verde). Si no: `MITO: no se pudo verificar Internet. Revisa usuario y contraseña.` (`:574`).
- Login de portal (`Invoke-YitoPortalLogin`, `:399-438`): lee la página del portal sin cookies; localiza el primer formulario con campo `password` (o el nombre configurado); usa como destino el `action` del formulario, que debe tener el **mismo origen** (`:417`: `El formulario apunta a otro origen; configura su URL explícitamente`); exige HTTPS salvo `proxy_authentication_require_https=false` (`:418-420`: `El POST requiere HTTPS; configura proxy_authentication_require_https=false para un portal HTTP`); envía los campos ocultos, los `proxy_authentication_form_field.*` y el usuario y contraseña; repite la sonda de comprobación sin cookies. La contraseña se pone a `null` y se libera el BSTR en `finally` (`:430-434`).
- El valor `proxy_authentication_url` debe ser HTTP/HTTPS con host y sin credenciales (`Test-YitoHttpUrl`, `:236-242`).

### 5.8 Telemetría de errores (PostHog) y saneado

Solo se envía si `config.properties` tiene `posthog.enabled=true` (`:146`, `:181`) y `posthog.host` y `posthog.api_key` con valor (`:147-149`, `:182-184`). La URL del host debe ser `http` o `https` sin credenciales, sin query y sin fragmento (`:186`).

- Al producirse un error (`Save-YitoBootstrapError`, `:140-177`), el evento se encola como JSON en `logs/pending/<guid32>.json` (`:169-171`), escrito a `.tmp` y renombrado.
- Antes de encolar: borra temporales huérfanos `<guid32>.tmp` (`:160-161`), borra los de más de 7 días o de más de 128 KiB (`:165`), y si quedan 100 o más, borra los más antiguos (`:168`).
- El envío (`Send-YitoBootstrapPending`, `:179-230`) procesa hasta 3 eventos por ejecución (`:193`) con `timeout` de 3000 ms (`:206`). Un evento se borra solo si el servidor responde 2xx (`:219`, `:221`). Tampoco se envía si `destination` (SHA-256 de `host + "\n" + token`, `:154`, `:187`) no coincide con el del evento: así se descartan pendientes de otra configuración (`:202`).
- Propiedades del evento `$exception` (`:150-152`): `distinct_id` (GUID de sesión), `$process_person_profile` = `false`, `$geoip_disable` = `true`, `$lib` = `mito`, `$lib_version`, `mito_version`, `mito_operation` = `bootstrap.powershell`, `mito_phase`, `mito_runtime` (windows, bits, powershell), y `$exception_list` con tipo, valor (ya saneado), `handled=true`, un único frame con `lineno`, `function` = fase.
- Saneado de texto (`Protect-YitoText`, `:53-67`), en este orden:
  1. `https?://usuario:clave@` → `$1[redacted]@` (`:54`).
  2. Query de URL `http(s)://...?...` → `?[redacted]` (`:55`).
  3. Tokens `phc_...`, `php_...`, `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` → `[redacted]` (`:56`).
  4. `Bearer <token>` → `Bearer [redacted]` (`:57`).
  5. `X:\Users\<nombre>` → `<USER>` (`:58`).
  6. Pares `password|passwd|pwd|token|api_key|api-key|apikey|secret|authorization|cookie` seguidos de `:` o `=` → valor `[redacted]` (`:59`).
  7. Raíz de MITO (con `\` y con `/`) → `<MITO>` (`:60-63`).
  8. `USERPROFILE` → `<USER>` (`:64`).
  9. Truncado a 12000 caracteres (`:65`).

Duda: la telemetría va a un servicio externo. ¿Mantenerla en `mito.exe`? Si se mantiene, hace falta consentimiento explícito, porque el original la activa desde la configuración sin preguntar. Recomendación: decidirlo con el responsable del producto antes de portar.

## 6. `bootstrap_tools.ensure` en arranques posteriores

Qué hace el código del original:

- `ensure_tools` solo se llama desde `bootstrap.py:50`, es decir, dentro de la preparación de Python. `bootstrap.ps1` no la vuelve a ejecutar si `mito-ready.json` existe (`:639-643`).
- `mito bootstrap` (`__main__.py:182-185`) ejecuta `bootstrap.py` **directamente**, aunque ya esté listo. Es la única vía en el alcance que vuelve a comprobar las herramientas.
- `mito doctor` (`__main__.py:221-225`) solo lee `.mito-tool.json` de `windows-terminal` y `7zip` e informa si falta, sin reparar.
- `terminal.py:49` lanza `Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.` si falta la herramienta. Fuera de alcance, pero indica que el arranque normal no repara herramientas.
- `ui.py`, `start` y `shell`: no llaman a `ensure_tools` en el alcance. Duda: el `mito.cmd` (fuera de alcance) puede lanzar `bootstrap.ps1` en cada arranque. Si lo hace, la bienvenida se omite por `mito-ready.json` y no se revisan las herramientas.

Comprobación en un arranque posterior, cuando `ensure_tools` sí se ejecuta:

- Para cada herramienta, se considera **buena** si cumple las tres condiciones de 5.3 paso 2: marcador presente, versión igual a la del lock y todos los `required` como ficheros. Entonces no se descarga nada.
- Si el destino existe y no cumple lo anterior (por ejemplo, la versión del lock es nueva o falta un ejecutable), se aborta con `Herramienta incompleta: <ruta>. Conservada para revisar antes de sustituirla.`. **No se sustituye**. Para recuperarse, el usuario debe borrar `core/tools/<id>/` a mano.
- Si el destino no existe, se instala como en la primera vez.
- Al final se reescribe `core/state/bootstrap-tools.json` con el SHA-256 actual del lock (`:56`), incluso si nada cambió.

Formato de estado en disco resumido:

| Estado | Marcador | `core/state/bootstrap-tools.json` |
|---|---|---|
| Herramienta preparada | `core/tools/<id>/.mito-tool.json` con `"version"` del lock | `lock_sha256` del lock de la última pasada |
| Herramienta ausente | No existe `core/tools/<id>/` | Igual |
| Herramienta incompleta | `core/tools/<id>/` existe sin marcador válido | Se conserva el anterior (no se escribe) |

Duda: el original nunca compara `lock_sha256` con el guardado para decidir nada. ¿Debe Rust usarlo como atajo, o basta comparar cada `version`?

## 7. Invariantes

1. Una herramienta solo aparece en `core/tools/<id>/` si su `payload` completo se extrajo y comprobó (`required`) y el SHA-256 de cada artefacto coincide (`bootstrap_tools.py:30-32`, `:47`, `:52`). El `rename` final es la única publicación.
2. `ensure_tools` nunca sustituye una carpeta existente de herramienta (`:21-22`). El reemplazo es manual.
3. `core/tools/python/` solo existe si `mito-ready.json` fue escrito en el staging antes de publicarlo (`bootstrap.py:52`, `bootstrap.ps1:685`).
4. Dos preparaciones no pueden correr a la vez: `bootstrap.ps1` mantiene `core/tools/bootstrap.lock` abierto con `FileShare.None` (`:637`), y `ensure_tools` toma `mutation(root)` (`bootstrap_tools.py:14`, ver `storage.md`).
5. El SHA-256 se comprueba antes de extraer cualquier artefacto (`bootstrap_tools.py:31-32` y `bootstrap.ps1:654-659`).
6. Los pendientes de telemetría se envían solo a la misma combinación de `host` y `api_key` con la que se crearon (`bootstrap.ps1:154`, `:202`).
7. El log nunca hace fallar la preparación (`bootstrap.ps1:137`, `diagnostics`).
8. Ninguna contraseña del portal llega al log (`test_bootstrap_proxy.py:159`).
9. Los mensajes del lanzador salen saneados con `Protect-YitoText` (`:142-143`, `:112`).

## 8. Casos límite y errores

- Sin consola interactiva: `Read-Host` lanza → la bienvenida devuelve no → código 2 (`:25`).
- Respuesta vacía en la bienvenida: no (`[s/N]`).
- Respuesta vacía al pedir guardar el proxy: sí (`[S/n]`, `:395`).
- `config.properties` ilegible: se ignora en silencio (`:86`).
- `config.properties` inexistente al guardar: `Set-YitoProperties` crea el fichero con UTF-8 sin BOM (`:369-384`).
- `config.properties` sin `\r\n`: el salto de línea se detecta del primer `\r\n` (`:370`).
- Staging con restos de una ejecución anterior fallida: no se reutiliza. Se crea otro `bootstrap-<guid>` (`:619`). Los antiguos quedan en disco (no hay limpieza).
- `python-backup-<guid>` se acumula en cada reparación (no hay limpieza).
- `bootstrap.lock.json` no válido JSON: falla en `read-bootstrap-lock` y se registra (test `test_diagnostics.py:137-153`).
- Marcador `.mito-tool.json` corrupto: `read_json` lanza `ValueError` de Python, que no se envuelve en `YitoError` (`bootstrap_tools.py:19`). Duda: ¿qué debe hacer Rust?
- Extracción de una herramienta falla a mitad: el temporal `tmpXXXX` se borra en Python (`:25`). El staging de Python se conserva en PowerShell (`:697`). Asimetría deliberada o no: duda.
- Herramientas en orden: si 7-Zip se instala y WT falla, 7-Zip queda preparado y se vuelve a intentar WT en la próxima llamada, porque cada herramienta se comprueba por separado. El `mito-ready.json` no se escribe, así que la bienvenida vuelve a salir.
- Cambio de versión en el lock de una herramienta ya instalada: error `Herramienta incompleta` (sección 6). Duda: el mensaje dice «incompleta» cuando lo que falla es la versión.
- `MITO_ROOT_PATH_LIMIT`: solo afecta a la comprobación de ruta (ver `path_limit.md`).

## 9. Casos de test del original

Tests que cubren el bloque de bootstrap. Los 4 de `test_path_limit.py` están en `path_limit.md`.

Leyenda: «Aplica» = el comportamiento debe conservarse en Rust. «no aplica en Rust» = el test solo tiene sentido con PowerShell o Python. Si el comportamiento sí se conserva, se indica al lado.

| Test | Qué comprueba | Estado en Rust |
|---|---|---|
| `test_bootstrap_proxy.py:81` `test_system_proxy_no_proxy_form_and_properties_helpers` | Exclusiones de `ProxyOverride` a `no_proxy` (`localhost,127.0.0.1,::1,.corp.example,intranet,corp.example`); proxy de PAC y manual; puerto 80 por defecto; que sin proxy no hay resultado; extracción de formulario (acción relativa con query, campos `csrf`, `login`, `pwd`); clasificación `ok`/`auth`/`portal`/`unreachable`/`blocked`; y `Set-YitoProperties` conserva comentarios, orden y claves ajenas. | no aplica en Rust como harness de PowerShell. Comportamiento a portar: sección 5.7 y el formato de `config.properties`. |
| `test_bootstrap_proxy.py:119` `test_probe_and_portal_login_through_local_proxy` | Sonda con 407 NTLM, portal con redirección, origen distinto rechazado, HTTPS obligatorio, campos ocultos y usuario enviados, contraseña incorrecta no verifica, contraseña correcta verifica, y la contraseña no aparece en el log. | no aplica en Rust como harness (usa un proxy HTTP local y PowerShell). Comportamiento a portar: sección 5.7 (con la contraseña no registrada). |
| `test_diagnostics.py:23` `test_rotation_keeps_three_backups_and_valid_json_lines` | Rotación con 3 copias y líneas JSON válidas. Es del módulo `diagnostics.py`, fuera de este contrato, pero el log del bootstrap lo usa. | Aplica (capa de log). Ver `diagnostics` cuando exista. |
| `test_diagnostics.py:35` `test_concurrent_writers_keep_complete_records` | Cuatro escritores concurrentes, 80 líneas completas. | Aplica (capa de log, bloqueo entre procesos). |
| `test_diagnostics.py:43` `test_fallback_and_failure_do_not_raise` | Si la carpeta principal falla, usa la alternativa; si ambas fallan, no lanza. | Aplica (5.5). |
| `test_diagnostics.py:52` `test_scrub_keeps_explanation_but_removes_known_secrets` | Saneado: credenciales de URL, query, `password="..."`, `Bearer`, `phc_`, rutas de usuario y de raíz; conserva el texto explicativo. | Aplica (5.8, lista de saneado). |
| `test_diagnostics.py:62` `test_subprocess_error_does_not_log_command_arguments` | Error de proceso hijo: mensaje `Child process failed with exit code 7`, sin argumentos (con `--password secret`). | Aplica en concepto: un error de proceso externo (7zr) no debe mostrar argumentos. Mecanismo Python no aplica. |
| `test_diagnostics.py:67` `test_bootstrap_welcome_requires_explicit_yes` | Solo `s`, `si`, `sí` (sin distinguir mayúsculas ni espacios) dan consentimiento; `''`, `n`, `yes` no. El texto contiene `My Integrated Terminal Orchestrator` y `Windows Terminal`. | El comportamiento **aplica** (3.3). El mecanismo de sustitución de `Read-Host` no aplica en Rust. Nota: `yes` se rechaza aquí, pero sí se acepta en la pregunta de proxy (`:396`). Duda. |
| `test_diagnostics.py:90` `test_bootstrap_cancellation_creates_no_tools_or_pending_events` | Cancelar sale con código 2, no crea `core/tools` ni `logs/pending`, y el último registro es `confirmation` / `info`. | Aplica (3.4). El harness PowerShell no aplica. |
| `test_diagnostics.py:109` `test_python_retries_powershell_pending_event_without_real_network` | Un error del lanzador queda en `logs/pending`, y Python lo reenvía a PostHog (simulado) con `mito_phase` y el valor de la excepción, y lo borra tras el envío. | no aplica en Rust (depende del reenvío desde Python con `error_tracking`). Duda sobre si Rust conserva la cola (5.8). |
| `test_diagnostics.py:137` `test_real_bootstrap_records_failure_before_python_exists` | Con un lock inválido, el bootstrap sale con 1, registra error en fase `read-bootstrap-lock`, no crea `python.exe` y no deja pendientes. | Aplica (fase de lectura del lock: error registrado y sin estado parcial). El harness PowerShell no aplica. |
| `test_diagnostics.py:156` `test_powershell_bootstrap_log_without_python_or_network` | El registro de error tiene `phase`, `level` = `error`, mensaje con `fixture`, campo `windows`; el secreto `SECRET` no aparece en el log; y un registro posterior de Python conserva `operation` = `python`. | Aplica en concepto (formato del registro y saneado). El harness PowerShell no aplica. |

Cobertura que falta en el original: no hay tests para `ensure_tools` (`bootstrap_tools.py`) ni para `bootstrap.py`, ni para el hash del lock, ni para `mito-ready.json`, ni para `-PreviewWelcome`, ni para `-Approved`, ni para la descarga con SHA-256 fallido. Hay que escribir tests nuevos en Rust para cada caso marcado en las secciones 5 y 6.

Test indirecto: `test_activation.py:118` se salta si falta el 7-Zip del bootstrap. No es un test de bootstrap.

## 10. Dependencias

Del original:

- `storage.py`: `write_json` (marcadores, `mito-ready.json`, `bootstrap-tools.json`), `read_json`, `inside` (rutas de `required` y `install.target`), `mutation` (bloqueo de `ensure_tools`), `YitoError`. Ver `storage.md`.
- `transfer.py`: `download(url, destino, sha256, root=)` y `extract_zip`. Fuera de alcance; el manejo de errores del hash está allí.
- `diagnostics.py`: `write`, `step`, `environment`, `message`, `scrub`, rotación y bloqueo del log. Fuera de alcance.
- `error_tracking.py`: `capture`, `flush`, `resume`. Solo Python. No se porta.
- `terminal.py`: `brand_window_icons`. Fuera de alcance.
- `i18n.py`: `tr` para los mensajes de `ensure_tools`. Ver `storage.md`.
- `path_limit.py`: comprobación de ruta. Ver `path_limit.md`.
- Consumidores en otros módulos: `__main__.py:127-138` (bloqueo de ruta), `__main__.py:182-185` (`mito bootstrap`), `__main__.py:221-225` (`doctor`), `self_update.py:183`, `:190-197`, `:256-270`, `:417` (actualizador con `-Approved`), `release.py:48` (empaqueta lanzador, lock y bootstrap.py en la release), `terminal.py:49`.
- Biblioteca estándar de PowerShell 5.1 y .NET Framework: `System.Net.WebRequest`, `Net.WebProxy`, `Get-ItemProperty`, `Compression.ZipFile`, `Security.Cryptography.SHA256`.

Del port (ADR 0005, 0008):

- Descargas: `mito_transfer::download`.
- Extracción: `mito_transfer::archives` (ZIP nativo; 7z delegado en `7zr.exe`).
- Log: `tracing` y la capa de `mito-core::diagnostics`.
- Errores: `mito_core::Error` con mensajes `Message` en español.
- Bloqueo: `File::try_lock` sobre `core/state/<name>` (ADR 0008 §8).

## 11. Notas para Rust

**Qué portar y qué no**

- Se portan: la bienvenida (sin filas de Python ni Textual), la comprobación de ruta (en `path_limit.md`), la descarga con SHA-256, la extracción de 7-Zip, innounp y Windows Terminal, los marcadores con el mismo formato, `bootstrap-tools.json`, el log y el saneado, y el proxy (ver abajo).
- No se portan: `bootstrap.ps1` como proceso, `bootstrap.py`, `python.zip`, las 39 ruedas, `mito-ready.json` (el ejecutable ya contiene el motor), `python*._pth`, `-DiagnosticExitCode`, la cola de telemetría de PowerShell (ver duda), y las fases `download-wheel`, `configure-python-path`, `validate-runtime-imports`, `publish-ready-marker`.
- Duda: el lock de Rust puede conservar la forma actual (con `python` y `wheels` y sin usarlos) para no romper las release publicadas, o eliminar esas claves. Recomendación: conservar `schema` y `tools` y dejar de leer `python` y `wheels`; hace falta acordar con quien publica las releases (`release.py:48`, `self_update.py:190`).

**Bienvenida en `mito.exe`**

- Opción A (recomendada, mantiene semántica): texto en consola con los mismos renglones, `[s/N]` y la misma lista de respuestas (`s`, `si`, `sí`, sin distinguir mayúsculas y con espacios recortados). Sin consola interactiva (lectura fallida), tratar como no.
- Opción B: `MessageBox` de Win32 con Sí y No, por defecto No, y el mismo texto. Pierde el texto con la forma exacta de la consola.
- Decidir una. En ambos casos, cancelar sale con código 2 y el mensaje literal `Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.`.
- Un flag oculto equivalente a `-Approved` (sin diálogo, sin proxy interactivo, sin comprobación de ruta) para el actualizador (`self_update.py:266`). Duda: el actualizador de Rust, ¿debe seguir lanzando un proceso separado?

**Descarga y verificación**

- `mito_transfer::download(url, destino, sha256)` debe borrar el fichero si el hash no coincide, como exige el original (`bootstrap_tools.py:31-32`). Duda: el original no muestra el hash esperado ni el obtenido. Recomendación: en el log, registrar solo los dos hashes, no la URL con query.
- Sin campo de tamaño en el lock (sección 2.2). Recomendación: no añadirlo sin acordarlo.
- Descargar y verificar antes de extraer; el staging `tmpXXXX` debe estar en el mismo volumen que `core/tools` para que el `rename` sea atómico.

**Extracción**

- ZIP (`windows-terminal`, `innounp`): `mito_transfer::archives` con la comprobación de rutas, duplicados y límites del ADR 0005. Copiar `source` como en el original.
- `7zip`: copiar `7zr.exe` y ejecutar `7zr.exe x <instalador> -o<payload> -y` con `CREATE_NO_WINDOW` y captura de salida. Un fallo se convierte en error con el código de salida, sin argumentos en el mensaje.
- Crear `.portable` para Windows Terminal (`bootstrap_tools.py:49-50`).

**Marcadores y estado**

- `.mito-tool.json` con `{"version": "<v>"}` y `core/state/bootstrap-tools.json` con `{"lock_sha256": "<hex>"}`. Formato: JSON con 2 espacios de sangría, UTF-8 sin BOM, fin de línea `\n`, claves en orden de inserción. En serde, `serde_json` con `PrettyFormatter` de 2 espacios y `preserve_order`.
- Escritura atómica: fichero temporal en el mismo directorio, `sync_all` y `persist`.
- Equivalente a `mito-ready.json`: el original escribe `{"schema": 1, "lock_sha256": "..."}`. Duda: ¿Rust lo necesita? Sin Python no hay fase que lo use, pero `doctor` y las herramientas de diagnóstico pueden leerlo.

**Herramienta incompleta vs. cambio de versión**

- Duda principal. El original falla con `Herramienta incompleta` si la versión cambia. Recomendación: distinguir los dos casos y, para versiones distintas, ofrecer reemplazo (pidiendo confirmación) en lugar de dejar al usuario borrar carpetas a mano. Esto altera el comportamiento; requiere decisión.
- Un marcador corrupto no debe producir un `ValueError` sin tratar. Recomendación: tratarlo como herramienta incompleta.

**Proxy**

- Solo Windows. Leer `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` (`ProxyEnable`, `ProxyServer`, `AutoConfigURL`, `ProxyOverride`) y la configuración de WinINet. Crate candidata: `windows` o `windows-sys` con `WinHttpGetIEProxyConfigForCurrentUser` y `WinHttpGetProxyForUrl` para PAC/WPAD (duda: el original usa `GetSystemWebProxy`, que está más cerca de WinINet; hay que probar en una máquina con PAC).
- PAC: el original no evalúa PAC; solo detecta el proxy que devuelve Windows. Duda: ¿Rust puede evaluar PAC? Recomendación: no hacerlo por ahora y documentar la diferencia.
- `config.properties`: `Set-YitoProperties` conserva comentarios, orden y claves ajenas (`test_bootstrap_proxy.py:116-117`). Replicar en Rust con el mismo algoritmo (leer líneas, sustituir la clave con `^\s*clave\s*=` sin distinguir mayúsculas, añadir al final, escribir temporal y reemplazar).
- Login de portal: pedir usuario y contraseña en consola sin eco (`rpassword` o `windows` con `ENABLE_ECHO_INPUT` apagado). No guardar credenciales (`:557`, `:531`). Duda: ¿se porta el login de portal o solo la detección? Recomendación: portar la detección y el aviso; el login es una pieza grande y de alto riesgo.
- Cabeceras y registro: nunca registrar la contraseña, y saneado de la URL del portal (`:506`).
- El mensaje `windows` (`:504`) menciona a Python. En Rust debe reescribirse sin referencias a Python; no lo traduzco aquí para no inventar un texto nuevo. Duda: texto nuevo a acordar.

**Telemetría**

- Duda: ¿se mantiene el envío de errores del bootstrap a PostHog? Si se mantiene, debe ser opt-in explícito. El saneado de la sección 5.8 debe mantenerse.

**Errores**

- `mito_core::Error` con `Message` (ADR 0008 §5): `Herramienta incompleta: {p1}` con la ruta como argumento; `Hash de Python incorrecto` deja de aplicar y se sustituye por el hash de la herramienta que falle (duda: texto nuevo; el literal actual es de Python).
- Duda: `MITO requires Windows x64` (en inglés, literal). En Rust, ¿mantener el inglés o traducir? Recomendación: mantener el literal, porque el ADR 0008 dice que se replican los mensajes del original.

**Carpetas y limpieza**

- Staging fallido: el original lo conserva en PowerShell y lo borra en Python. Decidir: recomendación, borrar el temporal tras un fallo pero conservar el log con la ruta del directorio, para diagnóstico.
- `python-backup-*`: no hay limpieza en el original; en Rust no aplica (no hay Python). Recomendación: no crear copias de seguridad de carpetas que ya no existen.

**Bloqueo**

- `core/tools/bootstrap.lock` con `FileShare.None` (PowerShell) → en Rust, `File::try_lock` (exclusivo) sobre el mismo nombre, con la misma ruta, para que una instancia de Python antigua no colisione (ADR 0008 §8). Duda: ¿debe coexistir con el original durante una transición?

**Orden de salida**

- Éxito: código 0 (el original no escribe mensaje de éxito; duda, ver sección 5.2). Cancelación: 2. Error: 1. Ruta demasiado larga: 3 en PowerShell, 1 en `__main__.py:138` (ver `path_limit.md`).

## Dudas abiertas (resumen)

1. Tamaño de descarga: el lock no lo tiene. ¿Añadirlo o no?
2. `destination` en el lock: no lo usa ningún código. ¿Mantenerlo?
3. `schema` y `python`/`wheels` en el lock de Rust: ¿qué forma tiene el lock?
4. Textos de la bienvenida sin Python ni Textual/Pillow/SDK MCP: ¿se suprimen?
5. Bienvenida: ¿consola o `MessageBox`? ¿Depende solo de `mito-ready.json` o de que falte alguna herramienta?
6. Cambio de versión de una herramienta: `Herramienta incompleta` (sin reemplazo). ¿Rust reemplaza con confirmación?
7. Marcador corrupto: ¿qué error se devuelve?
8. Quién llama a `ensure_tools` en arranques posteriores y si `mito.cmd` ejecuta `bootstrap.ps1` en cada inicio (`mito.cmd` está fuera de alcance).
9. `bootstrap-tools.json` no se lee en ninguna parte del alcance: ¿Rust lo usa como atajo?
10. Telemetría de errores del bootstrap: ¿se mantiene y con qué consentimiento?
11. Login de portal de proxy: ¿se porta o solo la detección?
12. Texto `Pulsa Intro para cerrar` y código de salida tras ruta larga: 3 en PowerShell, 1 en CLI (ver `path_limit.md`).
13. `mito.transfer.download` (verificación del hash y su mensaje de error) está fuera de alcance: hay que leerlo antes de fijar los mensajes de descarga.
14. Staging fallido: ¿se conserva para diagnóstico o se borra?
