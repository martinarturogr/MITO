//! Progreso y cancelación cooperativa (`operation.py` del original).
//!
//! Diferencia deliberada (ADR 0008): el original usa una `ContextVar` implícita; aquí
//! el [`Operation`] se pasa explícitamente. [`Operation::detached`] es la operación
//! vacía: no notifica y no se puede cancelar, igual que llamar a las funciones del
//! original sin operación activa.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Estado visible de una operación (el diccionario `state` del original).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationState {
    /// Título de la fase (p. ej. `DESCARGANDO`).
    pub title: String,
    /// Descripción de la fase.
    pub text: String,
    /// Unidades recibidas en la fase actual.
    pub received: Option<u64>,
    /// Total esperado, si se conoce.
    pub total: Option<u64>,
    /// Unidad de `received`/`total` (`bytes` por defecto).
    pub unit: String,
    /// Último aviso de la fase.
    pub note: String,
    /// Si la fase admite cancelación.
    pub cancellable: bool,
    /// Si hay una fase en curso.
    pub active: bool,
    /// Inicio de la fase actual.
    pub started: Option<Instant>,
}

impl Default for OperationState {
    fn default() -> Self {
        Self {
            title: String::new(),
            text: String::new(),
            received: None,
            total: None,
            unit: "bytes".to_owned(),
            note: String::new(),
            cancellable: true,
            active: false,
            started: None,
        }
    }
}

type Listener = Box<dyn Fn(&OperationState) + Send + Sync>;

struct Inner {
    state: Mutex<OperationState>,
    cancelled: Mutex<bool>,
    wake: Condvar,
    listener: Option<Listener>,
}

/// Operación en curso, compartible entre hilos (clonar comparte el mismo estado).
#[derive(Clone, Default)]
pub struct Operation {
    inner: Option<Arc<Inner>>,
}

