//! Punto de entrada de `mito.exe`: CLI compatible con `mito.cmd` del MITO original.
#![forbid(unsafe_code)]

mod app;
mod bootstrap;
mod cli;
mod console;
mod doctor;
mod host_run;

use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{CommandFactory as _, Parser as _};
use mito_core::Operation;

use crate::cli::Cli;
use crate::console::Console;

fn main() {
    let cli = Cli::parse();
    if let Err(message) = cli.validate() {
        // `parser.error` de argparse: uso y mensaje en stderr, código 2.
        Cli::command()
            .error(clap::error::ErrorKind::ArgumentConflict, message)
            .exit();
    }
    // Ctrl+C cancela la operación en curso y el proceso sale con 130 (`KeyboardInterrupt`).
    let interrupted = Arc::new(AtomicBool::new(false));
    let operation = Operation::silent();
    {
        let (flag, operation) = (Arc::clone(&interrupted), operation.clone());
        // Si ya hay un manejador instalado no se puede cambiar; el proceso sigue sin él.
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
            operation.cancel();
        });
    }
    let code = app::run(cli, &Console::stdio(), &operation, &interrupted);
    process::exit(code);
}
