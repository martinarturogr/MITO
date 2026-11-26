# diagnostics.py → mito-core::diagnostics

Origen: `core/app/mito/diagnostics.py` (150 líneas), `core/tests/test_diagnostics.py` (10 tests).

## Responsabilidad
Log local en JSON Lines, acotado y rotado, con saneado de secretos y rutas. Es independiente de dependencias externas porque lo usa también el bootstrap (Python previo al entorno). Expone la captura de contexto de fase (`step`), un lock compatible con el bootstrap PowerShell (`guard`) y la ubicación alternativa del log.

## API pública del original
| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `MAX_BYTES = 1024 * 1024` (`:16`) | — | constante | — |
| `BACKUPS = 3` (`:17`) | — | constante | — |
| `locations(root) -> (Path, Path)` (`:22-24`) | raíz | `(<root>/logs, <tmp>/MITO-logs/<identidad>)` | — |
| `scrub(root, value) -> str` (`:27-47`) | raíz, cualquier valor | texto saneado, máx. 12000 caracteres | — |
| `message(root, exc) -> str` (`:50-55`) | raíz, excepción | texto seguro para el log | — |
| `environment() -> dict` (`:58-64`) | — | `{python, bits, platform, windows?}` | — |
| `step(root, operation, phase, component=None)` (`:67-77`) | context manager | — | Re-lanza la excepción original |
| `guard(folder)` (`:80-111`) | carpeta | context manager (lock) | `OSError('Diagnostic lock busy')` interno |
| `write(root, level, operation, text, **details) -> Path \| None` (`:114-150`) | nivel, operación, texto, detalles | ruta del log o `None` | **Nunca lanza.** |

## Formatos de disco

### Línea de log (`write`, `:114-125`)
Cada registro es un objeto JSON en **una línea**, terminado en `\n`. Se serializa con `json.dumps(record, ensure_ascii=True, default=str)` (`:121`): separadores por defecto de Python (`", "` y `": "`), no-ASCII escapado como `\uXXXX`, sin saltos internos.

Claves de nivel superior, en este orden:
1. `time`: `datetime.now(timezone.utc).isoformat()`, por ejemplo `2026-11-26T21:15:00.123456+00:00`. Python omite los microsegundos cuando son 0.
2. `level`: `info`, `warning` o `error` (niveles usados en el código).
3. `pid`: entero, `os.getpid()`.
4. `operation`: `scrub(operation)`.
5. `message`: `scrub(text)`.
6. Después, los detalles como claves planas, en el orden de los argumentos nombrados de la llamada a `write`. Claves usadas por el código:
   - `event_id`, `version`, `runtime`, `exceptions`, `component`, `phase` (desde `error_tracking.capture`, `:218-222`). `component` y `phase` van siempre presentes, con `null` si no hay valor.
   - `component` (desde `step`, `:69`). Siempre presente, con `null` si no se pasó.
   - `pending` (`true`, desde `error_tracking`).
   - `truncated` (`true`, `:124-125`).

Ejemplo ilustrativo construido a partir del código (valores entre `<…>` son marcadores):
```
{"time": "<ISO-8601 UTC>", "level": "error", "pid": <pid>, "operation": "install", "message": "Cannot extract: password=[redacted]", "event_id": "<uuid>", "version": "<versión>", "runtime": {"python": "<x.y.z>", "bits": 64, "platform": "win32", "windows": "<major.minor.build>"}, "exceptions": [{"type": "OSError", "value": "Cannot extract: password=[redacted]", "module": "builtins", "mechanism": {"handled": true, "synthetic": false}, "stacktrace": {"type": "raw", "frames": [{"platform": "custom", "lang": "python", "filename": "mito/<ruta>.py", "function": "<f>", "lineno": <n>, "in_app": true}]}}], "component": null, "phase": "extract"}
```

Excepto en los detalles `exceptions` y `runtime`, los valores pasan siempre por `scrub`.

### `environment()` (`:58-64`)
```
{"python": "3.12.4", "bits": 64, "platform": "win32", "windows": "10.0.26100"}
```
(ejemplo; `windows` solo si `sys.getwindowsversion` existe, formato `major.minor.build`).

