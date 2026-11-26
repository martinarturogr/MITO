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

/// Función de registro de mensajes (`log` del original).
pub type Log<'a> = &'a dyn Fn(&str);

/// Confirmación de descargas sin hash (`confirm_unverified` del original).
pub type Confirm<'a> = &'a dyn Fn(&[String]) -> bool;
