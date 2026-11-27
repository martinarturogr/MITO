//! Primer arranque: prepara las herramientas privadas desde `mito.exe` (ADR 0010).
//!
//! Sustituye a `bootstrap.ps1`, `bootstrap.py` y `bootstrap_tools.py` del original: no
//! hay Python que preparar. Muestra la bienvenida en consola, pide una respuesta
//! afirmativa antes de crear `core/tools` o descargar, instala 7-Zip, innounp y Windows
//! Terminal con verificación de SHA-256 y deja los marcadores.

mod install;
pub mod lock;

use std::time::Duration;

use mito_core::diagnostics::{self, LogLevel};
use mito_core::storage::write_json;
use mito_core::{Layout, MutationLock, Operation};
use mito_engine::Downloader;
use serde_json::json;

use crate::console::{Color, Console};
use lock::{Lock, Tool};

/// Código de salida de un error.
pub const FAILURE: i32 = 1;
/// Código de salida tras cancelar la bienvenida.
pub const CANCELLED: i32 = 2;
/// Espera máxima por el lock de mutación.
const LOCK_WAIT: Duration = Duration::from_secs(30);
/// Operación de los registros del bootstrap.
const OPERATION: &str = "bootstrap.mito";

/// Opciones de `mito bootstrap` (los parámetros del lanzador original).
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Salta la bienvenida (`-Approved`); lo usa el actualizador.
    pub approved: bool,
    /// Muestra la bienvenida sin descargar ni preparar nada (`-PreviewWelcome`).
    pub preview_welcome: bool,
}

/// Registra un evento del bootstrap con su fase.
fn log(layout: &Layout, level: LogLevel, phase: &str, message: &str) -> Option<std::path::PathBuf> {
    diagnostics::write(
        layout,
        level,
        OPERATION,
        message,
        &[("phase", json!(phase))],
    )
}

/// ¿Es una respuesta afirmativa? Solo `s`, `si` y `sí`, sin distinguir mayúsculas ni
/// espacios alrededor; todo lo demás, incluida la cadena vacía, es «no».
pub fn accepts(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "s" | "si" | "sí")
}

/// Texto de la bienvenida: `(línea, color)` para las herramientas indicadas.
pub fn welcome_lines(tools: &[&Tool]) -> Vec<(String, Option<Color>)> {
    let mut lines = vec![
        (String::new(), None),
        (
            "Bienvenido a MITO (My Integrated Terminal Orchestrator)".to_owned(),
            Some(Color::Cyan),
        ),
        (String::new(), None),
        (
            "Para preparar MITO se descargarán las siguientes herramientas:".to_owned(),
            None,
        ),
    ];
    lines.extend(
        tools
            .iter()
            .map(|tool| (format!("  - {} {}", tool.label(), tool.version), None)),
    );
    lines.extend([
        (String::new(), None),
        (
            "Es necesaria una conexión a Internet.".to_owned(),
            Some(Color::Yellow),
        ),
        (
            "Las herramientas se guardarán dentro de esta carpeta de MITO.".to_owned(),
            None,
        ),
        (String::new(), None),
    ]);
    lines
}

/// Muestra la bienvenida y pregunta. Sin respuesta (EOF o consola no interactiva) es «no».
pub fn show_welcome(console: &Console, tools: &[&Tool]) -> bool {
    for (line, color) in welcome_lines(tools) {
        match color {
            Some(color) => console.colored_line(line, color),
            None => console.line(line),
        }
    }
    console
        .read_line("¿Deseas continuar? [s/N] ")
        .is_some_and(|answer| accepts(&answer))
}

/// Herramientas del lock que no están listas.
pub fn pending<'a>(layout: &Layout, lock: &'a Lock) -> Vec<&'a Tool> {
    lock.tools
        .iter()
        .filter(|tool| !tool.is_ready(layout))
        .collect()
}

/// ¿Falta alguna herramienta? Un lock ilegible cuenta como «no» (lo informará `bootstrap`).
pub fn is_missing(layout: &Layout) -> bool {
    Lock::load(layout).is_ok_and(|lock| !pending(layout, &lock).is_empty())
}

