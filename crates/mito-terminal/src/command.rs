//! Comandos `wt` que abren pestañas de la instancia (`command` y `launch*` del original).

use std::path::Path;
use std::process::Command;

use mito_core::{Layout, Message};
use mito_engine::Engine;

use crate::digest::sha256_hex;
use crate::error::{Result, TerminalError};
use crate::settings::{guid, prepare};

/// Intérprete de una pestaña de profile. `mito` solo existe como valor de
/// `terminal.defaultShell`, no como shell de una pestaña.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shell {
    /// `cmd.exe`.
    #[default]
    Cmd,
    /// Windows PowerShell.
    PowerShell,
}

impl Shell {
    /// Nombre usado en los GUID y en `--type`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::PowerShell => "powershell",
        }
    }
}

/// Argumentos que ejecuta una pestaña: `core\bin\mito.exe` y `ui` (sin profile),
/// `shell --profile <p>` (cmd, sin `--type`, como el original) o
/// `shell --type powershell --profile <p>`.
pub fn child_command(layout: &Layout, profile: Option<&str>, shell: Shell) -> Vec<String> {
    let mut child = vec![layout.mito_exe().display().to_string()];
    match (profile, shell) {
        (None, _) => child.push("ui".to_owned()),
        (Some(profile), Shell::Cmd) => {
            child.extend(["shell", "--profile", profile].map(str::to_owned));
        }
        (Some(profile), Shell::PowerShell) => {
            child
                .extend(["shell", "--type", "powershell", "--profile", profile].map(str::to_owned));
        }
    }
    child
}

/// Nombre de la ventana de la instancia: `mito-` y los 12 primeros hex del SHA-256 de la
/// raíz en minúsculas. Todas las pestañas de una raíz comparten ventana.
pub fn window_name(root: &Path) -> String {
    let hex = sha256_hex(root.display().to_string().to_lowercase().as_bytes());
    format!("mito-{}", &hex[..12])
}

fn missing_terminal() -> TerminalError {
    TerminalError::Core(mito_core::Error::domain(Message::new(
        "Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.",
    )))
}

/// Ejecutable de Terminal: `YitoTerminal.exe` si existe, si no `WindowsTerminal.exe`.
fn executable(layout: &Layout) -> Result<String> {
    if !layout.windows_terminal_exe().is_file() {
        return Err(missing_terminal());
    }
    let branded = layout.branded_terminal_exe();
    let chosen = if branded.is_file() {
        branded
    } else {
        layout.windows_terminal_exe()
    };
    Ok(chosen.display().to_string())
}

/// Parte fija de `wt`: ejecutable, ventana, pestaña, perfil, título y carpeta.
fn prefix(
    layout: &Layout,
    profile: Option<&str>,
    shell: Shell,
    title: &str,
) -> Result<Vec<String>> {
    let executable = executable(layout)?;
    Ok(vec![
        executable,
        "--maximized".to_owned(),
        "-w".to_owned(),
        window_name(layout.root()),
        "new-tab".to_owned(),
        "--profile".to_owned(),
        guid(profile, shell),
        "--title".to_owned(),
        title.to_owned(),
        "--startingDirectory".to_owned(),
        layout.root().display().to_string(),
    ])
}

fn default_title(profile: Option<&str>, shell: Shell) -> String {
    match (profile, shell) {
        (None, _) => "MITO".to_owned(),
        (Some(profile), Shell::PowerShell) => format!("PowerShell ({profile})"),
        (Some(profile), Shell::Cmd) => format!("CMD ({profile})"),
    }
}

/// Línea `wt` que abre una pestaña. Sin `profile` abre el gestor MITO.
///
/// `selected` valida que el profile exista y se añade como `--profile <selected>` al
/// final del comando de la pestaña (no de `wt`).
///
/// # Errors
///
/// `Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.` y los de
/// `Engine::profile` si `selected` no existe.
pub fn command(
    engine: &Engine,
    profile: Option<&str>,
    shell: Shell,
    selected: Option<&str>,
) -> Result<Vec<String>> {
    let layout = engine.layout();
    let mut argv = prefix(layout, profile, shell, &default_title(profile, shell))?;
    argv.extend(child_command(layout, profile, shell));
    if let Some(selected) = selected {
        engine.profile(selected)?;
        argv.extend(["--profile".to_owned(), selected.to_owned()]);
    }
    Ok(argv)
}

fn spawn(root: &Path, argv: &[String]) -> Result<()> {
    let (program, arguments) = argv.split_first().ok_or_else(missing_terminal)?;
    Command::new(program)
        .args(arguments)
        .current_dir(root)
        .spawn()
        .map_err(|e| mito_core::Error::io(program, e))?;
    Ok(())
}

