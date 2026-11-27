//! Errores del motor.

use std::error::Error as StdError;

use mito_core::Message;
use mito_core::diagnostics::StepError;

/// Resultado con el [`Error`] del motor.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errores del motor. Envuelve [`mito_core::Error`] (el `YitoError` del original, ADR
/// 0008 punto 7) y los de los crates de las capas inferiores.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Error del núcleo: de dominio, de E/S, de JSON o cancelación.
    #[error(transparent)]
    Core(#[from] mito_core::Error),
    /// Error del catálogo.
    #[error(transparent)]
    Catalog(#[from] mito_catalog::Error),
    /// Error de descarga o extracción.
    #[error(transparent)]
    Transfer(#[from] mito_transfer::Error),
    /// Error de una fase registrada con [`mito_core::diagnostics::step`]. Se conserva
    /// como nodo de la cadena `source()` para que el seguimiento de errores conozca la
    /// fase y el componente.
    #[error("{0}")]
    Step(#[source] StepError),
}

impl Error {
    /// Error de dominio con un mensaje traducible.
    pub fn domain(message: Message) -> Self {
        Self::Core(mito_core::Error::domain(message))
    }

    /// Envuelve un error de E/S con su ruta.
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        Self::Core(mito_core::Error::io(path, source))
    }

    /// Error de dominio con un texto que no se traduce (`STATE_CONFLICT: …`,
    /// `{key}: {status}`).
    pub fn untranslated(text: impl std::fmt::Display) -> Self {
        Self::domain(Message::new("{p1}").arg("p1", text))
    }

    /// Error interno anidado dentro de una fase, si lo hay.
    fn inner(&self) -> Option<&Self> {
        match self {
            Self::Step(step) => step.source()?.downcast_ref::<Self>(),
            _ => None,
        }
    }

    /// Mensaje traducible, si el error lo tiene.
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::Core(error) => error.message(),
            Self::Catalog(error) => error.message(),
            Self::Transfer(error) => error.message(),
            Self::Step(_) => self.inner().and_then(Self::message),
        }
    }

    /// Indica si el error es una cancelación del usuario.
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::Core(error) => error.is_cancelled(),
            Self::Catalog(error) => error.is_cancelled(),
            Self::Transfer(error) => error.is_cancelled(),
            Self::Step(_) => self.inner().is_some_and(Self::is_cancelled),
        }
    }
}

impl From<Message> for Error {
    fn from(message: Message) -> Self {
        Self::domain(message)
    }
}

impl From<mito_catalog::FetchError> for Error {
    fn from(error: mito_catalog::FetchError) -> Self {
        match error {
            mito_catalog::FetchError::Failed(core) => Self::Core(core),
            other => Self::Catalog(mito_catalog::Error::Fetch(other)),
        }
    }
}

/// Atajo para devolver un error de dominio.
///
/// # Errors
///
/// Siempre devuelve [`Error::Core`] de dominio con `message`.
pub fn bail<T>(message: Message) -> Result<T> {
    Err(Error::domain(message))
}

/// Ejecuta `body` como una fase de diagnóstico (`diagnostics.step` del original) y
/// conserva el error del motor dentro de [`Error::Step`].
pub(crate) fn step<T>(
    layout: &mito_core::Layout,
    operation: &str,
    phase: &str,
    component: Option<&str>,
    body: impl FnOnce() -> Result<T>,
) -> Result<T> {
    mito_core::diagnostics::step(layout, operation, phase, component, body).map_err(Error::Step)
}

/// Convierte un error de E/S de `path` en un error del motor.
pub(crate) fn io_error(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::io(path, source)
}
