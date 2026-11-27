//! Puntos de extensión del motor (ADR 0009).
//!
//! El original llama directamente a módulos que se portan en fases posteriores. Aquí
//! cada uno es un trait (o callback) que [`Engine`] recibe en [`Extensions`], con un
//! comportamiento por defecto seguro y explícito: lo que falta nunca se omite en
//! silencio.
//!
//! | Punto | Fase | Por defecto |
//! |---|---|---|
//! | [`Hooks`] (`pre_install`, `installer`) | F7 | «Pasos no soportados: &lt;campo&gt;» antes de descargar |
//! | [`Terminal::prepare`] | F5 | no-op |
//! | [`Host::migrate_application_links`] | F8 | no-op |
//! | [`Pocketools`] | F10 | lista vacía |
//! | [`Jobs`] (`--coupled`, `--owned`) | F8 | «no soportado todavía» |
//! | [`Extensions::sevenzip`] | F5 | `core/tools/7zip/7z.exe` si existe |
//! | [`Network`] (proxy) | F9 | no-op |

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use mito_core::{Layout, Message, Operation};
use serde_json::{Map, Value};

use crate::engine::Engine;
use crate::environment::Env;
use crate::error::{Error, Result, bail};
use crate::launch::LaunchCommand;
use crate::record::Record;

/// Descarga de un archivo. La implementación por defecto es [`crate::HttpDownloader`];
/// los tests inyectan una que escribe archivos de prueba sin red.
pub trait Downloader: Send + Sync {
    /// Descarga `url` en `destination`, verificando `expected` si se indica.
    ///
    /// # Errors
    ///
    /// Los de [`mito_transfer::download()`].
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        operation: &Operation,
    ) -> mito_transfer::Result<()>;
}

/// Ejecutor de los hooks de instalación (`hooks.py`, F7).
pub trait Hooks: Send + Sync {
    /// Campo del manifiesto efectivo que este ejecutor no soporta, si lo hay. Se
    /// comprueba al planificar, antes de descargar nada.
    fn unsupported(&self, manifest: &Map<String, Value>) -> Option<String>;

    /// Ejecuta `pre_install` sobre el payload en staging.
    ///
    /// # Errors
    ///
    /// Los del hook.
    fn pre_install(&self, key: &str, manifest: &Map<String, Value>, payload: &Path) -> Result<()>;

    /// Ejecuta `installer` (script o archivo) sobre el payload en staging.
    ///
    /// # Errors
    ///
    /// Los del instalador.
    fn installer(&self, key: &str, manifest: &Map<String, Value>, payload: &Path) -> Result<()>;
}

/// Hooks por defecto: ningún paso se ejecuta y un manifiesto que lo requiere se rechaza.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl Hooks for NoHooks {
    fn unsupported(&self, manifest: &Map<String, Value>) -> Option<String> {
        if manifest
            .get("pre_install")
            .is_some_and(mito_catalog::pyjson::py_truthy)
        {
            return Some("pre_install".to_owned());
        }
        let installer = mito_catalog::installer_definition(manifest)
            .ok()
            .flatten()?;
        Some(if installer.contains_key("file") {
            "installer.file".to_owned()
        } else {
            "installer.script".to_owned()
        })
    }

    fn pre_install(&self, _: &str, _: &Map<String, Value>, _: &Path) -> Result<()> {
        Ok(())
    }

    fn installer(&self, _: &str, _: &Map<String, Value>, _: &Path) -> Result<()> {
        Ok(())
    }
}

/// Integración con Windows Terminal (`terminal.py`, F5).
pub trait Terminal: Send + Sync {
    /// Regenera los perfiles de Windows Terminal tras cambiar profiles o activaciones.
    ///
    /// # Errors
    ///
    /// Los de la implementación.
    fn prepare(&self, _engine: &Engine) -> Result<()> {
        Ok(())
    }

    /// Abre la terminal del profile (lanzamiento sin `app`).
    ///
    /// # Errors
    ///
    /// Por defecto, «no soportado todavía».
    fn launch(&self, _engine: &Engine, _profile: &str) -> Result<i32> {
        bail(Message::new("Lanzamiento no soportado todavía: ").with_suffix("terminal del profile"))
    }
}

/// Terminal por defecto: `prepare` no hace nada y `launch` no está soportado.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTerminal;

impl Terminal for NoTerminal {}

/// Integración con el host (accesos directos, «Abrir con», exposición; F8).
pub trait Host: Send + Sync {
    /// Reescribe los accesos de una aplicación publicada (`migrate_application_links`).
    ///
    /// `links` acumula el contenido previo de cada acceso tocado (o `None` si no
    /// existía) para que el motor pueda deshacer el cambio si la publicación falla.
    ///
    /// # Errors
    ///
    /// Los de la implementación; un error revierte toda la publicación.
    fn migrate_application_links(
        &self,
        _engine: &Engine,
        _key: &str,
        _record: &Record,
        _links: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
        _log: crate::Log<'_>,
    ) -> Result<()> {
        Ok(())
    }