/// Prepara los ajustes y abre una pestaña (`launch` del original).
///
/// # Errors
///
/// Los de [`prepare`], [`command`] y los de E/S al arrancar el proceso.
pub fn launch(
    engine: &Engine,
    profile: Option<&str>,
    shell: Shell,
    selected: Option<&str>,
) -> Result<()> {
    prepare(engine, selected)?;
    let argv = command(engine, profile, shell, selected)?;
    spawn(engine.root(), &argv)
}

/// Línea `wt` de una pestaña dedicada a una aplicación: la del profile con otro título y
/// otro comando (`child`, los argumentos de `mito.exe`).
///
/// # Errors
///
/// Los de [`command`].
pub fn application_command(
    engine: &Engine,
    profile: &str,
    title: &str,
    child: &[String],
) -> Result<Vec<String>> {
    let layout = engine.layout();
    let mut argv = prefix(layout, Some(profile), Shell::Cmd, title)?;
    argv.push(layout.mito_exe().display().to_string());
    argv.extend(child.iter().cloned());
    Ok(argv)
}

/// Abre una sesión de aplicación en una pestaña propia de esta instancia.
///
/// # Errors
///
/// Los de [`prepare`] y [`application_command`].
pub fn launch_application(
    engine: &Engine,
    profile: &str,
    title: &str,
    child: &[String],
) -> Result<()> {
    prepare(engine, None)?;
    let argv = application_command(engine, profile, title, child)?;
    spawn(engine.root(), &argv)
}

/// Argumentos de `mito.exe` de un componente acoplado.
pub fn component_child(
    profile: &str,
    app: &str,
    shortcut: Option<&str>,
    arguments: &[String],
) -> Vec<String> {
    let mut child: Vec<String> = ["run", "--owned", "--profile", profile, "--app", app]
        .map(str::to_owned)
        .to_vec();
    if let Some(shortcut) = shortcut {
        child.extend(["--shortcut".to_owned(), shortcut.to_owned()]);
    }
    if !arguments.is_empty() {
        child.push("--".to_owned());
        child.extend(arguments.iter().cloned());
    }
    child
}

/// Argumentos de `mito.exe` de un ejecutable externo acoplado.
pub fn host_child(profile: &str, identity: &str, arguments: &[String]) -> Vec<String> {
    let mut child: Vec<String> = ["host-run", identity, "--profile", profile, "--owned"]
        .map(str::to_owned)
        .to_vec();
    if !arguments.is_empty() {
        child.push("--".to_owned());
        child.extend(arguments.iter().cloned());
    }
    child
}

/// Abre un componente (`run --owned`) en una pestaña con el título dado.
///
/// # Errors
///
/// Los de [`launch_application`].
pub fn launch_component(
    engine: &Engine,
    profile: &str,
    app: &str,
    title: &str,
    arguments: &[String],
    shortcut: Option<&str>,
) -> Result<()> {
    launch_application(
        engine,
        profile,
        title,
        &component_child(profile, app, shortcut, arguments),
    )
}

/// Abre un ejecutable externo (`host-run --owned`) en una pestaña con el título dado.
///
/// # Errors
///
/// Los de [`launch_application`].
pub fn launch_host(
    engine: &Engine,
    profile: &str,
    identity: &str,
    title: &str,
    arguments: &[String],
) -> Result<()> {
    launch_application(
        engine,
        profile,
        title,
        &host_child(profile, identity, arguments),
    )
}

/// Línea `wt` del servidor MCP: la ventana existente de la instancia, título `MITO MCP`
/// y `mcp-server --pipe <pipe> --owner <owner> --session <session>`.
///
/// # Errors
///
/// Los de [`command`].
pub fn mcp_command(engine: &Engine, pipe: &str, owner: i64, session: &str) -> Result<Vec<String>> {
    let layout = engine.layout();
    let mut argv = prefix(layout, None, Shell::Cmd, "MITO MCP")?;
    argv.push(layout.mito_exe().display().to_string());
    argv.extend(
        [
            "mcp-server",
            "--pipe",
            pipe,
            "--owner",
            &owner.to_string(),
            "--session",
            session,
        ]
        .map(str::to_owned),
    );
    Ok(argv)
}

/// Abre el servidor MCP en la ventana de la instancia.
///
/// # Errors
///
/// Los de [`prepare`] y [`mcp_command`].
pub fn launch_mcp(engine: &Engine, pipe: &str, owner: i64, session: &str) -> Result<()> {
    prepare(engine, None)?;
    let argv = mcp_command(engine, pipe, owner, session)?;
    spawn(engine.root(), &argv)
}
