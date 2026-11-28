//! Entradas «Abrir con» de Windows por componente (`open_with.py`).
//!
//! Cada registro crea un ProgID propio por profile, componente y acceso directo
//! (`HKCU\Software\Classes\MITO.<16 hex>`) cuyo comando es el mismo lanzador que los
//! accesos directos (`mitow.exe run --profile --app`, ADR 0013), de modo que se aplican
//! el entorno del profile, el workspace y las copias por profile. Las extensiones las
//! elige el usuario y la aplicación predeterminada nunca se cambia. Los registros viven en
//! `core/state/open-with.json`.
//!
//! Todas las operaciones de registro trabajan bajo una [`Classes`], que decide qué rama de
//! HKCU se usa (`Software\Classes` en producción, una rama de pruebas en los tests).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mito_core::storage::{identifier, read_json, write_json};
use mito_core::{Message, MutationLock};
use mito_engine::{Engine, Error, Log, LogLine, Record, Result};
use mito_win::registry::{Hive, Key};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

/// Registro de «Abrir con» relativo a la raíz de la instancia.
pub const REGISTRY: &str = "core/state/open-with.json";

/// Rama de HKCU de las clases en producción.
const CLASSES: &str = r"Software\Classes";

/// Sugerencias del desplegable «Habituales» (el orden es el del diálogo).
pub const COMMON: [&str; 62] = [
    ".pdf", ".epub", ".djvu", ".cbz", ".txt", ".md", ".rtf", ".csv", ".json", ".xml", ".html",
    ".log", ".docx", ".doc", ".xlsx", ".xls", ".pptx", ".ppt", ".odt", ".ods", ".odp", ".png",
    ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".svg", ".tif", ".ico", ".mp3", ".flac", ".wav",
    ".ogg", ".m4a", ".aac", ".opus", ".mp4", ".mkv", ".avi", ".mov", ".webm", ".wmv", ".zip",
    ".7z", ".rar", ".tar", ".gz", ".py", ".js", ".ts", ".java", ".c", ".cpp", ".cs", ".go", ".rs",
    ".ps1", ".bat", ".ini", ".yaml", ".toml",
];

/// Grupos de extensiones que suelen abrir aplicaciones conocidas: solo sugerencias del
/// diálogo (los nombres son marcas y no se traducen).
pub const PRESETS: [(&str, &[&str]); 8] = [
    (
        "Word",
        &[".docx", ".doc", ".docm", ".dotx", ".dot", ".rtf", ".odt"],
    ),
    (
        "Excel",
        &[".xlsx", ".xls", ".xlsm", ".xlsb", ".xltx", ".csv", ".ods"],
    ),
    (
        "PowerPoint",
        &[".pptx", ".ppt", ".pptm", ".ppsx", ".pps", ".potx", ".odp"],
    ),
    (
        "PDF / eBook",
        &[
            ".pdf", ".epub", ".mobi", ".azw3", ".djvu", ".xps", ".cbz", ".cbr",
        ],
    ),
    (
        "7-Zip",
        &[
            ".7z", ".zip", ".rar", ".tar", ".gz", ".tgz", ".bz2", ".xz", ".zst", ".iso", ".cab",
            ".wim",
        ],
    ),
    (
        "VLC",
        &[
            ".mp4", ".mkv", ".avi", ".mov", ".wmv", ".webm", ".flv", ".mpg", ".mpeg", ".m4v",
            ".3gp", ".mp3", ".flac", ".wav", ".ogg", ".m4a", ".aac", ".opus", ".wma",
        ],
    ),
    (
        "IrfanView",
        &[
            ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tif", ".tiff", ".ico", ".heic",
        ],
    ),
    (
        "Notepad++",
        &[
            ".txt", ".log", ".md", ".ini", ".cfg", ".conf", ".json", ".xml", ".yaml", ".yml",
            ".toml", ".ps1", ".bat", ".cmd", ".py", ".js", ".ts", ".html", ".css",
        ],
    ),
];

/// Rama de HKCU (y, para leer, de HKLM) donde viven las clases de archivo y de protocolo.
///
/// En producción es `Software\Classes`; los tests usan [`Classes::under`], que escribe en
/// una rama propia y no avisa al Explorador.
#[derive(Debug, Clone)]
pub struct Classes {
    user: String,
    machine: Option<String>,
    notify: bool,
}

impl Classes {
    /// `HKCU\Software\Classes` (y `HKLM\Software\Classes` para leer).
    pub fn system() -> Self {
        Self {
            user: CLASSES.to_owned(),
            machine: Some(CLASSES.to_owned()),
            notify: true,
        }
    }

    /// Rama de pruebas `HKCU\<base>\Classes`, sin HKLM y sin avisar al Explorador.
    pub fn under(base: &str) -> Self {
        Self {
            user: format!(r"{base}\Classes"),
            machine: None,
            notify: false,
        }
    }

    /// Ruta bajo HKCU de `relative` (vacío es la propia rama).
    pub(crate) fn user_path(&self, relative: &str) -> String {
        if relative.is_empty() {
            self.user.clone()
        } else {
            format!(r"{}\{relative}", self.user)
        }
    }