### Rotación (`write`, `:126-136`)
- Activo: `<carpeta>/mito.log`.
- Antes de añadir `data` (bytes): si existe el activo y `tamaño_actual + len(data) > MAX_BYTES` (1 048 576 bytes, `:16`, `:130`), se rota:
  1. borra `mito.log.3` (`BACKUPS`, `:131`),
  2. renombra `mito.log.2` → `mito.log.3`, luego `mito.log.1` → `mito.log.2` (`:132-135`),
  3. renombra `mito.log` → `mito.log.1` (`:136`).
- Resultado: como mucho `mito.log`, `mito.log.1`, `mito.log.2`, `mito.log.3` (cuatro ficheros).
- La rotación ocurre dentro del lock de la carpeta (`guard`, `:128`).

### Lock (`guard`, `:80-111`)
- Fichero `<carpeta>/.diagnostics.lock`, abierto `a+b` (se crea si falta). Si está vacío, se escribe un byte `\0` (`:85-86`) para que exista el byte 0.
- Bloqueo: en Windows `msvcrt.locking(fd, LK_NBLCK, 1)` sobre el byte 0 (`:94`). En Unix `fcntl.flock(LOCK_EX | LOCK_NB)` (`:97`).
- Compatible con `FileStream.Lock` de .NET usado en `bootstrap.ps1`, según el comentario de `:82`. Duda: el bootstrap PowerShell está en `core/bootstrap.ps1` y no se ha leído en esta tarea.
- Además, un `threading.RLock` de proceso (`_mutex`, `:18`, `:84`).
- Reintentos: 10 intentos separados por 0,02 s (`:89-101`), unos 200 ms como máximo. Si no se consigue, `OSError('Diagnostic lock busy')` (`:103`).
- Desbloqueo: Windows `LK_UNLCK` (`:110`), Unix `flock(LOCK_UN)` (`:111`).

## Invariantes
1. `write` nunca lanza. Cualquier error (incluido `OSError` de cada carpeta) se traga (`:143-144`, `:148-149`).
2. Cada registro es una línea JSON completa. Los escritores concurrentes no se intercalan (test de 4 hilos × 20 escrituras, 80 líneas) porque cada escritura se hace bajo `guard`.
3. Todo texto que llega al log pasa por `scrub` (operación, mensaje, componente de `step`). Los valores de `exceptions` pasan por `message`.
4. Los detalles son solo etiquetas explícitas pasadas por el código. Nunca se vuelcan argumentos de línea de comandos, variables de entorno ni locales (`:120`).
5. Un registro mayor de 128 KiB pierde `exceptions` y gana `truncated: true` (`:122-125`).
6. Se mantienen como máximo cuatro ficheros de log por carpeta.
7. Si la carpeta principal falla, `write` prueba la alternativa. Una carpeta alternativa usada se anuncia una sola vez por proceso.

## Casos límite y errores

