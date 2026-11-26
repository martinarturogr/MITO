# operation.py → mito_transfer::operation

Origen: `core/app/mito/operation.py` (110 líneas). Tests que lo ejercitan: `core/tests/test_transfer.py` (cancelación, 317-342) y `core/tests/test_ui.py` (diálogo de progreso, 394-418). No tiene test propio.

Nota de nombre: el crate es `mito-transfer`; el módulo Rust es `mito_transfer::operation`.

## Responsabilidad

Progreso y cancelación cooperativa de la operación en curso del hilo actual. La interfaz instala un `Operation` con `monitored()` y `transfer.py` lo lee con `current()` sin pasar parámetros adicionales. Sin `Operation` activa, todas las funciones auxiliares son no-op (salvo `wait`, que duerme).

Este contrato cubre solo la API que usa `transfer.py` (y el monitor que recibe). El resto del módulo (`phase`, `checkpoint`) lo usa `engine.py` y queda fuera de este contrato, salvo mención.

## API pública del original

| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `Cancelled(YitoError)` (17) | mensaje | excepción | n/a. Mensaje de `check()`: `Instalación cancelada` (línea 60, traducible) |
| `Operation(listener=None)` (21) | callable opcional `listener(dict)` | objeto con estado | n/a |
| `Operation.transfer(received, total=None, unit='bytes')` (38) | bytes recibidos, total o `None` | `None` | n/a (emite `listener` con copia del estado) |
| `Operation.note(text)` (42) | texto | `None` | n/a |
| `Operation.phase(title, text, cancellable=True)` (32) | título, texto, flag | `None` (usado por `engine.py`, no por transfer) | n/a |
| `Operation.finish()` (46) | — | `None`; `active = False` | n/a |
| `Operation.cancelled` (50) | — | `bool` (propiedad) | n/a |
| `Operation.cancel()` (54) | — | `None`; invocable desde cualquier hilo | n/a |
| `Operation.check()` (58) | — | `None` | `Cancelled` si está cancelada |
| `Operation.wait(seconds)` (62) | segundos (≥ 0) | `None` | `Cancelled` si se cancela durante la espera (la espera termina antes) |
| `current()` (67) | — | `Operation` activa del contexto, o `None` | n/a |
| `monitored(operation)` (71) | `Operation` | context manager | propaga excepciones del bloque; al salir llama `finish()` |
| `checkpoint()` (81) | — | `None` | `Cancelled` (usado por `engine.py`) |
| `transfer(received, total=None, unit='bytes')` (93) | — | `None`; no-op sin operación | n/a |
| `note(text)` (99) | — | `None`; no-op sin operación | n/a |
| `wait(seconds)` (105) | — | `None`; sin operación duerme `time.sleep` | `Cancelled` si la operación se cancela |

### Clave del estado (`Operation.state`, líneas 25-26)

Diccionario inicial: `title=''`, `text=''`, `received=None`, `total=None`, `unit='bytes'`, `note=''`, `cancellable=True`, `active=False`. `phase()` añade `started` (`time.monotonic()`). `listener` recibe una copia (`dict(self.state)`) en cada cambio.

## Formatos de disco

No escribe en disco. Solo emite diccionarios al `listener`.

Ejemplo de estado tras `phase('DESCARGANDO', 'Descargando extras/antigravity 2.35.1 (1/1)')` y `transfer(5*1024*1024, 20*1024*1024)` (test_ui.py:397-398):

```
{'title': 'DESCARGANDO', 'text': 'Descargando extras/antigravity 2.35.1 (1/1)',
 'received': 5242880, 'total': 20971520, 'unit': 'bytes', 'note': '',
 'cancellable': True, 'active': True, 'started': <float>}
```

## Invariantes

