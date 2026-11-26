//! Exclusión de mutaciones entre procesos (`storage.mutation` del original).
//!
//! El lock es un archivo `core/state/<name>` que se crea si falta y nunca se borra.
//! El sistema operativo lo libera si el proceso termina, también de forma abrupta.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::time::{Duration, Instant};

use crate::error::{Error, Result, bail};
use crate::layout::Layout;
use crate::message::Message;
use crate::storage::identifier;

/// Nombre del lock global de mutaciones.
pub const MUTATION_LOCK: &str = "mutation.lock";

/// Intervalo entre reintentos mientras se espera un lock ocupado.
const RETRY: Duration = Duration::from_millis(100);

/// Lock exclusivo adquirido; se libera al soltarlo.
#[derive(Debug)]
pub struct MutationLock {
    file: File,
}

impl MutationLock {
    /// Adquiere `core/state/<name>` esperando como máximo `wait`. Con `wait` cero falla
    /// en el acto si está ocupado.
    ///
    /// # Errors
    ///
    /// `Otra instancia está modificando MITO. Espera a que termine.` si sigue ocupado
    /// tras `wait`; los errores de [`identifier`] para `name`, y [`Error::Io`].
    pub fn acquire(layout: &Layout, name: &str, wait: Duration) -> Result<Self> {
        let name = identifier(name)?;
        let folder = layout.state();
        fs::create_dir_all(&folder).map_err(|e| Error::io(&folder, e))?;
        let path = folder.join(name);
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| Error::io(&path, e))?;
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(RETRY);
                }
                Err(TryLockError::WouldBlock) => {
                    return bail(Message::new(
                        "Otra instancia está modificando MITO. Espera a que termine.",
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(Error::io(&path, e)),
            }
        }
    }

    /// Adquiere el lock global [`MUTATION_LOCK`].
    ///
    /// # Errors
    ///
    /// Los de [`Self::acquire`].
    pub fn mutation(layout: &Layout, wait: Duration) -> Result<Self> {
        Self::acquire(layout, MUTATION_LOCK, wait)
    }
}

impl Drop for MutationLock {
    fn drop(&mut self) {
        // El cierre del handle también lo libera; el unlock explícito es inmediato.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_lock_es_exclusivo_y_se_libera_al_soltarlo() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let first = MutationLock::mutation(&layout, Duration::ZERO).unwrap();
        let busy = MutationLock::mutation(&layout, Duration::from_millis(250)).unwrap_err();
        assert_eq!(
            busy.to_string(),
            "Otra instancia está modificando MITO. Espera a que termine."
        );
        drop(first);
        MutationLock::mutation(&layout, Duration::ZERO).unwrap();
        assert!(layout.state().join(MUTATION_LOCK).exists());
    }

    #[test]
    fn espera_a_que_otro_hilo_libere_el_lock() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let first = MutationLock::mutation(&layout, Duration::ZERO).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(first);
        });
        MutationLock::mutation(&layout, Duration::from_secs(10)).unwrap();
        release.join().unwrap();
    }

    #[test]
    fn el_nombre_del_lock_se_valida() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        assert!(MutationLock::acquire(&layout, "../x", Duration::ZERO).is_err());
    }
}
