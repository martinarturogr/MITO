//! `host-run`: abre un ejecutable externo registrado con un profile de MITO (F8).
//!
//! Sin `--owned` devuelve `0` aunque el proceso hijo siga vivo; con `--coupled` abre una
//! pestaña de la terminal y devuelve `0`; con `--owned` espera a todo el árbol y devuelve
//! el código del lanzador. La pausa de `--owned` la aplica el despachador.

use mito_desktop::host_apps;
use mito_engine::Engine;

use crate::cli::HostRun;

/// Ejecuta `host-run` y devuelve el código de salida del comando.
pub fn run(engine: &Engine, args: &HostRun) -> anyhow::Result<i32> {
    let code = host_apps::launch(
        engine,
        &args.profile,
        &args.identity,
        &args.arguments,
        args.coupled,
        args.owned,
    )?;
    Ok(if args.owned { code } else { 0 })
}