/// Registra el fallo y lo muestra como `MITO: <mensaje>` seguido de la ruta del log.
fn fail(layout: &Layout, console: &Console, phase: &str, error: &anyhow::Error) -> i32 {
    let text = crate::app::describe(error);
    let path = log(layout, LogLevel::Error, phase, &text);
    console.error_line(format!("MITO: {text}"));
    if let Some(path) = path {
        console.error_line(format!("MITO: log: {}", path.display()));
    }
    FAILURE
}

/// Vista previa de la bienvenida: nada se descarga ni se crea.
fn preview(layout: &Layout, console: &Console) -> i32 {
    let lock = match Lock::load(layout) {
        Ok(lock) => lock,
        Err(error) => {
            console.error_line(format!("MITO: {}", crate::app::describe(&error)));
            return FAILURE;
        }
    };
    let tools: Vec<&Tool> = lock.tools.iter().collect();
    // La respuesta se descarta.
    let _ = show_welcome(console, &tools);
    console.line("Vista previa terminada. No se ha descargado ni preparado ninguna herramienta.");
    0
}

/// Prepara las herramientas que falten y devuelve el código de salida: `0` listo, `1`
/// error y `2` cancelado en la bienvenida.
///
/// Con todo listo no descarga nada, pero repasa la marca de Windows Terminal y reescribe
/// `core/state/bootstrap-tools.json`, como `mito bootstrap` del original.
pub fn run(
    layout: &Layout,
    options: Options,
    console: &Console,
    operation: &Operation,
    downloader: &dyn Downloader,
) -> i32 {
    if options.preview_welcome {
        return preview(layout, console);
    }
    log(layout, LogLevel::Info, "startup", "Bootstrap started");
    let lock = match Lock::load(layout) {
        Ok(lock) => lock,
        Err(error) => return fail(layout, console, "read-bootstrap-lock", &error),
    };
    let missing = pending(layout, &lock);
    if !missing.is_empty() {
        if !options.approved && !show_welcome(console, &missing) {
            log(
                layout,
                LogLevel::Info,
                "confirmation",
                "Bootstrap cancelled before downloading",
            );
            console
                .line("Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.");
            return CANCELLED;
        }
        log(layout, LogLevel::Info, "confirmation", "Bootstrap approved");
    }
    match prepare(layout, &lock, console, operation, downloader) {
        Ok(()) => {
            log(layout, LogLevel::Info, "complete", "Bootstrap completed");
            0
        }
        Err((phase, error)) => fail(layout, console, phase, &error),
    }
}

/// Instala lo que falte bajo el lock de mutación y finaliza.
fn prepare(
    layout: &Layout,
    lock: &Lock,
    console: &Console,
    operation: &Operation,
    downloader: &dyn Downloader,
) -> Result<(), (&'static str, anyhow::Error)> {
    let tools = layout.tools();
    std::fs::create_dir_all(&tools).map_err(|e| {
        (
            "create-tools-directory",
            mito_core::Error::io(&tools, e).into(),
        )
    })?;
    let _guard =
        MutationLock::mutation(layout, LOCK_WAIT).map_err(|e| ("bootstrap-lock", e.into()))?;
    // Otra instancia pudo terminar mientras esta esperaba el lock.
    let missing = pending(layout, lock);
    if missing.is_empty() {
        log(layout, LogLevel::Info, "ready", "Tools already prepared");
    }
    for tool in missing {
        console.line(format!("Preparando {} {}…", tool.id, tool.version));
        install::install_tool(layout, tool, downloader, operation)
            .map_err(|e| ("install-tool", e))?;
    }
    // Una versión recién descargada se marca antes de abrir la primera ventana. Un fallo
    // no impide el arranque: se usará WindowsTerminal.exe (ADR 0010).
    mito_terminal::branding::brand_or_warn(layout);
    write_json(
        &layout.state().join("bootstrap-tools.json"),
        &json!({"lock_sha256": lock.sha256}),
    )
    .map_err(|e| ("write-state", e.into()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
