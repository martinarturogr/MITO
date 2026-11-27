//! Integración con el Windows Terminal privado: ajustes, pestañas e identidad de ventana.
//!
//! Contratos: `docs/spec/modules/terminal.md`, `terminal_branding.md` y
//! `terminal_identity.md`; ADR 0009 y 0010.
//!
//! - [`settings`]: el `settings.json` del Terminal privado (perfiles MITO, PowerShell y
//!   CMD por profile, paletas, menú) y su escritura idempotente.
//! - [`mod@command`]: la línea `wt` que abre una pestaña de la instancia y su lanzamiento.
//! - [`branding`]: `YitoTerminal.exe`, la copia con el icono de MITO.
//! - [`identity`]: la identidad de la barra de tareas de las ventanas de la instancia.
//! - [`PrivateTerminal`]: la implementación del punto de extensión `terminal` del motor.
#![forbid(unsafe_code)]

pub mod branding;
pub mod command;
mod commandline;
mod digest;
pub mod error;
pub mod identity;
mod integration;
pub mod settings;

pub use branding::{brand_window_icons, ensure_branded_executable};
pub use command::{
    Shell, child_command, command, launch, launch_application, launch_component, launch_host,
    launch_mcp, window_name,
};
pub use commandline::list2cmdline;
pub use error::{Result, TerminalError};
pub use integration::PrivateTerminal;
pub use settings::{Mode, guid, prepare, scheme};
