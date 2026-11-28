//! MITO: CLI compatible con `mito.cmd` del MITO original.
//!
//! El mismo código se compila en dos ejecutables (ADR 0013): `mito.exe`, de consola, y
//! `mitow.exe`, sin consola, que es el destino de los accesos directos, «Abrir con» y
//! los protocolos. La diferencia está solo en [`Mode`]: en `mitow.exe` los errores, que
//! en consola irían a stderr, se registran en `logs/mito.log`.
#![forbid(unsafe_code)]

mod app;
mod bootstrap;
mod cli;
mod console;
mod doctor;
mod host_run;
mod jobs;
mod network;
mod shortcuts;
mod windowed;

use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{CommandFactory as _, Parser as _};
use mito_core::Operation;

use crate::cli::Cli;
use crate::console::Console;

/// Tipo de ejecutable que arranca la CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `mito.exe`: aplicación de consola.
    Console,
    /// `mitow.exe`: sin consola; los errores van a `logs/mito.log`.
    Windowed,
}

/// Punto de entrada común de `mito.exe` y `mitow.exe`: interpreta los argumentos,
/// ejecuta el subcomando y termina el proceso con su código de salida.
pub fn main_entry(mode: Mode) -> ! {
    let cli = match mode {
        Mode::Console => Cli::parse(),
        Mode::Windowed => match Cli::try_parse() {
            Ok(cli) => cli,
            Err(error) => {
                windowed::log_error(&error.to_string());
                process::exit(error.exit_code());
            }
        },
    };
    if let Err(message) = cli.validate() {
        // `parser.error` de argparse: uso y mensaje en stderr, código 2.
        let error = Cli::command().error(clap::error::ErrorKind::ArgumentConflict, message);
        if mode == Mode::Windowed {
            windowed::log_error(&error.to_string());
            process::exit(error.exit_code());
        }
        error.exit();
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
    let console = match mode {
        Mode::Console => Console::stdio(),
        Mode::Windowed => windowed::console(),
    };
    let code = app::run(cli, &console, &operation, &interrupted);
    process::exit(code);
}
