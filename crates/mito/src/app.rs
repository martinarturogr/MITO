//! Ejecución de los subcomandos (`main` de `__main__.py`).
//!
//! La lógica vive en `mito-engine`, `mito-catalog` y `mito-core`; aquí solo se traducen
//! los argumentos, se imprime con el formato del original y se convierten los errores en
//! `MITO: <mensaje>` con su código de salida.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context as _;
use mito_core::diagnostics::{self, LogLevel};
use mito_core::error_tracking::{self, Options};
use mito_core::{Layout, MutationLock, Operation};
use mito_engine::{
    DownloaderFetcher, Engine, Extensions, InstallOptions, LaunchOptions, ShellKind,
};
use mito_terminal::{PrivateTerminal, Shell};

use crate::bootstrap;
use crate::cli::{BucketCommand, Cli, Command, ProfileCommand, ShellType};
use crate::console::Console;
use crate::doctor;

/// Código de salida de un error de dominio.
const FAILURE: i32 = 1;
/// Código de salida tras Ctrl+C.
const INTERRUPTED_EXIT: i32 = 130;
/// Pausa de `--owned`.
const OWNED_PAUSE: &str = "Pulsa Intro para cerrar esta pestaña.";

/// Arranca la CLI: deduce la raíz, prepara el log y ejecuta el subcomando. Devuelve el
/// código de salida del proceso.
pub fn run(cli: Cli, console: &Console, operation: &Operation, interrupted: &AtomicBool) -> i32 {
    let command = cli.command.unwrap_or(Command::Ui { profile: None });
    let root = match resolve_root(cli.root) {
        Ok(root) => root,
        Err(error) => {
            console.error_line(format!("MITO: {}", describe(&error)));
            return FAILURE;
        }
    };
    let engine = Engine::with_extensions(root, extensions());
    // Equivalente de `startup.py`: el log local y el registro de pánicos de hilos.
    let _ = diagnostics::init(engine.layout().clone());
    error_tracking::install_panic_hook(engine.layout().clone());
    dispatch(&engine, &command, console, operation, interrupted)
}

/// Puntos de extensión del binario: el Windows Terminal privado y el resto reales.
fn extensions() -> Extensions {
    Extensions {
        terminal: Box::new(PrivateTerminal),
        ..Extensions::default()
    }
}

/// Raíz de la instancia: la opción oculta `--root` o la deducida de
/// `<raíz>\core\bin\mito.exe`.
fn resolve_root(option: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(root) = option {
        return Ok(root);
    }
    let executable = std::env::current_exe().context("no se pudo localizar mito.exe")?;
    let layout = Layout::from_executable(&executable).with_context(|| {
        format!(
            "mito.exe debe estar en <raíz>\\core\\bin, no en {}",
            executable.display()
        )
    })?;
    Ok(layout.root().to_path_buf())
}

/// Subcomandos que pertenecen a fases posteriores del port.
fn unavailable(command: &Command) -> Option<String> {
    let name = match command {
        Command::Profile(ProfileCommand::Export { .. }) => "profile export",
        Command::Profile(ProfileCommand::Import { .. }) => "profile import",
        Command::PocketoolRun(_)
        | Command::HostRun(_)
        | Command::McpServer(_)
        | Command::MigrateStorage
        | Command::AppShortcut(_)
        | Command::Shortcut { .. }
        | Command::Release(_) => command.name(),
        _ => return None,
    };
    Some(format!("Comando no disponible todavía: {name}"))
}

/// ¿Se pidió `--owned`? (la pestaña se queda abierta hasta pulsar Intro)
fn is_owned(command: &Command) -> bool {
    match command {
        Command::Run(run) => run.owned,
        Command::HostRun(host) => host.owned,
        Command::PocketoolRun(pocketool) => pocketool.owned,
        _ => false,
    }
}

/// Ejecuta `command` sobre `engine` y devuelve el código de salida, con la presentación
/// de errores de `main` del original.
pub fn dispatch(
    engine: &Engine,
    command: &Command,
    console: &Console,
    operation: &Operation,
    interrupted: &AtomicBool,
) -> i32 {
    dispatch_with(
        engine,
        command,
        console,
        operation,
        interrupted,
        &mito_core::path_limit::long_paths_enabled,
    )
}

