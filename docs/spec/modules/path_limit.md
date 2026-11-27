# path_limit.py (y comprobación de ruta de `bootstrap.ps1`) → `mito.exe` (comprobación de la raíz)

Origen:
- `core/app/mito/path_limit.py` (44 líneas).
- `core/tests/test_path_limit.py` (73 líneas, 4 tests).
- Equivalente en PowerShell: `core/bootstrap.ps1:29-51` (`Test-YitoLongPaths`, `Get-YitoRootPathLimit`, `Get-YitoRootPathProblem`), y su llamada en `bootstrap.ps1:604-615`.
- Llamada desde la CLI: `core/app/mito/__main__.py:127-138`.

## Responsabilidad

Impedir que `mito.exe` (y el lanzador) trabajen desde una carpeta cuya ruta sea demasiado larga cuando Windows no tiene habilitadas las rutas largas (`LongPathsEnabled`). El límite es 32 caracteres: la carpeta de MITO debe quedar cerca de la raíz del disco para que quepan árboles profundos (`node_modules`, recursos de Electron) bajo el límite de 260 caracteres de Windows. La comprobación solo bloquea la preparación y la interfaz, no las aplicaciones ya instaladas (`__main__.py:128-129`).

Contrato en Rust (ADR 0008 §3): solo Windows x64.

## API pública del original

| Función / constante | Entradas | Salida | Errores |
|---|---|---|---|
| `MAX_ROOT = 32` (`path_limit.py:12`) | — | Umbral por defecto | — |
| `long_paths_enabled(...)` (`:15-23`) | — | `bool` | No lanza. Si no es Windows, devuelve `True` (`:16-17`). Si la clave no existe, devuelve `False` (`:22-23`). |
| `maximum()` (`:26-32`) | Variable de entorno `MITO_ROOT_PATH_LIMIT` | Entero, nunca menor que 32 | No lanza. Valor no numérico → 32 (`:30-31`). |
| `problem(root, enabled=long_paths_enabled)` (`:35-44`) | Ruta de la raíz, función de detección | Mensaje traducido o `None` | No lanza. |
| `Test-YitoLongPaths` (`bootstrap.ps1:29-34`) | — | `bool` | Cualquier excepción → `$false` (`:33`). |
| `Get-YitoRootPathLimit` (`bootstrap.ps1:38-43`) | Variable `MITO_ROOT_PATH_LIMIT` | Entero | — |
| `Get-YitoRootPathProblem($Root, $Maximum, $LongPaths)` (`bootstrap.ps1:45-51`) | Ruta, máximo, si hay rutas largas | Texto o `$null` | — |

### Detección de rutas largas

- Python (`path_limit.py:16-23`): en Windows, abre `HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\FileSystem` (`:20`), lee `LongPathsEnabled` (`:21`) y devuelve `True` solo si el valor es exactamente `1` (`:21`). Si la clave o el valor fallan (`OSError`, `:22`), devuelve `False`.
- PowerShell (`bootstrap.ps1:31-32`): `Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled`, y `[int]$value.LongPathsEnabled -eq 1`. Cualquier excepción devuelve `$false` (`:33`).
- Ambos coinciden: solo `1` cuenta como activado. Cualquier otro valor (`0`, ausente, texto) es desactivado.

### Umbral (`maximum`)

- Python (`path_limit.py:26-32`): lee `MITO_ROOT_PATH_LIMIT`; si no es entero, devuelve `32`; si lo es, devuelve `max(valor, 32)`. Por tanto la variable solo puede subir el umbral (`:27`, docstring).
- PowerShell (`bootstrap.ps1:38-43`): `[int]::TryParse` sobre la variable (`:41`). Si el valor es mayor que 32 se devuelve; en otro caso, `32` (`:42`). Mismo efecto.
- Ejemplos de la tabla de tests: `1000` sube el umbral (`test_path_limit.py:64-65`); `5`, `x` y la cadena vacía dejan 32 (`:66-68`).

### Cálculo de la longitud (`problem`)

- Python (`path_limit.py:37`): `os.path.abspath(Path(root)).rstrip('\\/')`. La longitud es `len()` del texto resultante, es decir, puntos de código Unicode (`:38-39`).
- PowerShell (`bootstrap.ps1:46`): `[IO.Path]::GetFullPath($Root).TrimEnd('\')`. La longitud es `.Length`, es decir, unidades UTF-16 (`:47`).
- Regla común: si la longitud es menor o igual que el umbral, o hay rutas largas, no hay problema (`:39`, `bootstrap.ps1:47`).

### Mensaje literal