    /// Rama de HKLM que se lee, si se usa.
    pub(crate) fn machine(&self) -> Option<&str> {
        self.machine.as_deref()
    }

    /// Rama de HKCU.
    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    /// `SHChangeNotify(SHCNE_ASSOCCHANGED)`: el Explorador recarga las asociaciones.
    pub(crate) fn notify(&self) {
        if self.notify {
            mito_win::shell::notify_associations_changed();
        }
    }
}

/// Error de registro con la ruta de la clave.
pub(crate) fn registry_error(path: &str, source: io::Error) -> Error {
    Error::io(PathBuf::from(format!(r"HKCU\{path}")), source)
}

/// Espera por el lock de mutaciones (la misma que el motor por defecto: no esperar).
pub(crate) fn mutation(engine: &Engine) -> Result<MutationLock> {
    Ok(MutationLock::mutation(engine.layout(), Duration::ZERO)?)
}

/// Borra de HKCU `path` con todo su contenido; que no exista no es un error.
///
/// # Errors
///
/// El error del sistema.
pub(crate) fn delete_tree(path: &str) -> Result<()> {
    let Some((parent, name)) = path.rsplit_once('\\') else {
        return Ok(());
    };
    let Some(key) =
        Key::open(Hive::CurrentUser, parent, true).map_err(|e| registry_error(parent, e))?
    else {
        return Ok(());
    };
    key.delete_tree(name).map_err(|e| registry_error(path, e))
}

/// Ejecutable sin consola que registran los comandos: `core\bin\mitow.exe` (ADR 0013).
pub fn launcher(root: &Path) -> PathBuf {
    root.join("core").join("bin").join("mitow.exe")
}

/// Registro de «Abrir con» de un componente, profile y acceso directo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWithRecord {
    /// ProgID `MITO.<16 hex>`.
    pub progid: String,
    /// Profile.
    pub profile: String,
    /// Id de instalación (`bucket/app`).
    pub installation: String,
    /// Clave del componente (`lock['key']`).
    pub app: String,
    /// Acceso directo (`None` para la entrada principal).
    pub shortcut: Option<String>,
    /// Extensiones registradas, con su punto, sin repetidos.
    pub extensions: Vec<String>,
}

impl OpenWithRecord {
    /// JSON del registro, con los campos en el orden del original.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("progid".into(), Value::String(self.progid.clone()));
        map.insert("profile".into(), Value::String(self.profile.clone()));
        map.insert(
            "installation".into(),
            Value::String(self.installation.clone()),
        );
        map.insert("app".into(), Value::String(self.app.clone()));
        map.insert(
            "shortcut".into(),
            self.shortcut.clone().map_or(Value::Null, Value::String),
        );
        map.insert(
            "extensions".into(),
            Value::Array(self.extensions.iter().cloned().map(Value::String).collect()),
        );
        Value::Object(map)
    }

    /// Lee un registro; `None` si falta algún campo.
    pub fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |name: &str| object.get(name)?.as_str().map(str::to_owned);
        Some(Self {
            progid: text("progid")?,
            profile: text("profile")?,
            installation: text("installation")?,
            app: text("app")?,
            shortcut: text("shortcut"),
            extensions: object
                .get("extensions")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn registry_file(root: &Path) -> PathBuf {
    root.join("core").join("state").join("open-with.json")
}

/// Contenido bruto de `core/state/open-with.json` (vacío si no existe).
fn load(root: &Path) -> Result<Map<String, Value>> {
    let file = registry_file(root);
    if !file.exists() {
        return Ok(Map::new());
    }
    let value: Value = read_json(&file)?;
    Ok(match value {
        Value::Object(map) => map,
        _ => Map::new(),
    })
}

fn store(root: &Path, records: &Map<String, Value>) -> Result<()> {
    Ok(write_json(&registry_file(root), records)?)
}

/// Registros de `core/state/open-with.json` (los que tienen forma válida), en su orden.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn registry(root: &Path) -> Result<Vec<OpenWithRecord>> {
    Ok(load(root)?
        .values()
        .filter_map(OpenWithRecord::from_value)
        .collect())
}

/// ¿Es `value` una extensión válida (`^\.[a-z0-9][a-z0-9_+-]{0,15}$`)?
fn valid_extension(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('.') else {
        return false;
    };
    let mut chars = rest.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && rest.chars().count() <= 16
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-'))
}

/// Interpreta «pdf, .epub djvu» como `[".pdf", ".epub", ".djvu"]`: minúsculas, punto
/// añadido, sin repetidos y en orden.
///
/// # Errors
///
/// `Extensión no válida: {p1}` o `Indica al menos una extensión`.
pub fn extensions(text: &str) -> Result<Vec<String>> {
    let mut result: Vec<String> = Vec::new();
    for part in text
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|part| !part.is_empty())
    {
        let value = if part.starts_with('.') {
            part.to_owned()
        } else {
            format!(".{part}")
        };
        if !valid_extension(&value) {
            return Err(Error::domain(
                Message::new("Extensión no válida: {p1}").arg("p1", part),
            ));
        }
        if !result.contains(&value) {
            result.push(value);
        }
    }
    if result.is_empty() {
        return Err(Error::domain(Message::new("Indica al menos una extensión")));
    }
    Ok(result)
}

