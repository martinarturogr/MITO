//! Punto de extensión `terminal` del motor (ADR 0009).

use mito_engine::{Engine, Terminal};

use crate::command::{Shell, launch};
use crate::settings::prepare;

/// Windows Terminal privado de la instancia: regenera sus ajustes tras crear profiles,
/// activar o instalar, y abre la pestaña de un profile.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrivateTerminal;

impl Terminal for PrivateTerminal {
    fn prepare(&self, engine: &Engine) -> mito_engine::Result<()> {
        Ok(prepare(engine, None)?)
    }

    fn launch(&self, engine: &Engine, profile: &str) -> mito_engine::Result<i32> {
        launch(engine, Some(profile), Shell::Cmd, None)?;
        Ok(0)
    }
}