Python (`path_limit.py:41-44`), tres líneas separadas por `\n`, sin el prefijo `MITO:` (lo añade la CLI, `__main__.py:133`):

```
La carpeta de MITO tiene una ruta demasiado larga ({p1} caracteres; máximo {p2}): {p3}
Windows limita las rutas a 260 caracteres y algunas aplicaciones no se podrían instalar.
Mueve la carpeta más cerca de la raíz del disco, por ejemplo C:\mito, y vuelve a ejecutar mito.cmd.
```

- `{p1}` es la longitud calculada; `{p2}` el umbral; `{p3}` la ruta completa normalizada.
- En la tercera línea, `C:\mito` contiene una barra invertida simple (en Python se escribe `C:\\mito`, `:43`).
- PowerShell (`bootstrap.ps1:48-50`) produce el mismo texto: `La carpeta de MITO tiene una ruta demasiado larga (` + longitud + ` caracteres; máximo ` + umbral + `): ` + ruta, y las dos líneas siguientes unidas con `` `n ``.
- Se traduce con `tr()` (`path_limit.py:10`, `:41`). Los argumentos `p1`, `p2`, `p3` se pasan sin traducir.

Ejemplo de salida con la ruta del test, `C:\Users\usuario\Downloads\mito-0.3.1-windows-x64` (la longitud es la que calcule la implementación; el test solo comprueba que aparece el número, `test_path_limit.py:25`).

## Formatos de disco

No escribe nada. Lee del registro:

- `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem`, valor `LongPathsEnabled` (DWORD). Solo `1` activa las rutas largas.

El umbral no se guarda en ningún archivo. La variable `MITO_ROOT_PATH_LIMIT` es solo para pruebas (`bootstrap.ps1:39`, `path_limit.py:27`); los tests de `test_diagnostics.py:14` la usan para subir el umbral en carpetas temporales.

## Invariantes

1. El umbral efectivo nunca es menor que 32. Ni la variable de entorno ni un valor inválido pueden bajarlo (`path_limit.py:27-32`, `bootstrap.ps1:41-42`).
2. Una raíz de 32 caracteres o menos pasa siempre (`test_path_limit.py:19-21`, con longitud exacta 32).
3. Con `LongPathsEnabled = 1`, ninguna raíz se bloquea (`:39`, `bootstrap.ps1:47`).
4. En una raíz que no pasa, el mensaje contiene la ruta completa y su longitud (`test_path_limit.py:23-25`).
5. La barra final no cuenta (`test_path_limit.py:18`: `C:\mito-0.3.1-windows-x64\` pasa).
6. La comprobación no se hace si el lanzador corre con `-Approved` (`bootstrap.ps1:604`). El actualizador prepara en una carpeta temporal del host.
7. La comprobación no lee ni escribe el disco, salvo el registro.

## Casos límite y errores

- Raíz exactamente de 32 caracteres: pasa (`test_path_limit.py:19-21`).
- Raíz de 33 caracteres sin rutas largas: bloquea. No hay test de este caso exacto, pero es consecuencia directa de `len <= limit` (`path_limit.py:39`). Ver Casos de test.
- Ruta con barra final: se recorta (`C:\mito\` cuenta como `C:\mito`, `test_path_limit.py:52`).
- Ruta con barra `/`: Python la recorta; PowerShell no (solo recorta `\`). Ver Dudas.
- Ruta relativa: `abspath` la resuelve contra el directorio actual (`path_limit.py:37`). Dentro de MITO se usa la raíz ya absoluta.
- Ruta con caracteres no BMP (emoji): Python cuenta 1 por carácter; PowerShell cuenta 2. Ver Dudas.
- Registro inaccesible: se trata como desactivado (`path_limit.py:22-23`), con lo que la comprobación puede bloquear. Esto es lo esperado.
- `MITO_ROOT_PATH_LIMIT` con espacios: `int(" 40 ")` admite espacios en Python; `TryParse` también (`bootstrap.ps1:41`).
- Mensaje devuelto cuando `problem` no bloquea: `None` (Python) o `$null` (PowerShell).

## Comandos bloqueados por la CLI

Llamada en `__main__.py:127-131` (condición en `:127`, comprobación en `:130-131`). Antes de la comprobación, `logs` sale con código 0 y no se comprueba (`:122-125`, `test_path_limit.py:36-38`).

| Comando | Bloqueado | Motivo |
|---|---|---|
| `ui` (y sin comando, que equivale a `ui`) | Sí | Abre el gestor (`__main__.py:127`) |
| `start` | Sí | Abre una app (`:127`) |
| `bootstrap` | Sí | Prepara herramientas (`:127`, `:182-185`) |
| `logs` | No | Sale antes (`:121-123`) |
| Resto (`shell`, `mcp-server`, `doctor`, `posthog-check`, `bucket`, etc.) | No | Las apps, shims y MCP ya instalados siguen funcionando hasta mover la carpeta (`:125-126`) |

Cuando bloquea, en orden:

1. Registro `error`, operación `cli.*` (la de `diagnostics.write`), fase `startup.path-limit`, mensaje igual al literal (`:132`).
2. `print` a `stderr` de `\nMITO: <mensaje>\n` (`:133`).
3. `input(tr('Pulsa Intro para cerrar'))` (`:134-137`). `EOFError` y `KeyboardInterrupt` se ignoran.
4. Devuelve código `1` (`:138`).

El lanzador de PowerShell hace lo mismo con otros detalles (`bootstrap.ps1:604-615`):

- Registro `error`, fase `path-limit` (`:608`).
- `MITO: <mensaje>` en rojo (`:610`).
- `Read-Host 'Pulsa Intro para cerrar'` (`:612`). Sin consola, se ignora la excepción.
- Código `3` (`:613`).

## Casos de test del original

`test_path_limit.py` (4 tests):

| Test | Línea | Qué comprueba |
|---|---|---|
| `test_short_roots_pass_and_long_roots_only_fail_without_long_paths` | `:16` | `C:\mito` pasa. `C:\mito-0.3.1-windows-x64\` (con barra final) pasa. Una raíz de 32 caracteres exactos pasa. La ruta de Descargas (`C:\Users\usuario\Downloads\mito-0.3.1-windows-x64`) falla sin rutas largas, el mensaje contiene la ruta y su longitud, y pasa con rutas largas activadas. |
| `test_manager_and_bootstrap_are_blocked_but_other_commands_are_not` | `:28` | Con el problema simulado, `ui`, `start` y `bootstrap` devuelven `1`, llaman a `input` una vez y no llegan a `Engine.initialize`. `logs` devuelve `0` y ni siquiera llama a `problem`. |
| `test_bootstrap_uses_the_same_threshold` | `:40` | Solo Windows (`skipUnless`). Ejecuta las funciones de PowerShell con 4 casos: ruta larga sin rutas largas → `True`; ruta larga con rutas largas → `False`; 32 exactos → `False`; `C:\mito\` → `False`. Resultado esperado `True,False,False,False`. Comprueba además que el texto de la función contiene `return 32`. |
| `test_environment_can_only_raise_the_threshold` | `:62` | `MITO_ROOT_PATH_LIMIT=1000` hace pasar la ruta de Descargas. `5`, `x` y `''` dejan `maximum()` en 32. |

Los 4 casos están cubiertos. El test `test_manager_and_bootstrap_are_blocked_but_other_commands_are_not` valida la tabla de comandos bloqueados de la sección anterior.

Tabla de aplicabilidad en Rust:

| Test | Estado | Nota |
|---|---|---|
| `test_short_roots_...` | Aplica | Portar con las rutas de Windows; las rutas de Unix no aplican (ADR 0008 §3). |
| `test_manager_and_bootstrap_...` | Aplica con cambios | Los comandos de Rust son los de `mito.exe`. Verificar la tabla de comandos y la ausencia de bloqueo para `logs`. |
| `test_bootstrap_uses_the_same_threshold` | **no aplica en Rust** | Compara dos implementaciones (Python y PowerShell). Con un único código en Rust no hay nada que comparar. La parte verificable (que el umbral es 32) pasa a ser una constante testeada. |
| `test_environment_can_only_raise_the_threshold` | Aplica | Portar tal cual. |

## Dependencias

- `i18n.tr` (`path_limit.py:10`), mensajes en español por defecto. Ver `storage.md` para `tr`.
- `os.environ` (`path_limit.py:29`), `os.path.abspath` (`:37`), `winreg` (`:18`).
- Llamador: `__main__.py:127-138` (CLI). El lanzador de PowerShell (`bootstrap.ps1:604-615`) se reemplaza por la misma comprobación en `mito.exe` (ver `bootstrap.md`).
- No depende de `storage`, `engine` ni `diagnostics`, salvo el registro que hace el llamador (`__main__.py:132`).

## Notas para Rust

- **Dónde vive**: la comprobación debe ejecutarse al principio de `main` de `mito.exe`, antes de cualquier efecto, solo para los comandos que bloquea la tabla (`ui`, `start`, `bootstrap` y sin comando). Mantener `logs` fuera. El llamador debe decidir la tabla, no la función de comprobación.
- **Qué ruta medir**: la raíz de la instancia, no la del ejecutable, como en el original (`path_limit.py:35-37`). En Rust la raíz sale de `Layout::from_executable` (ADR 0008 §2), que sube dos niveles desde `core\bin\mito.exe`. Hay que medir esa raíz. Duda: el ejecutable está dentro de `core\bin`, así que su ruta es más larga; el original mide la raíz, no el ejecutable.
- **Normalización**: `std::path::absolute` (estable desde Rust 1.79) equivale a `abspath` sin tocar el disco. Quitar la barra final de `\` (y de `/` si se decide igualar a Python, ver Dudas). Usar `dunce` si hay que evitar el prefijo `\\?\`.
- **Longitud**: contar unidades UTF-16 (`encode_utf16().count()`) para coincidir con PowerShell, o puntos de código (`chars().count()`) para coincidir con Python. Dado que el original usa PowerShell solo en la primera ejecución y Python en todas las demás, recomendación: UTF-16, porque es lo que Windows usa para el límite de 260 caracteres.
- **Registro**: leer `HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\FileSystem` con la crate `winreg` (o `windows-registry`), valor `LongPathsEnabled` como DWORD. Solo `1` cuenta. Cualquier error de lectura se trata como desactivado (`path_limit.py:22-23`). Sin `KEY_WOW64_32KEY`: la vista de 64 bits es la correcta en el único target (ADR 0008 §3).
- **Umbral**: constante `MAX_ROOT = 32`. La variable `MITO_ROOT_PATH_LIMIT` solo sube el umbral y solo sirve para pruebas. Duda: ¿mantenerla en la versión publicada? Recomendación: mantenerla por compatibilidad con los tests del original (`test_diagnostics.py:14`), y documentarla como solo para pruebas.
- **Mensaje**: `Message` de `mito-core` (ADR 0008 §5) con la plantilla española de la sección "Mensaje literal" y los argumentos `p1`, `p2`, `p3` sin traducir. La TUI lo traduce con `mito-i18n`; la CLI lo muestra en español. Es decir, el texto no cambia.
- **Salida**: el original usa código 1 en la CLI (`__main__.py:138`) y código 3 en PowerShell (`bootstrap.ps1:613`). Duda: ¿qué código usa `mito.exe`? Recomendación: 1, para que el código de salida sea coherente con el resto de errores de la CLI.
- **Pausa**: `input('Pulsa Intro para cerrar')` se sustituye por lectura de línea en consola con el mismo texto. Si la entrada falla, salir sin error (`__main__.py:134-137`, `EOFError` y `KeyboardInterrupt`).
- **Registro del evento**: `diagnostics.write(root, 'error', 'startup.path-limit', message)` (`__main__.py:132`). En Rust, un `tracing::error!` con operación `startup.path-limit` y el mensaje. Las referencias de registro están en `__main__.py:132`.
- **Tests**: traducir los tests 1, 2 y 4 a `cargo test` con un parámetro de "rutas largas" inyectado (equivalente al `enabled=` de Python) para no leer el registro en CI. Aislar la lectura del registro en una función que se pueda sustituir.

## Dudas

- Barra `/` final: Python la recorta (`rstrip('\\/')`, `path_limit.py:37`) y PowerShell no (`bootstrap.ps1:46`). ¿Cuál se toma en Rust? Recomendación: recortar ambas, porque `mito.exe` no hace la comprobación de PowerShell.
- Unidades de longitud: `len()` de Python cuenta puntos de código; `.Length` de PowerShell cuenta unidades UTF-16. Para rutas con caracteres no BMP el resultado difiere en 1 por carácter. ¿Cuál es el criterio?
- Código de salida: 1 (CLI) frente a 3 (PowerShell). ¿Cuál usa `mito.exe`?
- Tabla de comandos: el original bloquea `ui`, `start` y `bootstrap`, pero no `shell`, `mcp-server` ni `doctor`. ¿Se mantiene la tabla en Rust, o los comandos nuevos (si los hay) se incluyen?
- El texto `Pulsa Intro para cerrar` aparece con y sin signo de cierre de frase en distintos sitios (`__main__.py:135` lo traduce con `tr()`; `bootstrap.ps1:612` no). No cambia el comportamiento; confirmar si el texto se repite tal cual.
- `long_paths_enabled` devuelve `True` fuera de Windows (`path_limit.py:16-17`). Como Rust es solo Windows, no hace falta esa rama. ¿Se elimina o se mantiene para tests en Linux?
- `MITO_ROOT_PATH_LIMIT`: ¿debe existir en la versión publicada de `mito.exe`?
