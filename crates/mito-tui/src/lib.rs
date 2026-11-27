//! Gestor MITO en modo texto, construido con ratatui (ADR 0011).
//!
//! Contrato en `docs/spec/modules/tui.md`, `ui.md` y `ui_dialogs.md`.
#![forbid(unsafe_code)]

pub mod app;
pub mod dialogs;
pub mod input;
pub mod model;
pub mod theme;
pub mod widgets;
pub mod worker;
