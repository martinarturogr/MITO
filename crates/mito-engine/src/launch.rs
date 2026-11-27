//! Lanzamiento de aplicaciones, comandos y shells dentro del entorno de un profile.
//!
//! Usa `std::process::Command`. `--coupled` y `--owned` pasan por [`crate::Jobs`] y por
//! defecto no están soportados (F8, ADR 0009).

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use mito_core::Message;
use mito_core::storage::inside;

use crate::engine::Engine;
use crate::environment::Env;
use crate::error::{Error, Result, bail};
use crate::inventory::select;
use crate::isolation::{payload_path, persist_root};
use crate::record::Record;

/// `CREATE_NEW_CONSOLE`.
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Intérprete de comandos de `shell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// `%COMSPEC% /d /k`.
    Cmd,
    /// Windows PowerShell con `-NoLogo -NoProfile -NoExit`.
    Powershell,
}

/// Qué lanzar (los argumentos de `Engine.launch`, salvo el profile).
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions<'a> {
    /// Componente (`bucket/app` o solo `app` si es único entre los seleccionados).
    /// Sin él se abre la terminal del profile.
    pub app: Option<&'a str>,
    /// Alias de un `bin` en lugar de la aplicación.
    pub binary: Option<&'a str>,
    /// Argumentos del usuario, tras los fijos del manifiesto.
    pub arguments: Vec<String>,
    /// Abrir en una consola nueva.
    pub new_terminal: bool,
    /// Acoplar a una pestaña de la terminal (no soportado todavía).
    pub coupled: bool,
    /// Lanzar dentro de un Job y esperar a su árbol (no soportado todavía).
    pub owned: bool,
    /// Etiqueta del shortcut declarado.
    pub shortcut: Option<&'a str>,
}

/// Comando listo para ejecutar con su entorno y directorio de trabajo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    /// Programa.
    pub program: String,
    /// Argumentos (cada uno se entrecomilla si hace falta).
    pub args: Vec<String>,
    /// Línea de argumentos literal (para `cmd.exe /c`), tras `args`.
    pub raw_args: Option<String>,
    /// Directorio de trabajo.
    pub cwd: PathBuf,
    /// Entorno completo del hijo.
    pub env: Env,
}

impl LaunchCommand {
    /// `std::process::Command` equivalente, con el entorno exactamente el indicado.
    pub fn to_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(raw) = &self.raw_args {
            command.raw_arg(raw);
        }
        command.current_dir(&self.cwd).env_clear().envs(&self.env);
        command
    }
}

/// Línea de `cmd.exe` para un `.bat`/`.cmd` (`jobs.batch_command`).
///
/// Los datos viajan en `MITO_COMMAND_ARG_<n>` (índice 0 = script) y la línea solo
/// referencia esas variables, de modo que `cmd` no reinterpreta `&`, `|` ni `%`.
///
/// # Errors
///
/// `INVALID_ARGUMENT: …` si un valor contiene comillas, saltos de línea o NUL.
pub fn batch_command(target: &Path, arguments: &[String], env: &mut Env) -> Result<String> {
    let values: Vec<String> = std::iter::once(target.display().to_string())
        .chain(arguments.iter().cloned())
        .collect();
    if values.iter().any(|v| v.contains(['"', '\r', '\n', '\0'])) {
        return Err(Error::untranslated(
            "INVALID_ARGUMENT: BAT/CMD argv cannot contain quotes or newlines; use an explicit script",
        ));
    }
    let bare = |value: &str| {
        !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_./:\\+-".contains(c))
    };
    let mut references = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let key = format!("MITO_COMMAND_ARG_{index}");
        env.insert(key.clone(), value.clone());
        // CMD trata un valor vacío como indefinido: un argumento vacío necesita
        // comillas literales. Los tokens simples quedan sin comillas para los scripts
        // que comparan `%1` sin recortarlas.
        if value.is_empty() {
            references.push("\"\"".to_owned());
        } else if index > 0 && bare(value) {
            references.push(format!("%{key}%"));
        } else {
            references.push(format!("\"%{key}%\""));
        }
    }
    Ok(format!("/d /v:off /s /c \"{}\"", references.join(" ")))
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned())
}

/// `shutil.which('java.exe', path=PATH)`.
fn find_in_path(name: &str, path: &str) -> Option<PathBuf> {
    path.split(';')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(name))
        .find(|candidate| candidate.is_file())
}

impl Engine {
    /// Resuelve el comando de `options` sin ejecutarlo. Devuelve el comando y el
    /// registro del componente.
    ///
    /// # Errors
    ///
    /// `Modo de lanzamiento no válido`, `{app} no está seleccionado en {profile}`,
    /// `Comando no encontrado: …`, `No existe …` y los del entorno.
    pub fn launch_command(
        &self,
        profile: &str,
        options: &LaunchOptions<'_>,
    ) -> Result<Option<(LaunchCommand, Record)>> {
        self.launch_command_from(profile, options, crate::environment::process_env())
    }

    /// Como [`Self::launch_command`] con un entorno base explícito.
    ///
    /// # Errors
    ///
    /// Los de [`Self::launch_command`].
    pub fn launch_command_from(
        &self,
        profile: &str,
        options: &LaunchOptions<'_>,
        base: Env,
    ) -> Result<Option<(LaunchCommand, Record)>> {
        if options.shortcut.is_some() && options.binary.is_some_and(|b| !b.is_empty()) {
            return bail(Message::new("Modo de lanzamiento no válido"));
        }
        let data = self.profile(profile)?;
        let mut env = self.environment_from(profile, base)?;
        let workspace = self.workspace_of(profile)?;
        let Some(app) = options.app.filter(|a| !a.is_empty()) else {
            return Ok(None);
        };
        let selected = select(&self.installed()?, &data)?;
        let matches: Vec<&Record> = selected
            .iter()
            .filter(|(id, lock)| {
                *id == app || lock.key == app || lock.key.split('/').nth(1) == Some(app)
            })
            .map(|(_, lock)| lock)
            .collect();
        let [lock] = matches.as_slice() else {
            return bail(
                Message::new("{p0} no está seleccionado en {p2}")
                    .arg("p0", app)
                    .arg("p2", profile),
            );
        };
        let lock = (*lock).clone();
        let manifest = lock.effective()?;
        let (executable, fixed) = match options.binary.filter(|b| !b.is_empty()) {
            Some(binary) => {
                let bins: Vec<_> = Self::bins(&manifest)?
                    .into_iter()
                    .filter(|b| b.alias.to_lowercase() == binary.to_lowercase())
                    .collect();
                let [bin] = bins.as_slice() else {
                    return bail(Message::new("Comando no encontrado: {p1}").arg("p1", binary));
                };
                (bin.executable.clone(), bin.args.clone())
            }
            None => self.application_target(&lock, options.shortcut)?,
        };
        let payload = payload_path(self.layout(), &lock, Some(profile))?;
        let target = self.component_path(&payload, &executable)?;
        if !target.is_file() {
            return bail(Message::new("No existe {p1}").arg("p1", &executable));
        }
        let persist = persist_root(self.layout(), &lock, Some(profile))?;
        let fixed: Vec<String> = fixed
            .into_iter()
            .map(|a| {
                a.replace("$dir", &payload.display().to_string())
                    .replace("$persist_dir", &persist.display().to_string())
            })
            .collect();
        let all: Vec<String> = fixed
            .iter()
            .cloned()
            .chain(options.arguments.clone())
            .collect();
        let extension = target
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let target_text = target.display().to_string();
        let (program, args, raw_args) = match extension.as_str() {
            "bat" | "cmd" => {
                let raw = batch_command(&target, &all, &mut env)?;
                (
                    format!(r"{}\System32\cmd.exe", system_root()),
                    Vec::new(),
                    Some(raw),
                )
            }
            "ps1" => {
                let mut args: Vec<String> = ["-NoLogo", "-NoProfile", "-File", &target_text]
                    .map(str::to_owned)
                    .into();
                args.extend(all);
                ("powershell.exe".to_owned(), args, None)
            }
            "chm" => {
                let mut args = vec![target_text];
                args.extend(all);
                (format!(r"{}\hh.exe", system_root()), args, None)
            }
            "jar" => {
                let path = env.get("PATH").map_or("", String::as_str);
                let Some(java) = find_in_path("java.exe", path) else {
                    return bail(Message::new("Comando no encontrado: {p1}").arg("p1", "java.exe"));
                };
                let mut args = vec!["-jar".to_owned(), target_text];
                args.extend(all);
                (java.display().to_string(), args, None)
            }
            _ => (target_text, all, None),
        };
        // Un comando de consola (shim de `bin`) trabaja donde se invoca; las
        // aplicaciones y accesos se abren en el workspace del profile.
        let cwd = if options.binary.is_some_and(|b| !b.is_empty()) {
            std::env::current_dir().unwrap_or(workspace)
        } else {
            workspace
        };
        Ok(Some((
            LaunchCommand {
                program,
                args,
                raw_args,
                cwd,
                env,
            },
            lock,
        )))
    }

    /// Lanza una aplicación, un comando de `bin` o la terminal del profile y devuelve el
    /// código de salida (`0` si no se espera al proceso: aplicaciones y consola nueva).
    ///
    /// # Errors
    ///
    /// Los de [`Self::launch_command`], `Modo de lanzamiento no soportado todavía: …` para
    /// `--coupled` y `--owned`, y los de E/S al arrancar el proceso.
    pub fn launch(&self, profile: &str, options: &LaunchOptions<'_>) -> Result<i32> {
        let Some((command, lock)) = self.launch_command(profile, options)? else {
            return self.extensions().terminal.launch(self, profile);
        };
        let binary = options.binary.is_some_and(|b| !b.is_empty());
        if options.coupled {
            if binary || options.new_terminal || options.owned {
                return bail(Message::new("Modo de lanzamiento no válido"));
            }
            let title = self.application_name(&lock, options.shortcut)?;
            return self.extensions().jobs.coupled(
                self,
                profile,
                &lock.key,
                &title,
                &options.arguments,
                options.shortcut,
            );
        }
        if options.owned {
            return self.extensions().jobs.owned(&command);
        }
        let mut process = command.to_command();
        let spawn_error = |error| Error::io(&command.program, error);
        if options.new_terminal {
            process.creation_flags(CREATE_NEW_CONSOLE);
            process.spawn().map_err(spawn_error)?;
            return Ok(0);
        }
        if !binary {
            process.spawn().map_err(spawn_error)?;
            return Ok(0);
        }
        Ok(process.status().map_err(spawn_error)?.code().unwrap_or(1))
    }

    /// Abre una shell (o ejecuta `arguments` en su lugar) en el workspace del profile y
    /// devuelve su código de salida.
    ///
    /// # Errors
    ///
    /// Los del entorno y de E/S al arrancar.
    pub fn shell(&self, profile: &str, arguments: &[String], kind: ShellKind) -> Result<i32> {
        let (env, workspace) = self.execution_context(profile)?;
        let command: Vec<String> = if arguments.is_empty() {
            match kind {
                ShellKind::Powershell => vec![
                    format!(
                        r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
                        system_root()
                    ),
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-NoExit".into(),
                ],
                ShellKind::Cmd => vec![
                    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned()),
                    "/d".into(),
                    "/k".into(),
                ],
            }
        } else {
            arguments.to_vec()
        };
        let (program, rest) = command
            .split_first()
            .ok_or_else(|| Error::domain(Message::new("Shell no soportada")))?;
        let status = Command::new(program)
            .args(rest)
            .current_dir(&workspace)
            .env_clear()
            .envs(&env)
            .status()
            .map_err(|e| Error::io(program, e))?;
        Ok(status.code().unwrap_or(1))
    }

    /// Carpeta del workspace del profile sin crearla.
    ///
    /// # Errors
    ///
    /// Los de [`Self::profile`].
    pub fn workspace_path(&self, profile: &str) -> Result<PathBuf> {
        let data = self.profile(profile)?;
        Ok(inside(&self.layout().workspaces(), &data.workspace)?)
    }
}
