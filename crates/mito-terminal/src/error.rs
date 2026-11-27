//! Errores de la integración con Windows Terminal.

/// Resultado con el [`TerminalError`].
pub type Result<T, E = TerminalError> = std::result::Result<T, E>;

/// Errores de `mito-terminal`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TerminalError {
    /// Error del núcleo: de dominio (`Falta Windows Terminal portable…`), de E/S o JSON.
    #[error(transparent)]
    Core(#[from] mito_core::Error),
    /// Error del motor (p. ej. un profile inexistente).
    #[error(transparent)]
    Engine(#[from] mito_engine::Error),
    /// Error de E/S del sistema sin ruta asociada.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<TerminalError> for mito_engine::Error {
    fn from(error: TerminalError) -> Self {
        match error {
            TerminalError::Core(error) => Self::Core(error),
            TerminalError::Engine(error) => error,
            TerminalError::Io(error) => Self::Core(mito_core::Error::io("", error)),
        }
    }
}
