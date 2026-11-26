//! Errores del catálogo.

use mito_core::Message;

/// Resultado con el [`Error`] del catálogo.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Fallo de una descarga del `Fetcher`.
///
/// El original distingue el `HTTPError` 404 de cualquier otro fallo (el refresco por
/// clave prueba la raíz del repositorio si `bucket/` no existe), por eso no basta con un
/// [`mito_core::Error`] genérico.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// El servidor respondió con un código HTTP de error.
    #[error("HTTP Error {0}")]
    HttpStatus(u16),
    /// Cualquier otro fallo (red, cancelación, hash…).
    #[error(transparent)]
    Failed(#[from] mito_core::Error),
}

/// Errores del catálogo. Envuelven [`mito_core::Error`] (ADR 0008, punto 7).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Error del núcleo (`YitoError`, E/S o JSON de un archivo de MITO).
    #[error(transparent)]
    Core(#[from] mito_core::Error),
    /// Error de dominio propio del catálogo, con mensaje traducible.
    #[error("{0}")]
    Domain(Message),
    /// Fallo de una descarga.
    #[error(transparent)]
    Fetch(#[from] FetchError),
}

impl Error {
    /// Crea un error de dominio.
    pub fn domain(message: Message) -> Self {
        Self::Domain(message)
    }

    /// Mensaje traducible, si el error tiene uno.
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::Core(error) => error.message(),
            Self::Domain(message) => Some(message.clone()),
            Self::Fetch(_) => None,
        }
    }

    /// Indica si el error es una cancelación del usuario.
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::Core(error) | Self::Fetch(FetchError::Failed(error)) => error.is_cancelled(),
            _ => false,
        }
    }
}

/// Atajo para devolver un error de dominio.
///
/// # Errors
///
/// Siempre devuelve [`Error::Domain`] con `message`.
pub fn bail<T>(message: Message) -> Result<T> {
    Err(Error::Domain(message))
}