    /// Quita las entradas de «Abrir con» de un profile borrado.
    ///
    /// # Errors
    ///
    /// Los de la implementación.
    fn unregister_profile(&self, _engine: &Engine, _profile: &str) -> Result<()> {
        Ok(())
    }

    /// Quita las entradas de «Abrir con» y la exposición al host de un componente
    /// eliminado. Los fallos no revierten la eliminación: se registran.
    ///
    /// # Errors
    ///
    /// Los de la implementación.
    fn unregister_installation(
        &self,
        _engine: &Engine,
        _installation: &str,
        _log: crate::Log<'_>,
    ) -> Result<()> {
        Ok(())
    }
}

/// Host por defecto: todo es no-op.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHost;

impl Host for NoHost {}

/// Directorios de Pocketools que se añaden al `PATH` (`pocketools.paths`, F10).
pub trait Pocketools: Send + Sync {
    /// Carpetas a añadir tras `core/commands`.
    fn paths(&self, layout: &Layout) -> Vec<PathBuf>;
}

/// Sin Pocketools: lista vacía.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPocketools;

impl Pocketools for NoPocketools {
    fn paths(&self, _layout: &Layout) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// Red del entorno de ejecución (`network.proxy_environment`, F9).
pub trait Network: Send + Sync {
    /// Añade al entorno heredado las variables de proxy de `config.properties`.
    ///
    /// # Errors
    ///
    /// Los de la implementación.
    fn proxy_environment(&self, layout: &Layout, env: &mut Env) -> Result<()>;
}

/// Sin proxy: el entorno heredado no se toca.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoNetwork;

impl Network for NoNetwork {
    fn proxy_environment(&self, _layout: &Layout, _env: &mut Env) -> Result<()> {
        Ok(())
    }
}

/// Lanzamientos con Job Object (`processes.ProcessJob`, F8).
pub trait Jobs: Send + Sync {
    /// Lanza el comando dentro de un Job y espera a que su árbol termine (`--owned`).
    ///
    /// # Errors
    ///
    /// Por defecto, «no soportado todavía».
    fn owned(&self, _command: &LaunchCommand) -> Result<i32> {
        bail(Message::new("Modo de lanzamiento no soportado todavía: ").with_suffix("--owned"))
    }

    /// Lanza la aplicación acoplada a una pestaña de la terminal (`--coupled`).
    ///
    /// # Errors
    ///
    /// Por defecto, «no soportado todavía».
    fn coupled(
        &self,
        _engine: &Engine,
        _profile: &str,
        _key: &str,
        _title: &str,
        _arguments: &[String],
        _shortcut: Option<&str>,
    ) -> Result<i32> {
        bail(Message::new("Modo de lanzamiento no soportado todavía: ").with_suffix("--coupled"))
    }
}

/// Sin Jobs: `--owned` y `--coupled` no están soportados.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoJobs;

impl Jobs for NoJobs {}

/// Punto de fallo para pruebas: recibe el nombre del punto (`save-profile`,
/// `prepare-payload`, `refresh-launchers`) y su sujeto (el profile) y puede devolver un
/// error de E/S para simular el fallo.
pub type FaultHook = Box<dyn Fn(&str, &str) -> io::Result<()> + Send + Sync>;

/// Todos los puntos de extensión de un [`Engine`].
pub struct Extensions {
    /// Descarga de archivos.
    pub downloader: Box<dyn Downloader>,
    /// Hooks `pre_install` e `installer` (F7).
    pub hooks: Box<dyn Hooks>,
    /// Windows Terminal (F5).
    pub terminal: Box<dyn Terminal>,
    /// Accesos directos y «Abrir con» (F8).
    pub host: Box<dyn Host>,
    /// Pocketools en el `PATH` (F10).
    pub pocketools: Box<dyn Pocketools>,
    /// Proxy del entorno (F9).
    pub network: Box<dyn Network>,
    /// Job Objects (F8).
    pub jobs: Box<dyn Jobs>,
    /// Ruta de `7z.exe`; `None` usa `core/tools/7zip/7z.exe` si existe (F5).
    pub sevenzip: Option<PathBuf>,
    /// Inyección de fallos para pruebas.
    #[doc(hidden)]
    pub fault: Option<FaultHook>,
}

impl Default for Extensions {
    fn default() -> Self {
        Self {
            downloader: Box::new(crate::transfer::HttpDownloader::default()),
            hooks: Box::new(NoHooks),
            terminal: Box::new(NoTerminal),
            host: Box::new(NoHost),
            pocketools: Box::new(NoPocketools),
            network: Box::new(NoNetwork),
            jobs: Box::new(NoJobs),
            sevenzip: None,
            fault: None,
        }
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extensions").finish_non_exhaustive()
    }
}

impl Extensions {
    /// Ejecuta el punto de fallo `point` con `subject`, si hay un hook de pruebas.
    pub(crate) fn fault(&self, point: &str, subject: &str) -> Result<()> {
        match &self.fault {
            Some(hook) => hook(point, subject).map_err(|e| Error::io(subject, e)),
            None => Ok(()),
        }
    }
}
