# error_tracking.py → mito-core::error_tracking (solo parte local)

Origen: `core/app/mito/error_tracking.py` (272 líneas), `core/tests/test_error_tracking.py` (15 tests).

## Responsabilidad
Captura de excepciones de MITO y registro local en `logs/mito.log` mediante `diagnostics.write`. Esta tarea solo porta la parte local: construir la lista de excepciones saneada, registrarla en el log local y los ganchos de captura. Toda la parte de envío a PostHog (outbox, HTTP, reintentos, límites de cola) **no se porta**.

## Clasificación: qué se porta y qué se descarta

### Se porta (local)
| Elemento | Líneas | Motivo |
|---|---|---|
| `exception_list(exc, handled=True, root=None)` | `:28-53` | Construye la traza saneada que se guarda en el log local (`exceptions`). |
| Saneado de `value` vía `diagnostics.message` | `:48` | Privacidad del log. |
| Registro local en `capture`: `diagnostics.write(root, 'error', operation, message, event_id, version, runtime, exceptions, component, phase)` | `:218-222` | Es el registro que se garantiza en disco aunque PostHog esté desactivado (test `test_local_log_exists_when_posthog_is_disabled`). |
| `capture` como punto de entrada: solo `Exception` (no `BaseException`), devuelve `bool` | `:213-216` | Contrato de la API. El tipo de retorno deberá revisarse sin PostHog. |
| Contexto de fase/componente: lectura de `_mito_phase` / `_mito_component` de la excepción | `:74-75`, `:81-83` | Proviene de `diagnostics.step`; el contexto va en el log local. |
| `report_threads(root)` (gancho `threading.excepthook`) | `:266-272` | Registra errores no capturados de hilos. Duda: ¿se mantiene si el gancho llama solo a la parte local? |

### Se descarta (PostHog)
| Elemento | Líneas | Motivo |
|---|---|---|
| `_configuration(root)` | `:56-66` | Lee `posthog.enabled`, `posthog.host`, `posthog.api_key` y valida la URL. |
| `_destination(host, token)` | `:69-70` | Identidad de la cola para no enviar a otro proyecto. |
| `_payload(...)` como formato PostHog | `:73-85` | `distinct_id`, `$process_person_profile`, `$geoip_disable`, `$lib`, `event: '$exception'`, `properties`. El formato `$exception_list` es el de PostHog; la lista interna se conserva (ver arriba). |
| `_send(root, host, token, payload, timeout=5)` | `:88-95` | HTTP `POST <host>/i/v0/e/`, `NoRedirect`, timeout 5 s. |
| `_files`, `_prune`, `_store` (cola en disco) | `:98-137` | Outbox en `logs/pending/` y `<tmp>/MITO-logs/<id>/pending/`. |
| `_enqueue`, `_drain`, hilo `mito-errors`, `_pending`, `_worker`, `_lock` | `:17-19`, `:140-186` | Cola y worker de envío. |
| `resume(root)` | `:189-210` | Reintenta eventos pendientes al arrancar. |
| `check(root)` (`posthog-check`) | `:239-256` | Envío diagnóstico explícito. |
| `flush(timeout=6)` | `:259-263` | Espera al worker. |
| `_seen`, `_attempted` y sus límites de 20 | `:20-21`, `:143`, `:226` | Deduplicación y límites por proceso para envío. Duda: ¿hay que deduplicar también el registro local? En el original, el registro local se escribe siempre, antes de la deduplicación (`:218` frente a `:224-228`). |
| Constantes del outbox `MAX_EVENTS=100`, `MAX_EVENT_BYTES=128*1024`, `MAX_AGE=7*86400`, `queue.Queue(maxsize=20)` | `:17`, `:23-25` | Solo afectan al outbox. |
| Comandos CLI `posthog-check` | `mito.__main__` (no leído) | Descartar. |

Nota para la fase de portado: el `root` por defecto de `exception_list` usa `source.parents[2]` (`:31`), y la traza usa la ruta relativa de `mito/` (`:40`). Ambos dependen de la estructura de carpetas del original.

## API pública del original (parte local)
| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `exception_list(exc, handled=True, root=None) -> list[dict]` (`:28-53`) | excepción, si fue manejada, raíz | lista de entradas de excepción (máx. 5) | Nunca lanza, salvo bug. |
| `capture(root, exc, *, handled=True, operation='error', component=None, phase=None) -> bool` (`:213-236`) | raíz, excepción, opciones | `True` si el evento se encoló para envío (en el port local, sin sentido). | `ValueError` y resto se tragan (`:232-236`). |
| `report_threads(root) -> hook anterior` (`:266-272`) | raíz | función anterior | — |

