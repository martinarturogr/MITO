//! Errores de `mito-transfer`.

use mito_core::Message;

/// Resultado con el [`Error`] de `mito-transfer`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errores de descarga y extracción. Envuelve [`mito_core::Error`] (el `YitoError` del
/// original, incluida la cancelación) y añade los casos propios de la red.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Error del kernel: de dominio, de E/S o cancelación.
    #[error(transparent)]
    Core(#[from] mito_core::Error),

    /// Respuesta HTTP permanente (`urllib.error.HTTPError`): un 4xx que no es 408 ni
    /// 429, o un error transitorio cuando se agotaron los reintentos.
    #[error("HTTP Error {status}: {reason} ({url})")]
    Http {
        /// Código de estado.
        status: u16,
        /// Frase estándar del estado (`Not Found`).
        reason: String,
        /// URL pedida, sin query ni fragmento.
        url: String,
    },

    /// Fallo de red tras agotar los reintentos (`URLError`/`OSError` del original).
    #[error("{reason}")]
    Network {
        /// Causa, sin la URL.
        reason: String,
    },

    /// El contenido recibido no es el artefacto declarado
    /// (`DownloadIntegrityError`). La interfaz lo explica en un diálogo.
    #[error("{message}")]
    Integrity {
        /// Texto para el usuario (en `Box` para que el error siga siendo pequeño).
        message: Box<Message>,
        /// URL pedida, sin query ni fragmento.
        url: String,
        /// Bytes recibidos.
        size: Option<u64>,
        /// Servidor final.
        host: String,
        /// `true` si es una página web interceptada (portal de autenticación) en lugar
        /// de contenido alterado.
        portal: bool,
    },

    /// El ZIP usa un método de compresión que la extracción nativa no soporta
    /// (`NotImplementedError` de `zipfile`). Se lanza antes de escribir nada, para que
    /// `archives::extract` pueda delegar en 7-Zip.
    #[error("Método de compresión del ZIP no soportado por la extracción nativa")]
    UnsupportedZipMethod,
}

impl Error {
    /// Error de dominio con un mensaje traducible.
    pub fn domain(message: Message) -> Self {
        Self::Core(mito_core::Error::domain(message))
    }

    /// Indica si el error es una cancelación del usuario.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Core(core) if core.is_cancelled())
    }

    /// Mensaje traducible, si el error lo tiene.
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::Core(core) => core.message(),
            Self::Integrity { message, .. } => Some((**message).clone()),
            Self::Http { .. } | Self::Network { .. } | Self::UnsupportedZipMethod => None,
        }
    }
}

impl From<Message> for Error {
    fn from(message: Message) -> Self {
        Self::domain(message)
    }
}
