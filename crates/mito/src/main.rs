//! Punto de entrada de `mito.exe`: CLI compatible con `mito.cmd` del MITO original.
#![forbid(unsafe_code)]

use clap::Parser;

/// My Integrated Terminal Orchestrator.
///
/// Entorno de desarrollo de Windows completo y portátil dentro de una carpeta.
#[derive(Debug, Parser)]
#[command(name = "mito", version, about, long_about = None, arg_required_else_help = true)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn la_definicion_de_la_cli_es_valida() {
        Cli::command().debug_assert();
    }
}