## Formatos de disco (local)
Solo el registro del log local, que escribe `diagnostics.write`. Ejemplo de la forma de la línea (ver `diagnostics.md`):
```
{"time": "<ISO-8601 UTC>", "level": "error", "pid": <pid>, "operation": "install", "message": "…", "event_id": "<uuid v4>", "version": "<versión>", "runtime": {"python": "…", "bits": 64, "platform": "win32"}, "exceptions": [{"type": "ValueError", "value": "password=[redacted] <USER>/file.txt", "module": "builtins", "mechanism": {"handled": true, "synthetic": false}, "stacktrace": {"type": "raw", "frames": [{"platform": "custom", "lang": "python", "filename": "mito/<ruta>.py", "function": "error", "lineno": 28, "in_app": true}]}}], "component": null, "phase": null}
```
Nota: `value` de la excepción debe coincidir con el `message` del registro (mismo saneado).

El campo `event_id` es el `uuid4` del evento (`:84`, `:219`). Se conserva en el registro local aunque no haya envío.

## Invariantes (parte local)
1. `capture` nunca lanza: devuelve `False` ante `BaseException` que no sea `Exception` (`:214-215`), y ante cualquier error (`:232-236`).
2. Un `KeyboardInterrupt` no se registra en el log local (la comprobación ocurre antes de escribir, `:214`).
3. El registro local se escribe **antes** de comprobar la configuración de PostHog (`:218` antes de `:223`). Por eso existe aunque PostHog esté desactivado.
4. Todo texto del registro pasa por `diagnostics.scrub`/`message` (`:48`, `:79`, `:83`, `:218`).
5. La lista de excepciones sigue `__cause__` y, si no hay, `__context__` salvo `__suppress_context__` (`:52`), sin repetir objetos (`visited`, `:29`, `:32`), y máximo 5 (`:32`).
6. Solo se incluyen frames de cada traceback, como máximo los últimos 50 (`:51`).
7. Un frame cuyo fichero está dentro del paquete `mito/` se guarda como `mito/<ruta relativa>` con `in_app=true` (`:40-41`). Fuera del paquete, solo el nombre base del fichero con `in_app=false` (`:43`).
8. Sin variables locales, argumentos ni entorno en las trazas (solo `function`, `lineno`, nombre de fichero).
9. `_mito_phase` y `_mito_component` de la excepción tienen prioridad sobre los argumentos `phase`/`component` (`:74-75`).

## Casos límite y errores
- Excepción encadenada de 6 o más niveles: solo se registran 5 (`:32`).
- Ciclos en `__cause__`/`__context__`: se cortan por `id()` (`:29`, `:32`).
- Traceback vacío: `frames = []` (`:36-37`).
- `CalledProcessError` y `TimeoutExpired`: el `value` es el mensaje genérico de `diagnostics.message` sin argumentos.
- `exception_list` llamado con `root=None` usa la raíz derivada de la ubicación del módulo (`:31`). Duda: en Rust, ¿cómo se obtiene la raíz?
- `capture` con `handled=False` (ganchos de hilos): `mechanism.handled=false`.
- `capture` con `component`/`phase` nulos: `component` y `phase` del registro son `null` (`:221-222`).

Mensajes del log local: `error` con `operation` (`error` por defecto, `cli.<comando>` desde la CLI, `thread.unhandled` desde `report_threads`).