/// ProgID de un profile, componente y acceso directo: `MITO.` y los 16 primeros
/// caracteres hexadecimales del SHA-256 de `profile/app[/shortcut]`.
pub fn progid(profile: &str, app: &str, shortcut: Option<&str>) -> String {
    let seed = match shortcut {
        None => format!("{profile}/{app}"),
        Some(shortcut) => format!("{profile}/{app}/{shortcut}"),
    };
    let digest = Sha256::digest(seed.as_bytes());
    let hex: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("MITO.{hex}")
}

/// Nombre que muestra el menú «Abrir con».
pub fn display_name(profile: &str, app: &str, shortcut: Option<&str>) -> String {
    let name = format!("{profile} - {}", app.replace('/', "-"));
    match shortcut {
        None => name,
        Some(shortcut) => format!("{name} - {shortcut}"),
    }
}

/// Línea de comando del lanzador para `profile`, `app` y `shortcut`, terminada en
/// ` -- "%1"`: `<raíz>\core\bin\mitow.exe run --profile <p> --app <a> [--shortcut <s>] -- "%1"`.
pub fn command(root: &Path, profile: &str, app: &str, shortcut: Option<&str>) -> String {
    let mut argv = vec![
        launcher(root).to_string_lossy().into_owned(),
        "run".to_owned(),
        "--profile".to_owned(),
        profile.to_owned(),
        "--app".to_owned(),
        app.to_owned(),
    ];
    if let Some(shortcut) = shortcut {
        argv.push("--shortcut".to_owned());
        argv.push(shortcut.to_owned());
    }
    format!("{} -- \"%1\"", mito_terminal::list2cmdline(&argv))
}

/// Icono del ProgID y de los protocolos de un componente.
///
/// Por ahora es el propio `mitow.exe` (icono 0). PUNTO DE ENGANCHE: cuando exista el
/// icono con insignia de `shortcuts::application_icon` (nombre `open-<tag>-<hash8>`, ver
/// `docs/spec/modules/open_with.md`), se devuelve aquí; ningún otro código decide el icono.
///
/// # Errors
///
/// Los de generar el icono (hoy ninguno).
pub fn icon_for(
    engine: &Engine,
    _installation: &Record,
    _profile: &str,
    _shortcut: Option<&str>,
) -> Result<PathBuf> {
    Ok(launcher(engine.root()))
}

fn set(key: &Key, path: &str, name: &str, value: &str) -> Result<()> {
    key.set_string(name, value)
        .map_err(|e| registry_error(path, e))
}

fn create(classes: &Classes, relative: &str) -> Result<(Key, String)> {
    let path = classes.user_path(relative);
    let key = Key::create(Hive::CurrentUser, &path).map_err(|e| registry_error(&path, e))?;
    Ok((key, path))
}

/// Escribe el ProgID del registro y marca sus extensiones en `OpenWithProgids`.
///
/// # Errors
///
/// El error del sistema.
pub fn write_progid(
    classes: &Classes,
    record: &OpenWithRecord,
    command_line: &str,
    icon: &Path,
) -> Result<()> {
    let name = display_name(&record.profile, &record.app, record.shortcut.as_deref());
    let icon = format!("{},0", icon.display());
    let base = &record.progid;
    let (key, path) = create(classes, base)?;
    set(&key, &path, "", &name)?;
    set(&key, &path, "FriendlyTypeName", &name)?;
    let (key, path) = create(classes, &format!(r"{base}\DefaultIcon"))?;
    set(&key, &path, "", &icon)?;
    // El menú «Abrir con» muestra esto en lugar del nombre y el icono del lanzador.
    let (key, path) = create(classes, &format!(r"{base}\Application"))?;
    set(&key, &path, "ApplicationName", &name)?;
    set(&key, &path, "ApplicationIcon", &icon)?;
    set(&key, &path, "ApplicationDescription", "MITO")?;
    let (key, path) = create(classes, &format!(r"{base}\shell\open"))?;
    set(&key, &path, "FriendlyAppName", &name)?;
    let (key, path) = create(classes, &format!(r"{base}\shell\open\command"))?;
    set(&key, &path, "", command_line)?;
    for extension in &record.extensions {
        let (key, path) = create(classes, &format!(r"{extension}\OpenWithProgids"))?;
        key.set_none(&record.progid)
            .map_err(|e| registry_error(&path, e))?;
    }
    Ok(())
}

