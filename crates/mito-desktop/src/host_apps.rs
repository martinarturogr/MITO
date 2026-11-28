//! Aplicaciones del host lanzadas con el entorno del profile (`host_apps.py`).
//!
//! Un ejecutable externo (`.exe`, `.bat`, `.cmd`, `.ps1`) se registra como referencia, sin
//! copiarlo, y se lanza con el entorno del profile seleccionado y en su workspace. Esta
//! capa también descubre candidatos en los accesos del menú Inicio y del escritorio.
//! El acceso directo de una aplicación del host vive en [`crate::shortcuts`].
//!
//! Contrato: `docs/spec/modules/host_apps.md`.

use std::fmt::Write as _;
use std::fs;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mito_core::Message;
use mito_core::storage::write_json;
use mito_engine::launch::batch_command;
use mito_engine::{Engine, Env, Error, LaunchCommand, Result};
use mito_win::jobs::ProcessJob;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::host_data::{self, HostRoots, Kind, fail, fold, lock, read_records, resolve, stem};
use crate::native::{self, NativeError};

/// Extensiones admitidas, en minúsculas y sin punto.
pub const EXTENSIONS: [&str; 4] = ["exe", "bat", "cmd", "ps1"];

/// Inventario de aplicaciones del host, bajo `core/state`.
pub const INVENTORY: &str = "core/state/host-applications.json";

/// `CREATE_NEW_CONSOLE`.
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Intervalo con el que se espera a que el Job quede vacío.
const JOB_POLL: Duration = Duration::from_millis(100);

/// SHA-256 en hexadecimal minúscula del texto UTF-8.
pub(crate) fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            // Escribir en un `String` no puede fallar.
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// ¿Tiene `path` una extensión admitida?
fn allowed(path: &Path) -> bool {
    extension(path).is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

/// Extensión en minúsculas, sin punto.
fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_lowercase())
}

/// Identidad de una ruta ya resuelta: los 16 primeros hexadecimales del SHA-256 de la
/// ruta sin mayúsculas (`str(path).casefold()`).
///
/// `C:\Apps\Editor\editor.exe` da `c853055b51894dc9`.
pub fn identity(path: &Path) -> String {
    let mut digest = sha256_hex(&fold(&path.display().to_string()));
    digest.truncate(16);
    digest
}

fn inventory_path(root: &Path) -> PathBuf {
    root.join(INVENTORY)
}

fn read_inventory(root: &Path) -> Result<Map<String, Value>> {
    read_records(&inventory_path(root))
}

fn write_inventory(root: &Path, applications: &Map<String, Value>) -> Result<()> {
    Ok(write_json(&inventory_path(root), applications)?)
}

/// Aplicaciones registradas (identidad y ruta), en el orden del archivo.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn inventory(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    Ok(read_inventory(root)?
        .into_iter()
        .filter_map(|(identity, path)| Some((identity, PathBuf::from(path.as_str()?))))
        .collect())
}

/// Ruta registrada para `identity`, o `None` si no está en el inventario.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn inventory_target(root: &Path, identity: &str) -> Result<Option<PathBuf>> {
    Ok(inventory(root)?
        .into_iter()
        .find_map(|(id, path)| (id == identity).then_some(path)))
}

/// Registra el ejecutable `filename` y devuelve su identidad. Es idempotente.
///
/// # Errors
///
/// `Selecciona un archivo .exe, .bat, .cmd o .ps1 existente` si no es una ruta
/// absoluta a un archivo con una extensión admitida.
pub fn add(root: &Path, filename: &Path) -> Result<String> {
    if !filename.is_absolute() || !filename.is_file() || !allowed(filename) {
        return fail(Message::new(
            "Selecciona un archivo .exe, .bat, .cmd o .ps1 existente",
        ));
    }
    let path = dunce::canonicalize(filename).map_err(|e| Error::io(filename, e))?;
    let identity = identity(&path);
    let _lock = lock(root)?;
    let mut applications = read_inventory(root)?;
    applications.insert(identity.clone(), Value::String(path.display().to_string()));
    write_inventory(root, &applications)?;
    Ok(identity)
}

/// Carpetas con los accesos del host: menú Inicio del usuario y común, y escritorio del
/// usuario y público (CSIDL 2, 23, 16 y 25). Se leen del shell, no de `APPDATA` ni
/// `USERPROFILE`, que dentro de un profile apuntan al home de MITO.
pub fn shortcut_folders() -> Vec<PathBuf> {
    use mito_win::shell::{KnownFolder, known_folder};
    [
        KnownFolder::Programs,
        KnownFolder::CommonPrograms,
        KnownFolder::Desktop,
        KnownFolder::PublicDesktop,
    ]
    .into_iter()
    .filter_map(|folder| known_folder(folder).ok())
    .filter(|path| !path.as_os_str().is_empty())
    .collect()
}

