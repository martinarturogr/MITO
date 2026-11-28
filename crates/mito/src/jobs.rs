//! Lanzamientos `--owned` y `--coupled` (`ProcessJob` y `terminal.launch_component`).
//!
//! - `--owned`: el comando arranca dentro de un Job Object (suspendido, asignado y
//!   reanudado, sin `CREATE_NO_WINDOW`: la consola de la pestaña ya existe), se espera a
//!   que salga el lanzador y después a que el Job quede vacío. El resultado es el
//!   código del lanzador, no el del árbol. Al soltar el Job mueren los procesos que
//!   queden (Ctrl+C incluido).
//! - `--coupled`: abre una pestaña nueva en la ventana de MITO que ejecuta
//!   `mito run --owned …`, y vuelve enseguida con 0.

use std::process::Command;
use std::time::Duration;

use mito_engine::{Engine, Error, Jobs, LaunchCommand, Result};
use mito_win::jobs::ProcessJob;

/// Intervalo con el que se sondea el Job (`wait_empty` del original).
const POLL: Duration = Duration::from_millis(50);

/// Implementación del punto de extensión `jobs` del motor con Job Objects reales.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessJobs;

/// Ejecuta `command` en un Job nuevo y espera a todo su árbol. Devuelve el código del
/// proceso lanzado (`1` si lo terminó una señal sin código).
///
/// # Errors
///
/// Los de E/S al crear el Job, arrancar, esperar o consultar el Job, con el programa
/// como ruta.
pub fn run_owned(command: &mut Command, program: &str) -> Result<i32> {
    let failed = |error| Error::io(program, error);
    let job = ProcessJob::new().map_err(failed)?;
    let mut child = job.spawn(command, 0).map_err(failed)?;
    let status = child.wait().map_err(failed)?;
    job.wait_empty(POLL).map_err(failed)?;
    Ok(status.code().unwrap_or(1))
}

impl Jobs for ProcessJobs {
    fn owned(&self, command: &LaunchCommand) -> Result<i32> {
        run_owned(&mut command.to_command(), &command.program)
    }

    fn coupled(
        &self,
        engine: &Engine,
        profile: &str,
        key: &str,
        title: &str,
        arguments: &[String],
        shortcut: Option<&str>,
    ) -> Result<i32> {
        mito_terminal::launch_component(engine, profile, key, title, arguments, shortcut)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use mito_engine::Env;

    use super::*;

    fn system32() -> PathBuf {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        PathBuf::from(root).join("System32")
    }

    fn cmd(arguments: &[&str]) -> LaunchCommand {
        let mut env = Env::new();
        env.insert("SystemRoot".to_owned(), r"C:\Windows".to_owned());
        env.insert("PATH".to_owned(), system32().display().to_string());
        LaunchCommand {
            program: system32().join("cmd.exe").display().to_string(),
            args: arguments.iter().map(|a| (*a).to_owned()).collect(),
            raw_args: None,
            cwd: std::env::temp_dir(),
            env,
        }
    }

    #[test]
    fn devuelve_el_codigo_del_lanzador() {
        let jobs = ProcessJobs;
        assert_eq!(jobs.owned(&cmd(&["/c", "exit", "7"])).unwrap(), 7);
        assert_eq!(jobs.owned(&cmd(&["/c", "exit", "0"])).unwrap(), 0);
    }

    #[test]
    fn espera_al_arbol_aunque_el_lanzador_ya_haya_salido() {
        // `start /b` deja un `ping` de ~2 s vivo cuando `cmd` ya terminó con 0.
        let command = cmd(&["/c", "start", "/b", "ping", "-n", "3", "127.0.0.1", ">nul"]);
        let started = Instant::now();
        assert_eq!(ProcessJobs.owned(&command).unwrap(), 0);
        assert!(
            started.elapsed() >= Duration::from_millis(1_000),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn un_ejecutable_inexistente_es_un_error_de_entrada_salida() {
        let mut command = cmd(&[]);
        command.program = r"C:\no\existe\mito-nada.exe".to_owned();
        let error = ProcessJobs.owned(&command).unwrap_err();
        assert!(error.to_string().contains("mito-nada.exe"), "{error}");
    }
}