## Casos de test del original
| Test | Clasificación | Qué comprueba |
|---|---|---|
| `test_disabled_does_not_send` | PostHog | Con PostHog desactivado, `capture` devuelve `False` y no envía. El registro local lo cubre `test_local_log_exists_when_posthog_is_disabled`. |
| `test_trace_is_private_and_deduplicated` | Mixta | Local: el payload no contiene `private` ni la raíz de la carpeta, y el último frame es `error`. PostHog: segundo `capture` devuelve `False` por deduplicación, y el envío tiene `$process_person_profile=False`. Portar la parte de privacidad. |
| `test_chain_and_interrupt` | Local | `exception_list` devuelve `[RuntimeError, OSError]` en ese orden con `mechanism.handled=false`, y `capture(KeyboardInterrupt())` devuelve `False`. |
| `test_process_limit` | PostHog | Como máximo 20 capturas enviadas por proceso (`_seen`/`_attempted`). |
| `test_incomplete_configuration_and_invalid_host` | PostHog | Host vacío, `file://`, URL con credenciales o con query no se envían. |
| `test_send_failure_is_not_retried` | PostHog | Un fallo de envío se intenta una vez y la cola queda vacía (`unfinished_tasks == 0`). |
| `test_http_payload_and_timeout` | PostHog | `_send` hace `POST <host>/i/v0/e/` con `token` en el cuerpo, timeout 5 s, y `NoRedirect`. |
| `test_cli_reports_and_preserves_exit_code` | Local | La CLI `installed` con error de `Engine.initialize` llama a `capture(root, error, operation='cli.installed')` y devuelve código 1. Portar el cableado de la CLI. |
| `test_explicit_diagnostic_reports_http_result` | PostHog | `check` devuelve el código HTTP y envía con timeout de 10 s y `mito_diagnostic=True`. |
| `test_diagnostic_failure_is_visible_and_not_recaptured` | PostHog | El comando `posthog-check` con fallo sale con código 1, muestra el error en `stderr` y no llama a `capture`. |
| `test_local_log_exists_when_posthog_is_disabled` | Local | Con PostHog desactivado, `capture` escribe en `logs/mito.log` un registro con `operation='install'`, `exceptions[0].type='ValueError'`, sin el texto `private` y sin carpeta `pending/`. |
| `test_offline_event_survives_and_retries_with_same_uuid` | PostHog | Un evento sin conexión queda en `logs/pending/`, sin la clave del proyecto en claro, y se reenvía con el mismo `uuid` y `timestamp` tras reiniciar. |
| `test_outbox_does_not_send_to_different_project_or_when_disabled` | PostHog | Los eventos pendientes no se envían si cambia la clave del proyecto o si PostHog está desactivado. |
| `test_disk_outbox_is_bounded_and_expires` | PostHog | La cola en disco se limita a `MAX_EVENTS` (2 en el test) y elimina ficheros de más de 7 días. |
| `test_report_includes_message_context_and_no_geoip` | Mixta | Local: `mito_phase='extract'` y `mito_component='main/example'` vienen de `diagnostics.step`, y el mensaje saneado conserva `Cannot extract` pero no `two words`. PostHog: `$geoip_disable=True`. |

Resumen: 3 locales (`chain_and_interrupt`, `local_log_exists_when_posthog_is_disabled`, `cli_reports_and_preserves_exit_code`), 2 mixtos (`trace_is_private_and_deduplicated`, `report_includes_message_context_and_no_geoip`), 10 PostHog (`disabled_does_not_send`, `process_limit`, `incomplete_configuration_and_invalid_host`, `send_failure_is_not_retried`, `http_payload_and_timeout`, `explicit_diagnostic_reports_http_result`, `diagnostic_failure_is_visible_and_not_recaptured`, `offline_event_survives_and_retries_with_same_uuid`, `outbox_does_not_send_to_different_project_or_when_disabled`, `disk_outbox_is_bounded_and_expires`).

## Dependencias
- `mito.__version__` (`:14`): versión del paquete. Se escribe en `version` del registro (`:219`).
- `diagnostics` (`:14`): `write`, `scrub`, `message`, `environment`, `guard`, `locations`. Solo `write`, `scrub`, `message` y `environment` son parte local.
- `network` (`:15`, no leído): `NoRedirect`, `opener`, `properties`, `validate_url`. Solo PostHog.
- `mito.__main__` (test, no leído): `main(argv)` llama a `capture` en la CLI.
- Sin dependencias de `i18n`: los mensajes de `ValueError` de la parte PostHog están en español y no pasan por `tr()`.

## Notas para Rust
- **Crates**: solo lo necesario para la parte local. `serde_json` para el registro y `uuid` (`v4`) para `event_id`.
- **Frames**: los frames de Python (`filename`, `function`, `lineno`) no tienen equivalente directo en Rust. Duda: ¿qué información de origen se registra (`#[track_caller]`, `std::backtrace::Backtrace`)?
- **Excepciones encadenadas**: en Rust, `std::error::Error::source()` sustituye a `__cause__`. `__context__` no tiene equivalente. Duda: ¿se mapea solo `source()`?
- **Traza**: Rust no tiene `__traceback__`. Duda: ¿se sustituye por la ubicación de origen (`#[track_caller]`) o por una captura de `std::backtrace::Backtrace`? La salida no será idéntica.
- **Panic**: `std::panic::set_hook` es el equivalente de `threading.excepthook` para panics. Duda: ¿se mapea `report_threads` a `set_hook`?
- **`handled`**: parámetro explícito de `capture` (`bool`).
- **Tipo de retorno**: `capture` en Python devuelve `bool`. Sin PostHog la semántica se reduce a «registrado». Duda: ¿se devuelve `()` o un `Result`?
- **Saneado**: reutilizar la función de `diagnostics` (`scrub`/`message`) del crate `mito-core::diagnostics`.
- **Dependencia cruzada**: el test del CLI exige que `main` llame a `capture` con `operation='cli.<comando>'`. Documentar en el crate de la CLI.
- **Duda abierta**: ¿se mantiene el campo `event_id` en el registro local si ya no hay envío? Se conserva por compatibilidad con el registro actual.
- **Duda abierta**: la deduplicación (`_seen`) afecta solo al envío. En Rust no se implementa a no ser que el envío se reintroduzca. Confirmar.