1. Sin `Operation` activa, `transfer`, `note` y `phase` no hacen nada y `wait` solo duerme (líneas 93-110).
2. `Operation.cancel()` es seguro desde cualquier hilo: solo activa un `threading.Event` (línea 56).
3. Tras `cancel()`, `check()` y `wait()` lanzan `Cancelled` (líneas 58-65).
4. `cancellable` de `phase()` se pone a `False` si la operación ya está cancelada (línea 34).
5. `monitored()` restaura el contexto anterior (`ContextVar.reset`) y llama a `finish()` aunque el bloque lance (líneas 72-78).
6. `current()` es por contexto (`contextvars.ContextVar`, línea 14). Un hilo nuevo no hereda la operación: `transfer.download` captura el monitor al inicio (transfer.py:207) y lo pasa como `check` al hilo de red.
7. `transfer` no comprueba cancelación; la comprobación la hace quien llama con `check` o `wait`.

## Casos límite y errores

- **Cancelación durante la lectura de red**: la lectura corre en hilo aparte (`_Stream`, transfer.py:140-190). El hilo principal sondea la cola cada 0.25 s y llama `check()` (transfer.py:177-187). Por eso una lectura bloqueada no impide cancelar.
- **Cancelación durante la espera de reintento**: `wait` retorna con `Cancelled` antes de que expire la espera (transfer.py:232-235).
- **Cancelación antes de la primera lectura**: el primer `check` se hace al empezar cada intento (transfer.py:245, `stop_if_cancelled`).
- **Listener que lanza**: no se captura en `_emit`; la excepción sube al llamador de `transfer`/`note`. Duda 2.

## Casos de test del original

| Test | Archivo | Qué comprueba |
|---|---|---|
| `test_cancellation_interrupts_a_stalled_read_and_discards_the_partial_file` (317-342) | `test_transfer.py` | `Operation(listener)` con `monitored`; el listener marca `progressed` cuando `state['received']` es verdadero; un hilo llama `cancel()` tras el progreso; `download` lanza `Cancelled` en menos de 15 s y no deja archivo |
| `test_install_progress_dialog_shows_bytes_and_cancels_the_installation` (test_ui.py:394-418) | `test_ui.py` | Diálogo `OperationDialog` muestra fase, bytes (5 MiB de 20 MiB) y, al pulsar `operation-cancel` (ui.py:735-736), `operation.cancel()` interrumpe el `wait` de 0.05 s en bucle y el diálogo se cierra |

## Dependencias

- `mito.i18n.tr` (línea 11, mensaje de `check`).
- `mito.storage.YitoError` (línea 12): `Cancelled` hereda de ella.
- Stdlib: `contextvars`, `threading`, `time`, `contextlib.contextmanager`.
- Consumidores: `transfer.py` (ver `transfer.md`), `engine.py` (`phase`, `checkpoint`, `transfer`), `components.py:273-275`, `ui.py:709-736, 1560-1574, 1708`.

## Notas para Rust

- Estado: `struct Operation { state: Mutex<State>, cancelled: AtomicBool, listener: Option<Box<dyn Fn(&State) + Send + Sync>> }`. El `Event` de Python equivale a `AtomicBool` más `Condvar` si hace falta esperar con timeout (`wait` debe despertar al cancelar).
- Contexto por hilo: `thread_local!` o un parámetro explícito. El original usa `ContextVar`, que no se hereda al crear hilos. Mantener la misma regla: `transfer::download` captura el handle antes de lanzar el hilo de red.
- `wait(seconds)` sin operación equivale a `std::thread::sleep`. Con operación: `Condvar::wait_timeout` y, si está cancelada, devolver `Err(Cancelled)`.
- `Cancelled` es una variante de error propia (no una excepción de sistema). Su texto `Instalación cancelada` pasa por i18n.

### Dudas

- Duda 1: `Operation.wait` con segundos negativos usa `max(0, seconds)`. En Rust, `Duration::from_secs_f64` falla con negativos; documentar el clamp.
- Duda 2: si el listener lanza una excepción, el original la propaga desde `transfer` o `note`. ¿Rust debe aislar el listener (`catch_unwind`) o propagar?
- Duda 3: `cancelled` es cualquier `cancel()` previo, incluso antes de empezar la descarga. ¿Debe `monitored()` reiniciar el flag al entrar? El original no lo hace.
