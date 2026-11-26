//! Error de dominio compartido (equivale a `YitoError` del original).

use std::io;
use std::path::{Path, PathBuf};

use crate::message::Message;

/// Resultado con el [`Error`] de `mito-core`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errores de `mito-core`. Los demás crates lo envuelven en sus propios errores.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Error de dominio con mensaje para el usuario (`YitoError` del original).
    #[error("{0}")]
    Domain(Message),

    /// El usuario canceló la operación (`operation.Cancelled`).
    #[error("{}", cancelled_message())]
    Cancelled,

    /// Fallo de entrada/salida sobre una ruta concreta.
    #[error("{}: {source}", path.display())]
    Io {
        /// Ruta afectada.
        path: PathBuf,
        /// Error del sistema.
        #[source]
        source: io::Error,
    },

    /// JSON inválido o no serializable.
    #[error("{}: {source}", path.display())]
    Json {
        /// Archivo afectado.
        path: PathBuf,
        /// Error de serde.
        #[source]
        source: serde_json::Error,
    },
}

/// Mensaje de [`Error::Cancelled`].
pub fn cancelled_message() -> Message {
    Message::new("Instalación cancelada")
}

impl Error {
    /// Crea un error de dominio.
    pub fn domain(message: Message) -> Self {
        Self::Domain(message)
    }

    /// Envuelve un error de E/S con su ruta.
    pub fn io(path: impl AsRef<Path>, source: io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Mensaje traducible, si el error tiene uno.
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::Domain(message) => Some(message.clone()),
            Self::Cancelled => Some(cancelled_message()),
            Self::Io { .. } | Self::Json { .. } => None,
        }
    }

    /// Indica si el error es una cancelación del usuario.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Atajo para devolver un error de dominio: `bail(Message::new("…"))`.
///
/// # Errors
///
/// Siempre devuelve [`Error::Domain`] con `message`.
pub fn bail<T>(message: Message) -> Result<T> {
    Err(Error::Domain(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_error_de_dominio_muestra_su_mensaje() {
        let error = Error::domain(Message::new("Ruta no portable: {p1}").arg("p1", "../x"));
        assert_eq!(error.to_string(), "Ruta no portable: ../x");
        assert!(!error.is_cancelled());
    }

    #[test]
    fn la_cancelacion_tiene_mensaje_traducible() {
        assert_eq!(Error::Cancelled.to_string(), "Instalación cancelada");
        assert_eq!(
            Error::Cancelled.message().map(|m| m.template().to_owned()),
            Some("Instalación cancelada".to_owned())
        );
    }
}