/// ¿Es `value` un ProgID propio (`MITO\.[0-9a-f]{16}`)?
fn own_progid(value: &str) -> bool {
    value.strip_prefix("MITO.").is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Retira solo el ProgID propio y su valor de cada `OpenWithProgids`; nunca toca el valor
/// predeterminado de la extensión ni la asociación del usuario.
///
/// # Errors
///
/// `Registro de «Abrir con» no válido` si el ProgID no es `MITO.<16 hex>`, y el error del
/// sistema.
pub fn remove_progid(classes: &Classes, record: &OpenWithRecord) -> Result<()> {
    if !own_progid(&record.progid) {
        return Err(Error::domain(Message::new(
            "Registro de «Abrir con» no válido",
        )));
    }
    for extension in record.extensions.iter().filter(|e| valid_extension(e)) {
        let path = classes.user_path(&format!(r"{extension}\OpenWithProgids"));
        if let Some(key) =
            Key::open(Hive::CurrentUser, &path, true).map_err(|e| registry_error(&path, e))?
        {
            key.delete_value(&record.progid)
                .map_err(|e| registry_error(&path, e))?;
        }
    }
    delete_tree(&classes.user_path(&record.progid))
}

/// Comando registrado del ProgID, o `None` si no existe.
///
/// # Errors
///
/// El error del sistema si no se puede leer.
pub fn current_command(classes: &Classes, record: &OpenWithRecord) -> Result<Option<String>> {
    let path = classes.user_path(&format!(r"{}\shell\open\command", record.progid));
    let Some(key) =
        Key::open(Hive::CurrentUser, &path, false).map_err(|e| registry_error(&path, e))?
    else {
        return Ok(None);
    };
    key.get_string("").map_err(|e| registry_error(&path, e))
}

/// Registra (o sustituye) «Abrir con» para un componente, profile y acceso directo.
///
/// Si ya existía el ProgID se retira antes de escribir; si la escritura falla se retira el
/// nuevo y se elimina el registro (el anterior también se pierde, como en el original).
///
/// # Errors
///
/// `El componente debe estar activo en el profile`, los de `application_target` (acceso
/// directo inexistente o ambiguo), los de [`extensions`] y los de registro y disco.
pub fn register(
    classes: &Classes,
    engine: &Engine,
    profile: &str,
    installation: &str,
    text: &str,
    shortcut: Option<&str>,
) -> Result<OpenWithRecord> {
    let profile = identifier(profile)?;
    let _lock = mutation(engine)?;
    let data = engine.profile(&profile)?;
    let inactive = || {
        Error::domain(Message::new(
            "El componente debe estar activo en el profile",
        ))
    };
    if !data.apps.iter().any(|app| app == installation) {
        return Err(inactive());
    }
    let selected = engine.selected(&data)?;
    let lock = selected.get(installation).ok_or_else(inactive)?;
    engine.application_target(lock, shortcut)?;
    let values = extensions(text)?;
    let app = lock.key.clone();
    let record = OpenWithRecord {
        progid: progid(&profile, &app, shortcut),
        profile: profile.clone(),
        installation: installation.to_owned(),
        app: app.clone(),
        shortcut: shortcut.map(str::to_owned),
        extensions: values,
    };
    let mut records = load(engine.root())?;
    let previous = records
        .get(&record.progid)
        .and_then(OpenWithRecord::from_value);
    let icon = icon_for(engine, lock, &profile, shortcut)?;
    let written = (|| -> Result<()> {
        if let Some(previous) = &previous {
            remove_progid(classes, previous)?;
        }
        write_progid(
            classes,
            &record,
            &command(engine.root(), &profile, &app, shortcut),
            &icon,
        )?;
        records.insert(record.progid.clone(), record.to_value());
        store(engine.root(), &records)
    })();
    if let Err(error) = written {
        let _ = remove_progid(classes, &record);
        if records.remove(&record.progid).is_some() {
            let _ = store(engine.root(), &records);
        }
        classes.notify();
        return Err(error);
    }
    classes.notify();
    Ok(record)
}

/// Registros del profile y la instalación.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn entries(root: &Path, profile: &str, installation: &str) -> Result<Vec<OpenWithRecord>> {
    Ok(registry(root)?
        .into_iter()
        .filter(|record| record.profile == profile && record.installation == installation)
        .collect())
}

/// Retira los registros para los que `select` es cierto y conserva el resto; un fallo por
/// registro se anota en `log` y el registro se conserva.
///
/// # Errors
///
/// Los de lectura y escritura del JSON.
pub fn unregister(
    classes: &Classes,
    root: &Path,
    select: &dyn Fn(&OpenWithRecord) -> bool,
    log: Log<'_>,
) -> Result<Vec<OpenWithRecord>> {
    let mut records = load(root)?;
    let mut removed = Vec::new();
    for (key, value) in records.clone() {
        let Some(record) = OpenWithRecord::from_value(&value) else {
            continue;
        };
        if !select(&record) {
            continue;
        }
        match remove_progid(classes, &record) {
            Ok(()) => {
                records.remove(&key);
                removed.push(record);
            }
            Err(error) => log(LogLine::from(
                Message::new("No se pudo retirar «Abrir con» {p1}: {p3}")
                    .arg("p1", &key)
                    .arg("p3", error),
            )),
        }
    }
    if !removed.is_empty() {
        store(root, &records)?;
        classes.notify();
    }
    Ok(removed)
}