/// ¿Es un desinstalador? Su nombre empieza por `unins` o contiene `uninstall` o
/// `desinstal`.
pub fn uninstaller(path: &Path) -> bool {
    let name = fold(&stem(path));
    name.starts_with("unins") || name.contains("uninstall") || name.contains("desinstal")
}

/// Clave de comparación de una ruta: sin mayúsculas, con `\` y sin `\\?\` ni `\` final.
fn path_key(path: &Path) -> String {
    let text = dunce::simplified(path)
        .display()
        .to_string()
        .replace('/', "\\");
    fold(text.trim_end_matches('\\'))
}

/// Accesos `.lnk` bajo `folder`, recursivamente y en orden de nombre.
fn find_links(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(listing) = fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = listing
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort_by_key(|p| fold(&p.display().to_string()));
    for path in paths {
        if path.is_dir() {
            find_links(&path, found);
        } else if path.is_file() && extension(&path).is_some_and(|e| e == "lnk") {
            found.push(path);
        }
    }
}

/// Aplicaciones del host (etiqueta y ejecutable) que apuntan los accesos `.lnk` de
/// `folders`, con `resolve_target` como lector de destinos.
///
/// Solo se mantienen destinos `.exe`, `.bat`, `.cmd` o `.ps1` existentes fuera de MITO,
/// sin desinstaladores ni aplicaciones ya registradas; un mismo destino aparece una vez
/// (gana el primer acceso por nombre). El resultado va ordenado por etiqueta. Sin ningún
/// `.lnk` no se llama a `resolve_target`.
///
/// # Errors
///
/// Los de lectura del inventario.
pub fn discover(
    root: &Path,
    folders: &[PathBuf],
    resolve_target: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<Vec<(String, PathBuf)>> {
    let mut links = Vec::new();
    for folder in folders.iter().filter(|f| f.is_dir()) {
        find_links(folder, &mut links);
    }
    if links.is_empty() {
        return Ok(Vec::new());
    }
    let known: Vec<String> = inventory(root)?
        .iter()
        .map(|(_, path)| path_key(path))
        .collect();
    let mito = format!("{}\\", path_key(&resolve(root)));
    links.sort_by_key(|link| fold(&stem(link)));
    let mut found: Vec<(String, (String, PathBuf))> = Vec::new();
    for link in links {
        let Some(path) = resolve_target(&link).filter(|p| !p.as_os_str().is_empty()) else {
            continue;
        };
        let key = path_key(&path);
        if found.iter().any(|(k, _)| *k == key)
            || known.contains(&key)
            || key.starts_with(&mito)
            || !allowed(&path)
            || uninstaller(&path)
            || uninstaller(&link)
            || !path.is_file()
        {
            continue;
        }
        found.push((key, (stem(&link), path)));
    }
    let mut result: Vec<(String, PathBuf)> = found.into_iter().map(|(_, item)| item).collect();
    result.sort_by_key(|(label, _)| fold(label));
    Ok(result)
}

/// Aplicaciones del host que descubre [`discover`] en el menú Inicio y el escritorio
/// reales, leyendo cada `.lnk` con `IShellLinkW`.
///
/// # Errors
///
/// Los de [`discover`].
pub fn discover_host(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    discover(root, &shortcut_folders(), &|link| {
        mito_win::shell::read_link(link)
            .ok()
            .map(|l| l.target)
            .filter(|target| !target.as_os_str().is_empty())
    })
}

/// Ruta registrada de `identity`.
///
/// # Errors
///
/// `La aplicación del host ya no está registrada en MITO`.
pub fn target(root: &Path, identity: &str) -> Result<PathBuf> {
    inventory_target(root, identity)?.map_or_else(
        || {
            fail(Message::new(
                "La aplicación del host ya no está registrada en MITO",
            ))
        },
        Ok,
    )
}

/// Quita la referencia a la aplicación y los junctions de sus datos y caché. El
/// ejecutable y los datos del host se conservan.
///
/// # Errors
///
/// Los de [`host_data::unlink_locked`]: si un enlace ha cambiado, falla antes de quitar la
/// referencia.
pub fn remove(root: &Path, host: &HostRoots, identity: &str) -> Result<()> {
    let _lock = lock(root)?;
    for kind in Kind::ALL {
        host_data::unlink_locked(root, host, identity, kind)?;
    }
    let mut applications = read_inventory(root)?;
    applications.shift_remove(identity);
    write_inventory(root, &applications)
}

/// Ruta del ejecutable registrado, comprobando que sigue existiendo.
fn available(root: &Path, identity: &str) -> Result<PathBuf> {
    let path = target(root, identity)?;
    if !path.is_file() || !allowed(&path) {
        return fail(Message::new(
            "El archivo de la aplicación del host ya no está disponible",
        ));
    }
    Ok(path)
}

/// Carpeta `System32` del entorno del profile (`SYSTEMROOT`), con la del proceso o
/// `C:\Windows` como último recurso.
fn system32(env: &Env) -> PathBuf {
    let root = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("SYSTEMROOT"))
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var("SystemRoot").ok())
        .unwrap_or_else(|| r"C:\Windows".to_owned());
    PathBuf::from(root).join("System32")
}