### `scrub(root, value)` (`:27-47`), en este orden
1. **URLs** (`:37`, `https?://[^\s<>"']+`, sin distinguir mayúsculas): se conservan esquema, host y ruta. Se quitan `user:pass@` (se queda el host), la query se sustituye por `[redacted]` si existe, y el fragmento se elimina (`urlunsplit`, `:34`). Si `urlsplit` lanza `ValueError`, la URL se sustituye por `[url]` (`:35-36`).
2. **Tokens de GitHub/PostHog** (`:38`): `(?i)\b(?:ph[cp]_|gh[pousr]_)[A-Za-z0-9_]+` → `[redacted]`.
3. **Bearer** (`:39`): `Bearer <token>` → `Bearer [redacted]`.
4. **Perfil de usuario de Windows** (`:40`): `[A-Z]:[\\/]Users[\\/][^\\/\s]+` → `<USER>`.
5. **Pares clave-valor secretos** (`:41`, sin distinguir mayúsculas): claves `password`, `passwd`, `pwd`, `token`, `api_key`/`api-key`, `secret`, `authorization`, `cookie`, con separador `:` o `=`. El valor puede ser `"…"`, `'…'` o `[^\s,;]+`. Se sustituye por `[redacted]` y se conserva la clave y el separador.
6. **Rutas**: la raíz resuelta se sustituye por `<MITO>` (`:42-45`), y `USERPROFILE` por `<USER>`. Se prueba con `\` y con `/` (`:46`). Sin distinguir mayúsculas.
7. **Longitud**: el resultado se recorta a 12 000 caracteres (`:47`).

Orden: la regla 3 (Bearer) se aplica antes que la 5, así que `Authorization: Bearer X` queda como `Authorization: [redacted] [redacted]`. Duda: ¿es aceptable esta salida?

### `message(root, exc)` (`:50-55`)
- `CalledProcessError` → `Child process failed with exit code {returncode}` (sin argumentos).
- `TimeoutExpired` → `Child process exceeded timeout of {timeout} seconds`.
- Resto → `scrub(root, exc)`.

### `step(root, operation, phase, component=None)` (`:67-77`)
- Escribe de inmediato un registro `info` con `operation` y `message = phase` (el texto es la fase; no hay clave `phase`). El componente va en `component`, saneado con `scrub`, o `null`.
- Si el bloque lanza, y la excepción no tiene `_mito_phase`, le asigna `_mito_phase = phase` y `_mito_component = component` (sin sanear). Después re-lanza la misma excepción. La primera fase gana.
- Duda: en Rust no hay atributos en excepciones. ¿Se usará un tipo de error con campos de contexto?

### `write(root, level, operation, text, **details)` (`:114-150`)
- Si la carpeta principal falla, sigue con la siguiente (`:126`, `:143`).
- Mensajes únicos por proceso, en `stderr`, en español y **sin** `tr()`:
  - `MITO: log alternativo: {folder}` (`:141`), una vez por carpeta.
  - `MITO: no se pudo escribir el log local ni el alternativo.` (`:147`), una vez por proceso.
- Devuelve `folder / 'mito.log'` al escribir con éxito (`:142`), o `None`.

### `locations(root)` (`:22-24`)
- Primaria: `<root>/logs`.
- Alternativa: `<tempfile.gettempdir()>/MITO-logs/<identidad>`, donde `identidad = sha256(str(Path(root).resolve()).lower().encode()).hexdigest()[:16]` (`:23`). Se calcula sobre la ruta en minúsculas, así que dos rutas que solo difieren en mayúsculas comparten carpeta.

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_rotation_keeps_three_backups_and_valid_json_lines` | Con `MAX_BYTES=600`, 12 escrituras dejan 4 ficheros `mito.log*` de máximo 600 bytes, cada línea es JSON válido con `operation='test'`, y la última línea de `mito.log` tiene `message` que empieza por `11`. |
| `test_concurrent_writers_keep_complete_records` | 4 hilos × 20 escrituras dejan 80 líneas completas y válidas en `logs/mito.log`. |
| `test_fallback_and_failure_do_not_raise` | Si la carpeta principal es un fichero, `write` devuelve `fallback/mito.log`. Si ambas fallan, devuelve `None` sin lanzar. |
| `test_scrub_keeps_explanation_but_removes_known_secrets` | `scrub` elimina `SECRET`, `two words`, `user:password`, `phc_…`, el nombre de usuario y la raíz, pero conserva `Download failed` y `example.com/path`. |
| `test_subprocess_error_does_not_log_command_arguments` | `message` de `CalledProcessError` devuelve exactamente `Child process failed with exit code 7`, sin argumentos `--password secret`. |
| `test_bootstrap_welcome_requires_explicit_yes` (solo Windows) | Integración PowerShell: `Confirm-YitoBootstrap` acepta solo `s`, `si`, `sí` (sin distinguir mayúsculas, con recorte). Ejecuta `core/bootstrap.ps1`. |
| `test_bootstrap_cancellation_creates_no_tools_or_pending_events` (solo Windows) | Integración PowerShell: cancelar la confirmación devuelve código 2, no crea `core/tools` ni `logs/pending`, y el último registro tiene `phase='confirmation'` y `level='info'`. |
| `test_python_retries_powershell_pending_event_without_real_network` (solo Windows) | Integración PowerShell y `error_tracking`: un error del bootstrap queda en `logs/pending`, `resume` lo envía una vez (`_send` simulado) con `mito_phase='download-python'`, y borra el fichero. Depende de PostHog. |
| `test_real_bootstrap_records_failure_before_python_exists` (solo Windows) | Integración PowerShell: un `bootstrap.lock.json` inválido devuelve código 1, el último registro tiene `phase='read-bootstrap-lock'` y `level='error'`, y no crea Python ni `logs/pending`. |
| `test_powershell_bootstrap_log_without_python_or_network` (solo Windows) | Integración PowerShell: un error con `password=SECRET` no deja `SECRET` en el log, el registro tiene `phase`, `level='error'`, `windows`, y Python puede seguir escribiendo en el mismo fichero. |