/// Subcomandos que no se ejecutan desde una raíz demasiado larga: abrir el gestor y
/// preparar herramientas. Las apps, shims y MCP ya instalados siguen funcionando hasta
/// mover la carpeta. La vista previa y `--approved` de `bootstrap` no lo comprueban.
fn blocks_on_long_root(command: &Command) -> bool {
    match command {
        Command::Ui { .. } | Command::Start { .. } => true,
        Command::Bootstrap(args) => !args.approved && !args.preview_welcome,
        _ => false,
    }
}

/// Como [`dispatch`], con el detector de rutas largas de Windows inyectado.
pub fn dispatch_with(
    engine: &Engine,
    command: &Command,
    console: &Console,
    operation: &Operation,
    interrupted: &AtomicBool,
    long_paths: &dyn Fn() -> bool,
) -> i32 {
    let layout = engine.layout();
    let name = command.name();
    // `logs` sale antes de escribir en el diagnóstico.
    if matches!(command, Command::Logs) {
        for folder in diagnostics::locations(layout) {
            console.line(folder.display().to_string());
        }
        return 0;
    }
    // La vista previa de la bienvenida no escribe nada: ni log ni carpetas.
    if let Command::Bootstrap(args) = command
        && args.preview_welcome
    {
        let options = bootstrap::Options {
            approved: args.approved,
            preview_welcome: true,
        };
        let downloader = engine.extensions().downloader.as_ref();
        return bootstrap::run(layout, options, console, operation, downloader);
    }
    diagnostics::write(
        layout,
        LogLevel::Info,
        &format!("cli.{name}"),
        "Command started",
        &[("runtime", diagnostics::environment())],
    );
    if blocks_on_long_root(command)
        && let Some(message) = mito_core::path_limit::problem(engine.root(), long_paths)
    {
        let text = message.to_string();
        diagnostics::write(
            layout,
            LogLevel::Error,
            &format!("cli.{name}"),
            &text,
            &[("phase", serde_json::json!("startup.path-limit"))],
        );
        console.error_line(format!("\nMITO: {text}\n"));
        console.pause("Pulsa Intro para cerrar");
        return FAILURE;
    }
    if let Some(message) = unavailable(command) {
        console.error_line(format!("MITO: {message}"));
        return FAILURE;
    }
    let owned = is_owned(command);
    if owned || matches!(command, Command::Shell { .. }) {
        // La identidad de la barra de tareas nunca impide arrancar.
        mito_terminal::identity::apply_safely(layout);
    }
    let result = engine
        .initialize()
        .map_err(anyhow::Error::from)
        .and_then(|()| execute(engine, command, console, operation));
    if interrupted.load(Ordering::SeqCst) {
        console.error_line("Operación interrumpida");
        return INTERRUPTED_EXIT;
    }
    match result {
        Ok(code) => {
            if owned {
                pause_owned_exit(console, code);
            }
            code
        }
        Err(error) => {
            capture(layout, &error, &format!("cli.{name}"));
            console.error_line(format!("MITO: {}", describe(&error)));
            if owned {
                pause_owned_exit(console, FAILURE);
            }
            FAILURE
        }
    }
}

/// `pause_owned_exit` del original.
fn pause_owned_exit(console: &Console, code: i32) {
    console.line(format!("La aplicación terminó con el código {code}."));
    console.pause(OWNED_PAUSE);
}

/// ¿Es el error de un tipo de MITO cuyo texto ya es el mensaje para el usuario?
fn is_domain(error: &(dyn StdError + 'static)) -> bool {
    error.is::<mito_engine::Error>()
        || error.is::<mito_catalog::Error>()
        || error.is::<mito_core::Error>()
        || error.is::<mito_transfer::Error>()
        || error.is::<std::io::Error>()
}

/// Texto de un error para `MITO: …`. Un error de dominio se muestra tal cual (sin
/// cadena de causas); los errores del propio binario añaden su contexto.
pub(crate) fn describe(error: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    for link in error.chain() {
        parts.push(link.to_string());
        if is_domain(link) {
            break;
        }
    }
    parts.join(": ")
}

/// Error sin tipo de MITO, para el registro local.
#[derive(Debug)]
struct Untyped(String);

impl fmt::Display for Untyped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for Untyped {}

/// Registra el error en el log local (`capture` del original).
fn capture(layout: &Layout, error: &anyhow::Error, operation: &str) {
    let options = Options::operation(operation);
    if let Some(error) = error.downcast_ref::<mito_engine::Error>() {
        error_tracking::capture(layout, error, &options);
    } else if let Some(error) = error.downcast_ref::<mito_catalog::Error>() {
        error_tracking::capture(layout, error, &options);
    } else if let Some(error) = error.downcast_ref::<mito_core::Error>() {
        error_tracking::capture(layout, error, &options);
    } else if let Some(error) = error.downcast_ref::<mito_transfer::Error>() {
        error_tracking::capture(layout, error, &options);
    } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
        error_tracking::capture(layout, error, &options);
    } else {
        error_tracking::capture(layout, &Untyped(describe(error)), &options);
    }
}

