//! Adaptador de `mito_transfer::download` a los traits [`Downloader`] y [`Fetcher`].

use std::path::Path;

use mito_catalog::{FetchError, Fetcher};
use mito_core::Operation;
use mito_transfer::DownloadOptions;

use crate::extensions::Downloader;

/// Descargas HTTP/HTTPS reales con las opciones indicadas.
#[derive(Debug, Clone, Default)]
pub struct HttpDownloader {
    /// Opciones de descarga (reintentos, proxy, límites).
    pub options: DownloadOptions,
}

impl HttpDownloader {
    /// Descargador con `options`.
    pub fn new(options: DownloadOptions) -> Self {
        Self { options }
    }
}

impl Downloader for HttpDownloader {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        operation: &Operation,
    ) -> mito_transfer::Result<()> {
        mito_transfer::download(url, destination, expected, &self.options, operation)
    }
}

/// Convierte el error de una descarga al del catálogo: un `HTTP Error` pasa a
/// [`FetchError::HttpStatus`] (el refresco por clave distingue el 404).
pub fn fetch_error(error: mito_transfer::Error) -> FetchError {
    match error {
        mito_transfer::Error::Http { status, .. } => FetchError::HttpStatus(status),
        mito_transfer::Error::Core(core) => FetchError::Failed(core),
        other => FetchError::Failed(mito_core::Error::domain(
            mito_core::Message::new("{p1}").arg("p1", other),
        )),
    }
}

impl Fetcher for HttpDownloader {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        operation: &Operation,
    ) -> Result<(), FetchError> {
        Downloader::download(self, url, destination, expected, operation).map_err(fetch_error)
    }
}

/// Adapta cualquier [`Downloader`] (p. ej. el de pruebas) al [`Fetcher`] del catálogo.
pub struct DownloaderFetcher<'a>(pub &'a dyn Downloader);

impl Fetcher for DownloaderFetcher<'_> {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        operation: &Operation,
    ) -> Result<(), FetchError> {
        self.0
            .download(url, destination, expected, operation)
            .map_err(fetch_error)
    }
}