/// Aplica el resultado del diálogo `{entrada: [extensiones]}`; una lista vacía retira la
/// entrada. Es idempotente: si coincide con lo registrado no hace nada.
///
/// # Errors
///
/// Los de [`register`] y [`unregister`].
pub fn apply(
    classes: &Classes,
    engine: &Engine,
    profile: &str,
    installation: &str,
    wanted: &[(String, Vec<String>)],
    log: Log<'_>,
) -> Result<()> {
    let profile = identifier(profile)?;
    let existing = entries(engine.root(), &profile, installation)?;
    for (shortcut, values) in wanted {
        let current = existing
            .iter()
            .find(|record| record.shortcut.as_deref() == Some(shortcut))
            .map(|record| record.extensions.as_slice())
            .unwrap_or_default();
        if values.as_slice() == current {
            continue;
        }
        if values.is_empty() {
            unregister(
                classes,
                engine.root(),
                &|record| {
                    record.profile == profile
                        && record.installation == installation
                        && record.shortcut.as_deref() == Some(shortcut)
                },
                log,
            )?;
            log(LogLine::from(Message::new("Retirado de «Abrir con».")));
        } else {
            let record = register(
                classes,
                engine,
                &profile,
                installation,
                &values.join(" "),
                Some(shortcut),
            )?;
            log(LogLine::from(
                Message::new(
                    "Registrado en «Abrir con» para {p1}. Puede tardar unos segundos en aparecer en el Explorador.",
                )
                .arg("p1", record.extensions.join(" ")),
            ));
        }
    }
    Ok(())
}

/// Comparación de rutas como `os.path.normcase(os.path.normpath(..))` en Windows.
fn normcase(text: &str) -> String {
    let mut value = text.replace('/', "\\").to_lowercase();
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value
}

/// Tras mover la instancia reescribe comando e icono de los registros que apuntaban a
/// `old`; tras copiarla elimina el registro de esta instancia y deja la clave a la
/// original. Devuelve cuántos registros se vieron afectados (ambos casos cuentan).
pub fn relocate(
    classes: &Classes,
    engine: &Engine,
    old: &Path,
    copied: bool,
    log: Log<'_>,
) -> usize {
    let Ok(mut records) = load(engine.root()) else {
        return 0;
    };
    let old_root = normcase(&old.to_string_lossy());
    let mut changed = 0;
    let warn = |key: &str, error: &dyn std::fmt::Display| {
        log(LogLine::from(
            Message::new("No se pudo retirar «Abrir con» {p1}: {p3}")
                .arg("p1", key)
                .arg("p3", error),
        ));
    };
    for (key, value) in records.clone() {
        let Some(record) = OpenWithRecord::from_value(&value) else {
            continue;
        };
        let current = match current_command(classes, &record) {
            Ok(Some(current)) => current,
            Ok(None) => continue,
            Err(error) => {
                warn(&key, &error);
                continue;
            }
        };
        if !normcase(&current).contains(&old_root) {
            continue;
        }
        changed += 1;
        if copied {
            records.remove(&key);
            continue;
        }
        let rewritten = (|| -> Result<()> {
            let data = engine.profile(&record.profile)?;
            let selected = engine.selected(&data)?;
            let lock = selected.get(&record.installation).ok_or_else(|| {
                Error::domain(Message::new(
                    "El componente debe estar activo en el profile",
                ))
            })?;
            let icon = icon_for(engine, lock, &record.profile, record.shortcut.as_deref())?;
            let line = command(
                engine.root(),
                &record.profile,
                &record.app,
                record.shortcut.as_deref(),
            );
            write_progid(classes, &record, &line, &icon)
        })();
        if let Err(error) = rewritten {
            warn(&key, &error);
        }
    }
    if changed > 0 {
        if let Err(error) = store(engine.root(), &records) {
            warn("open-with", &error);
        }
        classes.notify();
    }
    changed
}

/// «Profile borrado»: retira las entradas de ese profile, que de otro modo lanzarían un
/// profile inexistente. Lo llama el motor desde `Host::unregister_profile`.
///
/// # Errors
///
/// Los de [`unregister`].
pub fn profile_deleted(classes: &Classes, engine: &Engine, profile: &str) -> Result<()> {
    unregister(
        classes,
        engine.root(),
        &|record| record.profile == profile,
        &|_| {},
    )?;
    Ok(())
}

/// «Instalación eliminada»: retira las entradas de ese componente en todos los profiles.
/// Lo llama el motor desde `Host::unregister_installation`.
///
/// # Errors
///
/// Los de [`unregister`].
pub fn installation_removed(
    classes: &Classes,
    engine: &Engine,
    installation: &str,
    log: Log<'_>,
) -> Result<()> {
    unregister(
        classes,
        engine.root(),
        &|record| record.installation == installation,
        log,
    )?;
    Ok(())
}

/// Utilidades de los tests de este crate: rama de registro de pruebas e instancia temporal.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "utilidades de tests"
)]
pub(crate) mod testing {
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    use mito_engine::Profile;
    use serde_json::json;

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Rama `HKCU\Software\MITO-tests\<único>` que se borra al soltarla.
    pub struct Sandbox {
        name: String,
        pub classes: Classes,
    }

