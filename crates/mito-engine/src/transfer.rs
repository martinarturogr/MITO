//! Adaptador de `mito_transfer::download` a los traits [`Downloader`] y [`Fetcher`].

use std::path::Path;

use mito_catalog::{FetchError, Fetcher};
use mito_core::{Layout, Operation};
use mito_transfer::DownloadOptions;
use mito_transfer::network::Proxies;

use crate::extensions::Downloader;

/// Descargas HTTP/HTTPS reales con las opciones indicadas.
#[derive(Debug, Clone, Default)]
pub struct HttpDownloader {
    /// Opciones de descarga (reintentos, proxies, límites).
    pub options: DownloadOptions,
    /// Instancia cuya `config.properties` fija los proxies en cada descarga.
    root: Option<Layout>,
    /// Proxies del sistema fijados (pruebas); `None` los lee en cada descarga.
    system: Option<Proxies>,
}

impl HttpDownloader {
    /// Descargador con `options`.
    pub fn new(options: DownloadOptions) -> Self {
        Self {
            options,
            root: None,
            system: None,
        }
    }

    /// Descargador de la instancia `layout`: en cada descarga resuelve los proxies con la
    /// configuración vigente (`_build_opener` en cada intento del original), así que un
    /// cambio en `config.properties` se aplica sin reiniciar. Un proxy inválido en
    /// `config.properties` es el error de la descarga. [`HttpDownloader::default`] sigue
    /// sin raíz (proxies del sistema), como `transfer` con `root=None`.
    pub fn for_root(layout: &Layout) -> Self {
        Self {
            options: DownloadOptions::default(),
            root: Some(layout.clone()),
            system: None,
        }
    }

    /// Igual que [`HttpDownloader::for_root`], con los proxies del sistema fijados en lugar
    /// de leídos del entorno y del registro en cada descarga (pruebas).
    pub fn for_root_with_system(layout: &Layout, system: Proxies) -> Self {
        Self {
            system: Some(system),
            ..Self::for_root(layout)
        }
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
        let Some(layout) = &self.root else {
            return mito_transfer::download(url, destination, expected, &self.options, operation);
        };
        let proxies =
            crate::network::proxies_for_root(layout, self.system.as_ref()).map_err(|error| {
                match error {
                    crate::Error::Core(core) => mito_transfer::Error::Core(core),
                    other => mito_transfer::Error::untranslated(other),
                }
            })?;
        let options = DownloadOptions {
            proxies: Some(proxies),
            ..self.options.clone()
        };
        mito_transfer::download(url, destination, expected, &options, operation)
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