/// Comando y banderas de creación para `path`: los `.bat`/`.cmd` van por `cmd.exe` con
/// los datos en variables, los `.ps1` por Windows PowerShell y el resto directos.
fn build(
    path: &Path,
    arguments: &[String],
    mut env: Env,
    workspace: PathBuf,
) -> Result<(LaunchCommand, u32)> {
    let system = system32(&env);
    let (program, args, raw_args, flags) = match extension(path).as_deref() {
        Some("bat" | "cmd") => (
            system.join("cmd.exe").display().to_string(),
            Vec::new(),
            Some(batch_command(path, arguments, &mut env)?),
            CREATE_NEW_CONSOLE,
        ),
        Some("ps1") => {
            let mut args: Vec<String> = [
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &path.display().to_string(),
            ]
            .map(str::to_owned)
            .into();
            args.extend(arguments.iter().cloned());
            (
                system
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
                    .display()
                    .to_string(),
                args,
                None,
                CREATE_NEW_CONSOLE,
            )
        }
        _ => (path.display().to_string(), arguments.to_vec(), None, 0),
    };
    Ok((
        LaunchCommand {
            program,
            args,
            raw_args,
            cwd: workspace,
            env,
        },
        flags,
    ))
}

/// Comando con el que se lanza la aplicación en `profile` y sus banderas de creación,
/// sin ejecutarlo. Crea el workspace del profile.
///
/// # Errors
///
/// `El archivo de la aplicación del host ya no está disponible`, `La aplicación del host
/// ya no está registrada en MITO`, `INVALID_ARGUMENT: …` para `.bat`/`.cmd` con comillas
/// y los del entorno del profile.
pub fn launch_command(
    engine: &Engine,
    profile: &str,
    identity: &str,
    arguments: &[String],
) -> Result<(LaunchCommand, u32)> {
    let path = available(engine.root(), identity)?;
    let (env, workspace) = engine.execution_context(profile)?;
    build(&path, arguments, env, workspace)
}

/// Lanza la aplicación del host con el entorno y el workspace de `profile`.
///
/// - Independiente: arranca el proceso sin esperarlo y devuelve `0`.
/// - `owned`: lo ejecuta dentro de un Job, espera a todo su árbol y devuelve el código de
///   salida del proceso.
/// - `coupled`: abre una pestaña de la terminal (`host-run --owned`) y devuelve `0`.
///
/// # Errors
///
/// `Modo de lanzamiento no válido` con `coupled` y `owned` a la vez, los de
/// [`launch_command`] y los de E/S al arrancar.
pub fn launch(
    engine: &Engine,
    profile: &str,
    identity: &str,
    arguments: &[String],
    coupled: bool,
    owned: bool,
) -> Result<i32> {
    let path = available(engine.root(), identity)?;
    let (env, workspace) = engine.execution_context(profile)?;
    if coupled {
        if owned {
            return fail(Message::new("Modo de lanzamiento no válido"));
        }
        mito_terminal::launch_host(engine, profile, identity, &stem(&path), arguments)?;
        return Ok(0);
    }
    let (command, flags) = build(&path, arguments, env, workspace)?;
    if owned {
        return run_owned(&command);
    }
    command
        .to_command()
        .creation_flags(flags)
        .spawn()
        .map_err(|e| Error::io(&command.program, e))?;
    Ok(0)
}

/// Ejecuta `command` en un Job, espera al proceso y a su árbol y devuelve su código.
fn run_owned(command: &LaunchCommand) -> Result<i32> {
    let fail = |e| Error::io(&command.program, e);
    let mut job = ProcessJob::new().map_err(fail)?;
    let mut process = command.to_command();
    let result = job
        .spawn(&mut process, 0)
        .and_then(|mut child| child.wait())
        .and_then(|status| {
            job.wait_empty(JOB_POLL)
                .map(|()| status.code().unwrap_or(1))
        });
    job.close();
    result.map_err(fail)
}

/// Abre el selector nativo de archivo para agregar una aplicación del host. `None` si se
/// cancela.
///
/// # Errors
///
/// Los de [`native::choose_file`].
pub fn choose_file(
    translate: &dyn Fn(&str) -> String,
) -> std::result::Result<Option<PathBuf>, NativeError> {
    native::choose_file(translate)
}

#[cfg(test)]
mod tests;