impl fmt::Debug for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field("detached", &self.inner.is_none())
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Un listener que entra en pánico no debe bloquear la cancelación.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Operation {
    /// Operación que notifica cada cambio a `listener` con una copia del estado.
    pub fn new(listener: impl Fn(&OperationState) + Send + Sync + 'static) -> Self {
        Self::build(Some(Box::new(listener)))
    }

    /// Operación cancelable sin listener.
    pub fn silent() -> Self {
        Self::build(None)
    }

    /// Operación vacía: los avisos son no-op y nunca se cancela.
    pub fn detached() -> Self {
        Self { inner: None }
    }

    fn build(listener: Option<Listener>) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                state: Mutex::new(OperationState::default()),
                cancelled: Mutex::new(false),
                wake: Condvar::new(),
                listener,
            })),
        }
    }

    fn update(&self, change: impl FnOnce(&mut OperationState)) {
        if let Some(inner) = &self.inner {
            let snapshot = {
                let mut state = lock(&inner.state);
                change(&mut state);
                state.clone()
            };
            if let Some(listener) = &inner.listener {
                listener(&snapshot);
            }
        }
    }

    /// Copia del estado actual (estado por defecto si está desacoplada).
    pub fn state(&self) -> OperationState {
        self.inner
            .as_ref()
            .map_or_else(OperationState::default, |inner| lock(&inner.state).clone())
    }

    /// Empieza una fase y reinicia el progreso. Una operación ya cancelada deja de ser
    /// cancelable.
    pub fn phase(&self, title: impl Into<String>, text: impl Into<String>, cancellable: bool) {
        let cancelled = self.is_cancelled();
        let (title, text) = (title.into(), text.into());
        self.update(|s| {
            *s = OperationState {
                title,
                text,
                cancellable: cancellable && !cancelled,
                active: true,
                started: Some(Instant::now()),
                ..OperationState::default()
            };
        });
    }

    /// Informa del progreso de la fase.
    pub fn transfer(&self, received: u64, total: Option<u64>, unit: &str) {
        self.update(|s| {
            s.received = Some(received);
            s.total = total;
            unit.clone_into(&mut s.unit);
        });
    }

    /// Publica un aviso de la fase.
    pub fn note(&self, text: impl Into<String>) {
        let text = text.into();
        self.update(|s| s.note = text);
    }

    /// Marca la operación como terminada (`active = false`).
    pub fn finish(&self) {
        self.update(|s| s.active = false);
    }

    /// Indica si se pidió la cancelación.
    pub fn is_cancelled(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| *lock(&inner.cancelled))
    }

    /// Pide la cancelación. Seguro desde cualquier hilo; despierta a quien espere en
    /// [`Self::wait`]. No hace nada en una operación desacoplada.
    pub fn cancel(&self) {
        if let Some(inner) = &self.inner {
            *lock(&inner.cancelled) = true;
            inner.wake.notify_all();
        }
    }

    /// Punto de cancelación.
    ///
    /// # Errors
    ///
    /// [`Error::Cancelled`] si se pidió la cancelación.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Espera `duration`, o menos si se cancela antes.
    ///
    /// # Errors
    ///
    /// [`Error::Cancelled`] si la operación se cancela antes o durante la espera.
    pub fn wait(&self, duration: Duration) -> Result<()> {
        let Some(inner) = &self.inner else {
            std::thread::sleep(duration);
            return Ok(());
        };
        let guard = lock(&inner.cancelled);
        let (guard, _) = inner
            .wake
            .wait_timeout_while(guard, duration, |cancelled| !*cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        if *guard {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Ejecuta `body` con `operation` y la marca como terminada al salir, también si
/// `body` falla (`monitored` del original).
pub fn monitored<T>(operation: &Operation, body: impl FnOnce(&Operation) -> T) -> T {
    struct Finish<'a>(&'a Operation);
    impl Drop for Finish<'_> {
        fn drop(&mut self) {
            self.0.finish();
        }
    }
    let _finish = Finish(operation);
    body(operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn la_operacion_desacoplada_no_hace_nada() {
        let operation = Operation::detached();
        operation.phase("T", "x", true);
        operation.cancel();
        assert!(!operation.is_cancelled());
        assert!(operation.check().is_ok());
        assert!(operation.wait(Duration::ZERO).is_ok());
        assert_eq!(operation.state(), OperationState::default());
    }

    #[test]
    fn notifica_fase_y_progreso_como_el_original() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let operation = Operation::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        operation.phase("DESCARGANDO", "Descargando extras/app 1.0 (1/1)", true);
        operation.transfer(5 * 1024 * 1024, Some(20 * 1024 * 1024), "bytes");
        let state = operation.state();
        assert_eq!(state.title, "DESCARGANDO");
        assert_eq!(state.received, Some(5_242_880));
        assert_eq!(state.total, Some(20_971_520));
        assert!(state.active && state.cancellable && state.started.is_some());
        operation.note("Aviso");
        operation.phase("OTRA", "", true);
        assert_eq!(operation.state().received, None);
        assert_eq!(operation.state().note, "");
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn una_fase_tras_cancelar_no_es_cancelable() {
        let operation = Operation::silent();
        operation.cancel();
        operation.phase("T", "", true);
        assert!(!operation.state().cancellable);
        assert!(operation.check().unwrap_err().is_cancelled());
    }

    #[test]
    fn cancelar_desde_otro_hilo_interrumpe_la_espera() {
        let operation = Operation::silent();
        let remote = operation.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            remote.cancel();
        });
        let result = operation.wait(Duration::from_secs(30));
        handle.join().unwrap();
        assert!(result.unwrap_err().is_cancelled());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn monitored_termina_la_operacion_aunque_falle() {
        let operation = Operation::silent();
        let result: Result<()> = monitored(&operation, |op| {
            op.phase("T", "", true);
            Err(Error::Cancelled)
        });
        assert!(result.is_err());
        assert!(!operation.state().active);
    }
}