/// Quita el `--` inicial de un `REMAINDER` de `argparse`.
fn without_separator(arguments: &[String]) -> Vec<String> {
    match arguments.split_first() {
        Some((first, rest)) if first == "--" => rest.to_vec(),
        _ => arguments.to_vec(),
    }
}

/// Ejecuta el subcomando. `unavailable` ya descartó los de fases posteriores.
fn execute(
    engine: &Engine,
    command: &Command,
    console: &Console,
    operation: &Operation,
) -> anyhow::Result<i32> {
    match command {
        Command::Installed => installed(engine, console)?,
        Command::Doctor => doctor::run(engine, console)?,
        Command::Bucket(action) => bucket(engine, action, console, operation)?,
        Command::Search { query, compatible } => search(engine, query, *compatible, console)?,
        Command::Profile(action) => profile(engine, action, console)?,
        Command::Install { app, profile } => {
            install(engine, app, profile.as_deref(), console, operation)?;
        }
        Command::Activate(toggle) => {
            engine.set_active(&toggle.profile, &toggle.installation, true)?;
        }
        Command::Deactivate(toggle) => {
            engine.set_active(&toggle.profile, &toggle.installation, false)?;
        }
        Command::Shell {
            profile,
            shell_type,
            arguments,
        } => {
            let kind = match shell_type {
                ShellType::Cmd => ShellKind::Cmd,
                ShellType::Powershell => ShellKind::Powershell,
            };
            return Ok(engine.shell(profile, &without_separator(arguments), kind)?);
        }
        Command::Run(run) => {
            let options = LaunchOptions {
                app: Some(&run.app),
                binary: run.bin.as_deref(),
                arguments: without_separator(&run.arguments),
                new_terminal: false,
                coupled: run.coupled,
                owned: run.owned,
                shortcut: run.shortcut.as_deref(),
            };
            return Ok(engine.launch(&run.profile, &options)?);
        }
        Command::Start { profile } => return start(engine, profile.as_deref(), console, operation),
        Command::Bootstrap(args) => {
            let options = bootstrap::Options {
                approved: args.approved,
                preview_welcome: args.preview_welcome,
            };
            let downloader = engine.extensions().downloader.as_ref();
            return Ok(bootstrap::run(
                engine.layout(),
                options,
                console,
                operation,
                downloader,
            ));
        }
        Command::Terminal {
            profile,
            shell_type,
            new: _,
        } => {
            let shell = match shell_type {
                ShellType::Cmd => Shell::Cmd,
                ShellType::Powershell => Shell::PowerShell,
            };
            mito_terminal::launch(engine, Some(profile), shell, None)?;
        }
        Command::Ui { profile } => return ui(engine, profile.as_deref()),
        Command::Logs
        | Command::PocketoolRun(_)
        | Command::HostRun(_)
        | Command::McpServer(_)
        | Command::MigrateStorage
        | Command::AppShortcut(_)
        | Command::Shortcut { .. }
        | Command::Release(_) => {
            anyhow::bail!("Comando no disponible todavía: {}", command.name());
        }
    }
    Ok(0)
}

/// `ui` (y `mito` sin argumentos): abre la interfaz de texto.
fn ui(engine: &Engine, profile: Option<&str>) -> anyhow::Result<i32> {
    // Nunca se borra el último profile; solo el primer arranque puede no tener ninguno.
    engine.ensure_default_profile()?;
    // La interfaz comparte el motor con su hilo de trabajo, así que necesita uno propio:
    // se construye otro sobre la misma raíz y con los mismos puntos de extensión.
    let owned = Engine::with_extensions(engine.root(), extensions());
    mito_tui::run(owned, profile.map(str::to_owned))?;
    Ok(0)
}

