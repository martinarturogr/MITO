//! `app-shortcut` y `shortcut`: accesos de escritorio de una aplicación y de MITO.
//!
//! Ambos escriben en `core/state/` dentro del lock de mutación y muestran la ruta del
//! `.lnk` creado (una sola línea en stdout).

use std::time::Duration;

use mito_core::MutationLock;
use mito_desktop::shortcuts::{LinkOptions, application_link, desktop_path, mito_link};
use mito_engine::Engine;

use crate::cli::AppShortcut;
use crate::console::Console;

/// `app-shortcut <instalación> --profile …`.
pub fn app_shortcut(engine: &Engine, args: &AppShortcut, console: &Console) -> anyhow::Result<()> {
    let _lock = MutationLock::mutation(engine.layout(), Duration::ZERO)?;
    let options = LinkOptions {
        badge: args.badge.as_deref(),
        color: args.color.as_deref(),
        coupled: args.coupled,
        arguments: &args.arguments,
        shortcut: args.shortcut.as_deref(),
    };
    let path = application_link(
        engine,
        &desktop_path,
        &args.profile,
        &args.installation,
        &options,
    )?;
    console.line(path.display().to_string());
    Ok(())
}

/// `shortcut --profile …`: acceso a la terminal de MITO.
pub fn mito_shortcut(
    engine: &Engine,
    profile: &str,
    badge: Option<&str>,
    color: Option<&str>,
    console: &Console,
) -> anyhow::Result<()> {
    let _lock = MutationLock::mutation(engine.layout(), Duration::ZERO)?;
    let path = mito_link(engine, &desktop_path, profile, badge, color)?;
    console.line(path.display().to_string());
    Ok(())
}