Notas de clasificación: solo los tests 1-5 son de la lógica Python de este módulo. Los tests 6-10 dependen de `core/bootstrap.ps1` (PowerShell 5.1, Windows) y, en dos casos, de `error_tracking` (outbox de PostHog). Duda: ¿se portan o se sustituyen por tests de integración Rust?

## Dependencias
- Biblioteca estándar únicamente (`contextlib`, `datetime`, `hashlib`, `json`, `os`, `pathlib`, `re`, `subprocess`, `sys`, `tempfile`, `threading`, `time`, `urllib.parse`).
- Dependientes: `error_tracking` (usa `write`, `scrub`, `message`, `environment`, `guard`, `locations`), `__main__` (CLI), y `bootstrap.ps1` (mismo formato de línea y mismo lock).

## Notas para Rust
- **Formato JSON**: `serde_json` produce JSON compacto sin espacios tras `:` y `,`. Python usa `", "` y `": "`. Para coincidir byte a byte, serializar con un formatter propio o con `serde_json::ser::CompactFormatter` adaptado. Duda: ¿es necesaria la coincidencia byte a byte o basta con que cada línea sea JSON válido? Sin embargo, `ensure_ascii=True` (escape `\uXXXX`, pares sustitutos para caracteres fuera del BMP) no lo hace `serde_json` por defecto. Hay que escribir ese escape.
- **Fecha**: `chrono` o `time`, formato `%Y-%m-%dT%H:%M:%S%.6f+00:00`. Duda: Python omite los microsegundos cuando valen 0.
- **Hash**: `sha2` y `hex`. `Path::resolve()` en Windows devuelve `\\?\`; usar `dunce::canonicalize` antes de hashear. Confirmar que `to_lowercase()` coincide con `.lower()` de Python para la ruta.
- **Lock**: `windows-sys` `LockFileEx` sobre el byte 0 (`LOCKFILE_FAIL_IMMEDIATELY`) para compatibilidad con .NET. En Unix, `flock` con `rustix` o `fs4`. Reintentos: 10 × 20 ms.
- **RLock**: `std::sync::ReentrantMutex` no existe en la biblioteca estándar. Usar `parking_lot::ReentrantMutex` si se necesita reentrancia. Duda: ¿hace falta reentrancia? `write` no reentra en el original.
- **Rotación**: `std::fs::rename` reemplaza en Windows (`MoveFileEx`). Mantener el orden de renombrado.
- **Escritura**: `OpenOptions::new().create(true).append(true)`, un solo `write_all`.
- **Versión de Windows**: `sys.getwindowsversion()` no tiene equivalente directo en Rust. Duda: ¿`RtlGetVersion` (`windows-sys`) o el dato de `ntdll`?
- **Mensajes de consola**: `stderr` en español sin `tr()`. Duda: ¿deben traducirse? El test de i18n solo comprueba `tr('…')` literales.
- **Diferencia de formato**: el bootstrap PowerShell escribe `phase` y `windows` en el nivel superior. Python escribe `runtime.windows` y `phase` solo dentro de los detalles de `capture`. Duda: ¿se unifica el esquema en Rust?
- **Límite de tamaño**: `write` no vuelve a recortar tras quitar `exceptions`. Duda: ¿debe acotarse también el mensaje?