/// `start`: abre MITO en el Windows Terminal privado. Si falta alguna herramienta, antes
/// pasa por el bootstrap (con su bienvenida); si este no termina bien, devuelve su código.
fn start(
    engine: &Engine,
    profile: Option<&str>,
    console: &Console,
    operation: &Operation,
) -> anyhow::Result<i32> {
    // Nunca se borra el último profile; solo el primer arranque puede no tener ninguno.
    engine.ensure_default_profile()?;
    if bootstrap::is_missing(engine.layout()) {
        let code = bootstrap::run(
            engine.layout(),
            bootstrap::Options::default(),
            console,
            operation,
            engine.extensions().downloader.as_ref(),
        );
        if code != 0 {
            return Ok(code);
        }
    }
    mito_terminal::launch(engine, None, Shell::Cmd, profile)?;
    Ok(0)
}

/// `installed`: `{id:65} {versión}` por componente.
fn installed(engine: &Engine, console: &Console) -> anyhow::Result<()> {
    for (id, record) in &engine.installed()? {
        console.line(format!("{id:<65} {}", record.version_text()));
    }
    Ok(())
}

/// `bucket import|refresh`, dentro del lock de mutación.
fn bucket(
    engine: &Engine,
    action: &BucketCommand,
    console: &Console,
    operation: &Operation,
) -> anyhow::Result<()> {
    let _lock = MutationLock::mutation(engine.layout(), Duration::ZERO)?;
    let catalog = engine.catalog()?;
    let fetcher = DownloaderFetcher(engine.extensions().downloader.as_ref());
    match action {
        BucketCommand::Import { name, source } => {
            let count = catalog.import_bucket(name, source, &fetcher, operation)?;
            console.line(format!("{count} manifiestos importados"));
        }
        BucketCommand::Refresh { name } => {
            for (bucket, count) in catalog.refresh_sources(name.as_deref(), &fetcher, operation)? {
                console.line(format!("{bucket}: {count} manifiestos"));
            }
        }
    }
    Ok(())
}

/// `search`: `{clave:44} {versión:20} {estado}`.
fn search(engine: &Engine, query: &str, compatible: bool, console: &Console) -> anyhow::Result<()> {
    let entries = engine.catalog()?.search_entries()?;
    for entry in mito_catalog::rank_entries(&entries, query, &BTreeSet::new())? {
        if compatible && !entry.status.is_compatible() {
            continue;
        }
        let version = entry
            .manifest
            .raw()
            .get("version")
            .map_or_else(|| "?".to_owned(), mito_catalog::pyjson::py_str);
        console.line(format!(
            "{:<44} {:<20} {}",
            entry.key,
            version,
            entry.status.text()
        ));
    }
    Ok(())
}

/// `profile create|delete|list|show`.
fn profile(engine: &Engine, action: &ProfileCommand, console: &Console) -> anyhow::Result<()> {
    match action {
        ProfileCommand::Create { name, workspace } => {
            let created = engine.create_profile(name, workspace.as_deref())?;
            console.line(format!("Profile creado: {created}"));
        }
        ProfileCommand::Delete { name } => {
            engine.delete_profile(name)?;
            console.line(format!("Profile eliminado: {name}"));
        }
        ProfileCommand::List => {
            let names = engine.profiles();
            if names.is_empty() {
                console.line("Todavía no hay profiles");
            } else {
                console.line(names.join("\n"));
            }
        }
        ProfileCommand::Show { name } => {
            let data = engine.profile(name)?;
            let text = serde_json::to_string_pretty(&data.to_value())
                .context("no se pudo serializar el profile")?;
            console.line(text);
        }
        ProfileCommand::Export { .. } | ProfileCommand::Import { .. } => {
            anyhow::bail!("Comando no disponible todavía: profile");
        }
    }
    Ok(())
}

/// `install`: pregunta por consola antes de descargar sin hash.
fn install(
    engine: &Engine,
    app: &str,
    profile: Option<&str>,
    console: &Console,
    operation: &Operation,
) -> anyhow::Result<()> {
    let confirm = |downloads: &[String]| {
        console.line("Descargas sin hash: no se podrá comprobar su integridad.");
        console.line(downloads.join("\n"));
        console.confirm("¿Descargar e instalar? [s/N] ")
    };
    let options = InstallOptions {
        profile,
        confirm_unverified: Some(&confirm),
        ..InstallOptions::default()
    };
    engine.install(
        &[app],
        &options,
        &|line| console.line(line.message.to_string()),
        operation,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