    impl Sandbox {
        pub fn new() -> Self {
            let name = format!(
                "desktop-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let classes = Classes::under(&format!(r"Software\MITO-tests\{name}"));
            Self { name, classes }
        }

        /// Texto del valor `name` de la clave `relative` de la rama de clases.
        pub fn value(&self, relative: &str, name: &str) -> Option<String> {
            let key =
                Key::open(Hive::CurrentUser, &self.classes.user_path(relative), false).unwrap()?;
            key.get_string(name).unwrap()
        }

        /// ¿Es `name` un `REG_NONE` de la clave `relative`?
        pub fn marked(&self, relative: &str, name: &str) -> bool {
            Key::open(Hive::CurrentUser, &self.classes.user_path(relative), false)
                .unwrap()
                .is_some_and(|key| {
                    key.get(name).unwrap() == Some(mito_win::registry::RegValue::None)
                })
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            if let Ok(Some(parent)) = Key::open(Hive::CurrentUser, r"Software\MITO-tests", true) {
                let _ = parent.delete_tree(&self.name);
            }
        }
    }

    /// Instancia temporal con un componente instalado (`key`, ejecutable y etiqueta del
    /// acceso) activo en los profiles `default` y `work`.
    pub struct Instance {
        _dir: tempfile::TempDir,
        pub engine: Engine,
    }

    impl Instance {
        pub fn new(key: &str, executable: &str, label: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let engine = build(dir.path(), key, executable, label);
            Self { _dir: dir, engine }
        }

        pub fn root(&self) -> &Path {
            self.engine.root()
        }
    }

    /// Crea una instancia en `root` y devuelve su motor.
    pub fn build(root: &Path, key: &str, executable: &str, label: &str) -> Engine {
        fs::create_dir_all(root).unwrap();
        let engine = Engine::new(root);
        engine.initialize().unwrap();
        let folder = engine.root().join("core").join("apps").join(key);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(executable), b"MZ").unwrap();
        let record = Record {
            key: key.to_owned(),
            version: json!("1"),
            manifest_sha256: "0".repeat(64),
            path: format!("core/apps/{key}"),
            manifest: json!({"version": "1", "shortcuts": [[executable, label]]}),
            pinned: None,
        };
        fs::write(
            folder.join(".mito.json"),
            serde_json::to_vec(&record.marker()).unwrap(),
        )
        .unwrap();
        for name in ["default", "work"] {
            let _ = engine.create_profile(name, None);
            let mut profile: Profile = engine.profile(name).unwrap();
            profile.apps = vec![key.to_owned()];
            engine.save_profile(name, &profile).unwrap();
        }
        engine
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests"
)]
mod tests {
    use super::testing::{Instance, Sandbox, build};
    use super::*;

    fn quiet(_: LogLine) {}

    fn fixture() -> (Sandbox, Instance) {
        (
            Sandbox::new(),
            Instance::new("main/foliate", "foliate.exe", "Foliate"),
        )
    }

    #[test]
    fn las_extensiones_se_normalizan_y_validan() {
        assert_eq!(
            extensions("PDF, .epub;djvu pdf").unwrap(),
            [".pdf", ".epub", ".djvu"]
        );
        for text in ["", "  ", "a/b", "..pdf", &"x".repeat(20), ",;"] {
            assert!(extensions(text).is_err(), "{text:?}");
        }
        assert_eq!(
            extensions("a/b").unwrap_err().to_string(),
            "Extensión no válida: a/b"
        );
        assert_eq!(
            extensions(" ").unwrap_err().to_string(),
            "Indica al menos una extensión"
        );
        // El límite es de 16 caracteres tras el punto.
        assert!(extensions(&"x".repeat(16)).is_ok());
        assert!(extensions(&"x".repeat(17)).is_err());
    }

    #[test]
    fn todos_los_presets_son_validos_y_sin_duplicados() {
        for (name, values) in PRESETS {
            assert_eq!(extensions(&values.join(" ")).unwrap(), values, "{name}");
        }
        assert_eq!(extensions(&COMMON.join(" ")).unwrap(), COMMON);
    }

    #[test]
    fn el_progid_y_el_nombre_coinciden_con_el_original() {
        assert_eq!(
            progid("default", "main/foliate", Some("Foliate")),
            "MITO.38613277e964aa60"
        );
        assert_eq!(
            progid("default", "main/foliate", None),
            "MITO.ef616740509e44aa"
        );
        assert_eq!(
            display_name("default", "main/foliate", Some("Foliate")),
            "default - main-foliate - Foliate"
        );
        assert_eq!(
            display_name("default", "main/foliate", None),
            "default - main-foliate"
        );
    }

