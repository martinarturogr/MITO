//! Motor de MITO: dependencias, instalación, profiles, construcción del entorno y lanzamiento.
//!
//! Contratos: `docs/spec/modules/engine.md`, `components.md`, `isolation.md`,
//! `activation.md` y `processes.md`; ADR 0008 y 0009. [`Engine`] es la fachada con los
//! mismos métodos que el `Engine` del original; cada submódulo agrupa una responsabilidad
//! ([`profiles`], [`inventory`], [`environment`], [`install`], [`components`],
//! [`isolation`], [`launch`]).
//!
//! Las piezas que llegan en fases posteriores (hooks de PowerShell, Windows Terminal,
//! accesos directos, Pocketools, Job Objects, proxy) son puntos de extensión inyectables
//! ([`Extensions`]) con un comportamiento por defecto seguro y explícito.
#![forbid(unsafe_code)]
#![allow(
    clippy::too_many_lines,
    reason = "las fases del pipeline reproducen funciones largas del original"
)]

pub mod arguments;
pub mod components;
pub mod engine;
pub mod environment;
pub mod error;
pub mod extensions;
mod fsutil;
pub mod install;
pub mod inventory;
pub mod isolation;
pub mod launch;
pub mod profiles;
mod publish;
pub mod record;
pub mod transfer;

use mito_core::Message;

pub use components::{Candidate, VersionKey, validate_update_layout, version_key};
pub use engine::Engine;
pub use environment::{AppEntry, Bin, Env};
pub use error::{Error, Result};
pub use extensions::{
    Downloader, Extensions, FaultHook, Hooks, Host, Jobs, Network, NoHooks, Pocketools, Terminal,
};
pub use install::{InstallOptions, MoveOutcome, Plan, move_whole};
pub use launch::{LaunchCommand, LaunchOptions, ShellKind};
pub use profiles::Profile;
pub use record::{Record, Records};
pub use transfer::{DownloaderFetcher, HttpDownloader};

/// Una línea del registro de una operación (`log` del original).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Mensaje sin traducir.
    pub message: Message,
    /// `Some(true)` abre un paso largo sin progreso y `Some(false)` lo cierra
    /// (`LongStep` del original); `None` es una línea normal.
    pub long_step: Option<bool>,
}

impl LogLine {
    /// Línea que abre (`active`) o cierra un paso largo.
    pub fn long_step(message: Message, active: bool) -> Self {
        Self {
            message,
            long_step: Some(active),
        }
    }
}

impl From<Message> for LogLine {
    fn from(message: Message) -> Self {
        Self {
            message,
            long_step: None,
        }
    }
}

impl From<&'static str> for LogLine {
    fn from(template: &'static str) -> Self {
        Message::new(template).into()
    }
}

/// Función de registro de mensajes (`log` del original).
pub type Log<'a> = &'a dyn Fn(LogLine);

/// Confirmación de descargas sin hash (`confirm_unverified` del original).
pub type Confirm<'a> = &'a dyn Fn(&[String]) -> bool;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_linea_normal_no_abre_ni_cierra_pasos_largos() {
        let line = LogLine::from(Message::new("Instalado {p1}").arg("p1", "a"));
        assert_eq!(line.long_step, None);
        assert_eq!(LogLine::from("x").long_step, None);
        assert_eq!(
            LogLine::long_step(Message::new("x"), true).long_step,
            Some(true)
        );
    }
}