    #[test]
    fn el_comando_usa_mitow_y_entrecomilla_lo_necesario() {
        let line = command(
            Path::new(r"C:\Mi MITO"),
            "default",
            "main/foliate",
            Some("Mi App"),
        );
        assert_eq!(
            line,
            r#""C:\Mi MITO\core\bin\mitow.exe" run --profile default --app main/foliate --shortcut "Mi App" -- "%1""#
        );
        let line = command(Path::new(r"C:\mito"), "default", "main/foliate", None);
        assert_eq!(
            line,
            r#"C:\mito\core\bin\mitow.exe run --profile default --app main/foliate -- "%1""#
        );
    }

    #[test]
    fn registrar_escribe_el_progid_con_el_lanzador_del_profile() {
        let (sandbox, instance) = fixture();
        let classes = &sandbox.classes;
        let record = register(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            "pdf epub",
            Some("Foliate"),
        )
        .unwrap();
        let progid = record.progid.clone();
        assert_eq!(progid, "MITO.38613277e964aa60");
        let line = sandbox
            .value(&format!(r"{progid}\shell\open\command"), "")
            .unwrap();
        let launcher = launcher(instance.root());
        assert!(
            line.contains(&launcher.to_string_lossy().into_owned()),
            "{line}"
        );
        assert!(line.contains("run --profile default --app main/foliate --shortcut Foliate"));
        assert!(line.ends_with(r#" -- "%1""#));
        let name = "default - main-foliate - Foliate";
        assert_eq!(sandbox.value(&progid, "").as_deref(), Some(name));
        assert_eq!(
            sandbox.value(&progid, "FriendlyTypeName").as_deref(),
            Some(name)
        );
        assert_eq!(
            sandbox
                .value(&format!(r"{progid}\Application"), "ApplicationName")
                .as_deref(),
            Some(name)
        );
        assert_eq!(
            sandbox
                .value(&format!(r"{progid}\Application"), "ApplicationDescription")
                .as_deref(),
            Some("MITO")
        );
        assert_eq!(
            sandbox
                .value(&format!(r"{progid}\shell\open"), "FriendlyAppName")
                .as_deref(),
            Some(name)
        );
        let icon = sandbox
            .value(&format!(r"{progid}\DefaultIcon"), "")
            .unwrap();
        assert!(icon.ends_with(",0"), "{icon}");
        assert!(sandbox.marked(r".pdf\OpenWithProgids", &progid));
        assert!(sandbox.marked(r".epub\OpenWithProgids", &progid));
        assert_eq!(
            entries(instance.root(), "default", "main/foliate").unwrap(),
            [record]
        );
        // Registrar de nuevo sustituye las extensiones anteriores.
        register(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            "djvu",
            Some("Foliate"),
        )
        .unwrap();
        assert!(!sandbox.marked(r".pdf\OpenWithProgids", &progid));
        assert!(sandbox.marked(r".djvu\OpenWithProgids", &progid));
        // Nunca se toca el valor predeterminado de la extensión.
        assert_eq!(sandbox.value(".djvu", ""), None);
    }

    #[test]
    fn el_registro_en_disco_tiene_el_formato_del_original() {
        let (sandbox, instance) = fixture();
        register(
            &sandbox.classes,
            &instance.engine,
            "default",
            "main/foliate",
            "pdf",
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(registry_file(instance.root())).unwrap();
        assert_eq!(
            text,
            "{\n  \"MITO.ef616740509e44aa\": {\n    \"progid\": \"MITO.ef616740509e44aa\",\n    \"profile\": \"default\",\n    \"installation\": \"main/foliate\",\n    \"app\": \"main/foliate\",\n    \"shortcut\": null,\n    \"extensions\": [\n      \".pdf\"\n    ]\n  }\n}\n"
        );
    }

    #[test]
    fn un_componente_inactivo_o_un_acceso_inexistente_se_rechazan() {
        let (sandbox, instance) = fixture();
        let classes = &sandbox.classes;
        assert_eq!(
            register(
                classes,
                &instance.engine,
                "default",
                "main/other",
                "pdf",
                None
            )
            .unwrap_err()
            .to_string(),
            "El componente debe estar activo en el profile"
        );
        let error = register(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            "pdf",
            Some("Missing"),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Shortcut unavailable or ambiguous")
        );
        assert!(registry(instance.root()).unwrap().is_empty());
    }

    #[test]
    fn retirar_solo_quita_los_registros_seleccionados() {
        let (sandbox, instance) = fixture();
        let classes = &sandbox.classes;
        let first = register(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            "pdf",
            None,
        )
        .unwrap();
        let second = register(
            classes,
            &instance.engine,
            "work",
            "main/foliate",
            "pdf",
            None,
        )
        .unwrap();
        unregister(
            classes,
            instance.root(),
            &|record| record.profile == "default",
            &quiet,
        )
        .unwrap();
        assert_eq!(sandbox.value(&first.progid, ""), None);
        assert!(!sandbox.marked(r".pdf\OpenWithProgids", &first.progid));
        assert!(sandbox.marked(r".pdf\OpenWithProgids", &second.progid));
        assert_eq!(registry(instance.root()).unwrap(), [second]);
    }

    #[test]
    fn aplicar_anade_amplia_no_repite_y_retira() {
        let (sandbox, instance) = fixture();
        let classes = &sandbox.classes;
        let messages = std::sync::Mutex::new(Vec::new());
        let log = |line: LogLine| messages.lock().unwrap().push(line.message.to_string());
        let wanted = |values: &[&str]| {
            vec![(
                "Foliate".to_owned(),
                values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
            )]
        };
        apply(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            &wanted(&[".docx"]),
            &log,
        )
        .unwrap();
        let progid = entries(instance.root(), "default", "main/foliate").unwrap()[0]
            .progid
            .clone();
        apply(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            &wanted(&[".docx", ".pdf"]),
            &log,
        )
        .unwrap();
        assert!(sandbox.marked(r".docx\OpenWithProgids", &progid));
        assert!(sandbox.marked(r".pdf\OpenWithProgids", &progid));
        // Sin cambios no se vuelve a registrar: no hay mensaje nuevo.
        let before = messages.lock().unwrap().len();
        apply(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            &wanted(&[".docx", ".pdf"]),
            &log,
        )
        .unwrap();
        assert_eq!(messages.lock().unwrap().len(), before);
        apply(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            &wanted(&[]),
            &log,
        )
        .unwrap();
        assert_eq!(sandbox.value(&progid, ""), None);
        assert!(registry(instance.root()).unwrap().is_empty());
        let messages = messages.lock().unwrap();
        assert_eq!(
            messages[0],
            "Registrado en «Abrir con» para .docx. Puede tardar unos segundos en aparecer en el Explorador."
        );
        assert_eq!(messages.last().unwrap(), "Retirado de «Abrir con».");
    }

    #[test]
    fn un_progid_ajeno_nunca_se_borra() {
        let sandbox = Sandbox::new();
        let record = OpenWithRecord {
            progid: r"Applications\notepad.exe".to_owned(),
            profile: String::new(),
            installation: String::new(),
            app: String::new(),
            shortcut: None,
            extensions: Vec::new(),
        };
        assert_eq!(
            remove_progid(&sandbox.classes, &record)
                .unwrap_err()
                .to_string(),
            "Registro de «Abrir con» no válido"
        );
        for bad in [
            "MITO.ABCDEF0123456789",
            "MITO.123",
            "MITO.38613277e964aa60x",
            "x.38613277e964aa60",
        ] {
            assert!(!own_progid(bad), "{bad}");
        }
    }

    #[test]
    fn mover_reescribe_el_comando_y_copiar_libera_el_registro() {
        let sandbox = Sandbox::new();
        let parent = tempfile::tempdir().unwrap();
        let old_engine = build(
            &parent.path().join("old"),
            "main/foliate",
            "foliate.exe",
            "Foliate",
        );
        let new_engine = build(
            &parent.path().join("new"),
            "main/foliate",
            "foliate.exe",
            "Foliate",
        );
        let classes = &sandbox.classes;
        let record =
            register(classes, &old_engine, "default", "main/foliate", "pdf", None).unwrap();
        // El archivo de registros viaja con la instancia movida.
        std::fs::copy(
            registry_file(old_engine.root()),
            registry_file(new_engine.root()),
        )
        .unwrap();
        let command_path = format!(r"{}\shell\open\command", record.progid);
        assert_eq!(
            relocate(classes, &new_engine, old_engine.root(), false, &quiet),
            1
        );
        let line = sandbox.value(&command_path, "").unwrap();
        assert!(
            line.contains(&*new_engine.root().to_string_lossy()),
            "{line}"
        );
        assert!(
            !line
                .to_lowercase()
                .contains(&old_engine.root().to_string_lossy().to_lowercase())
        );
        // Una copia deja la clave a la instancia original.
        register(classes, &old_engine, "default", "main/foliate", "pdf", None).unwrap();
        std::fs::copy(
            registry_file(old_engine.root()),
            registry_file(new_engine.root()),
        )
        .unwrap();
        assert_eq!(
            relocate(classes, &new_engine, old_engine.root(), true, &quiet),
            1
        );
        let line = sandbox.value(&command_path, "").unwrap();
        assert!(
            line.contains(&*old_engine.root().to_string_lossy()),
            "{line}"
        );
        assert!(registry(new_engine.root()).unwrap().is_empty());
    }

    #[test]
    fn profile_borrado_e_instalacion_eliminada_retiran_sus_entradas() {
        let (sandbox, instance) = fixture();
        let classes = &sandbox.classes;
        let first = register(
            classes,
            &instance.engine,
            "default",
            "main/foliate",
            "pdf",
            None,
        )
        .unwrap();
        let second = register(
            classes,
            &instance.engine,
            "work",
            "main/foliate",
            "pdf",
            None,
        )
        .unwrap();
        profile_deleted(classes, &instance.engine, "default").unwrap();
        assert_eq!(sandbox.value(&first.progid, ""), None);
        assert_eq!(registry(instance.root()).unwrap(), [second.clone()]);
        installation_removed(classes, &instance.engine, "main/foliate", &quiet).unwrap();
        assert_eq!(sandbox.value(&second.progid, ""), None);
        assert!(registry(instance.root()).unwrap().is_empty());
    }
}
