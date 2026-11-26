//! Clasificación de compatibilidad de un manifiesto (`Catalog.compatibility`).
//!
//! Contrato: `docs/spec/modules/catalog.md` («Reglas de `compatibility`»). Es una función
//! pura: toda validación de ruta es sintáctica y no toca el disco ni resuelve rutas del
//! host (invariante 1).
//!
//! Diferencia deliberada (ADR 0008, punto 1): en el original, un `bin` que no es texto ni
//! lista, un `env_set` que no es un objeto o un valor de variable que no es texto lanzan
//! `AttributeError`, que `compatibility` no captura. Aquí se clasifican como no
//! soportados con un motivo, igual que el resto de tipos inesperados.

use mito_core::Message;
use mito_core::storage::{PortablePath, command_name, identifier, portable_path};
use mito_transfer::{download_filename, download_hashes};
use serde_json::{Map, Value};

use crate::error::Error;
use crate::manifest::{Manifest, array, installer_definition, is_text_or_lines};
use crate::pyjson::{py_str, py_truthy, py_type_name};
use crate::pypath;
use crate::resources::required_buckets;

/// Texto que identifica un manifiesto instalable.
pub const COMPATIBLE: &str = "Compatible";

/// Extensiones de descarga admitidas (comparadas en minúsculas). Una URL sin extensión
/// se acepta.
const ALLOWED_EXTENSIONS: [&str; 18] = [
    ".zip", ".7z", ".rar", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".nupkg", ".msi",
    ".exe", ".dll", ".jar", ".ico", ".ps1", ".cmd", ".bat", ".com",
];

/// Variables de entorno que un manifiesto no puede definir (en mayúsculas).
const RESERVED_VARIABLES: [&str; 14] = [
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "PATH",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "HOMEDRIVE",
    "HOMEPATH",
    "MITO_ROOT",
    "MITO_PROFILE",
];

/// Resultado de clasificar un manifiesto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    /// MITO puede instalarlo.
    Compatible,
    /// MITO no puede instalarlo; el mensaje explica el motivo y se muestra tal cual.
    Unsupported(Message),
}

impl Compatibility {
    /// ¿Es instalable?
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    /// Mensaje de rechazo, si lo hay.
    pub fn reason(&self) -> Option<&Message> {
        match self {
            Self::Compatible => None,
            Self::Unsupported(message) => Some(message),
        }
    }

    /// Texto español del estado: `Compatible` o el mensaje de rechazo. Es la cadena que
    /// el original devuelve y que muestran la CLI y el log.
    pub fn text(&self) -> String {
        match self {
            Self::Compatible => COMPATIBLE.to_owned(),
            Self::Unsupported(message) => message.to_string(),
        }
    }
}

impl std::fmt::Display for Compatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

/// Motivo interno de rechazo: con o sin el prefijo `Manifiesto no soportado:`.
enum Reject {
    /// El original devuelve el mensaje tal cual.
    Plain(Message),
    /// El original capturó una excepción y antepone el prefijo al texto del error.
    Error(String),
}

impl From<Error> for Reject {
    fn from(error: Error) -> Self {
        Self::Error(error.to_string())
    }
}

impl From<mito_core::Error> for Reject {
    fn from(error: mito_core::Error) -> Self {
        Self::Error(error.to_string())
    }
}

fn plain(template: &'static str) -> Reject {
    Reject::Plain(Message::new(template))
}

/// Error de tipo de Python para un valor que debía ser una ruta.
fn not_a_path(value: &Value) -> Reject {
    Reject::Error(format!(
        "argument should be a str or an os.PathLike object where __fspath__ returns a str, not '{}'",
        py_type_name(value)
    ))
}

fn path_of(value: &Value) -> Result<PortablePath, Reject> {
    match value {
        Value::String(text) => Ok(portable_path(text)?),
        other => Err(not_a_path(other)),
    }
}

/// Clasifica un manifiesto tal como viene del snapshot (sin `effective` previo).
///
/// Las reglas se aplican en el orden del contrato y la primera que falla decide.
pub fn compatibility(key: &str, manifest: &Manifest) -> Compatibility {
    match classify(key, manifest) {
        Ok(()) => Compatibility::Compatible,
        Err(Reject::Plain(message)) => Compatibility::Unsupported(message),
        Err(Reject::Error(error)) => Compatibility::Unsupported(
            Message::new("Manifiesto no soportado: {error}").arg("error", error),
        ),
    }
}

fn classify(key: &str, manifest: &Manifest) -> Result<(), Reject> {
    let effective = manifest.effective()?;
    let Some(m) = effective.as_object() else {
        return Err(Reject::Error(
            "El manifiesto debe ser un objeto JSON".into(),
        ));
    };
    required_buckets(key, m)?;
    if let Some(script) = m.get("pre_install")
        && !script.is_null()
        && !is_text_or_lines(script)
    {
        return Err(plain("pre_install debe ser texto o una lista de líneas"));
    }
    let Some(version) = m.get("version") else {
        return Err(Reject::Error("'version'".into()));
    };
    identifier(&py_str(version))?;
    if let Err(error) = installer_definition(m) {
        return Err(Reject::Plain(
            error
                .message()
                .unwrap_or_else(|| Message::new(error.to_string())),
        ));
    }
    if let Some(innosetup) = m.get("innosetup")
        && !innosetup.is_boolean()
    {
        return Err(Reject::Plain(
            Message::new("Pasos no soportados: ").with_suffix("innosetup"),
        ));
    }
    check_persist(m)?;
    let urls = array(m.get("url"));
    if urls.is_empty() {
        return Err(plain("Falta URL de descarga"));
    }
    download_hashes(urls.len(), m.get("hash"))?;
    for url in urls {
        check_url(url)?;
    }
    check_shortcuts(m)?;
    for field in ["extract_dir", "extract_to", "env_add_path"] {
        for value in array(m.get(field)) {
            if !value.is_null() {
                path_of(value)?;
            }
        }
    }
    if m.get("psmodule").is_some_and(py_truthy) {
        return Err(plain("Pasos no soportados: psmodule"));
    }
    check_bin(m)?;
    check_env_set(m)
}

/// Segmentos en minúsculas: `PureWindowsPath` compara sin distinguir mayúsculas.
fn folded(path: &PortablePath) -> Vec<String> {
    path.segments().iter().map(|s| s.to_lowercase()).collect()
}

fn check_persist(m: &Map<String, Value>) -> Result<(), Reject> {
    let mut persisted: Vec<Vec<String>> = Vec::new();
    for item in array(m.get("persist")) {
        let (source, destination) = match item {
            Value::Array(parts) if parts.len() == 2 => (&parts[0], &parts[1]),
            Value::Array(_) => {
                return Err(Reject::Error(
                    Message::new("persist debe ser una ruta o [origen, destino]").to_string(),
                ));
            }
            other => (other, other),
        };
        let source_path = path_of(source)?;
        path_of(destination)?;
        let concrete = |value: &Value| !matches!(value.as_str(), Some("." | ""));
        if !concrete(source) || !concrete(destination) {
            return Err(plain("persist requiere una ruta concreta"));
        }
        let current = folded(&source_path);
        if persisted
            .iter()
            .any(|other| current.starts_with(other) || other.starts_with(&current))
        {
            return Err(plain("persist contiene rutas solapadas"));
        }
        persisted.push(current);
    }
    Ok(())
}

/// Esquema de una URL como lo calcula `urllib.parse.urlsplit`, en minúsculas.
fn url_scheme(url: &str) -> String {
    let cleaned: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    match cleaned.split_once(':') {
        Some((scheme, _))
            if !scheme.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            scheme.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn check_url(url: &Value) -> Result<(), Reject> {
    let Some(url) = url.as_str() else {
        return Err(plain("Requiere HTTP o HTTPS"));
    };
    if !matches!(url_scheme(url).as_str(), "http" | "https") {
        return Err(plain("Requiere HTTP o HTTPS"));
    }
    let filename = download_filename(url)?.to_lowercase();
    if !pypath::suffix(&filename).is_empty()
        && !ALLOWED_EXTENSIONS.iter().any(|ext| filename.ends_with(ext))
    {
        return Err(plain("Formato de archivo todavía no soportado"));
    }
    Ok(())
}

fn check_shortcuts(m: &Map<String, Value>) -> Result<(), Reject> {
    let shortcuts = match m.get("shortcuts") {
        Some(Value::Array(items)) => items,
        // Iterar un texto o un objeto solo produce elementos que no son listas.
        None | Some(Value::String(_) | Value::Object(_)) => return Ok(()),
        Some(other) => {
            return Err(Reject::Error(format!(
                "'{}' object is not iterable",
                py_type_name(other)
            )));
        }
    };
    for shortcut in shortcuts {
        let Value::Array(parts) = shortcut else {
            continue;
        };
        let Some(icon) = parts.get(3).filter(|icon| py_truthy(icon)) else {
            continue;
        };
        let valid = icon.as_str().is_some_and(|text| {
            matches!(
                pypath::suffix(text).to_lowercase().as_str(),
                ".ico" | ".exe" | ".dll"
            )
        });
        if !valid {
            return Err(Reject::Plain(
                Message::new("Formato de icono no soportado para el acceso directo: {p1}")
                    .arg("p1", py_str(icon)),
            ));
        }
        path_of(icon)?;
    }
    Ok(())
}

fn check_bin(m: &Map<String, Value>) -> Result<(), Reject> {
    for item in array(m.get("bin")) {
        let (executable, alias): (&Value, Value) = match item {
            Value::Array(parts) => {
                if !matches!(parts.len(), 2 | 3) {
                    return Err(plain(
                        "bin debe tener ejecutable, alias y argumentos opcionales",
                    ));
                }
                (&parts[0], parts[1].clone())
            }
            Value::String(text) => (item, Value::String(pypath::stem(text).to_owned())),
            other => {
                return Err(Reject::Error(format!(
                    "'{}' object has no attribute 'replace'",
                    py_type_name(other)
                )));
            }
        };
        path_of(executable)?;
        match &alias {
            Value::String(name) => {
                command_name(name)?;
            }
            other => {
                return Err(Reject::Error(
                    Message::new("Nombre de comando no válido: {p1}")
                        .arg("p1", crate::pyjson::py_repr(other))
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_env_set(m: &Map<String, Value>) -> Result<(), Reject> {
    let variables = match m.get("env_set") {
        None => return Ok(()),
        Some(Value::Object(map)) => map,
        Some(other) => {
            return Err(Reject::Error(format!(
                "'{}' object has no attribute 'items'",
                py_type_name(other)
            )));
        }
    };
    for (name, value) in variables {
        let reserved = RESERVED_VARIABLES.contains(&name.to_uppercase().as_str());
        if reserved || !is_variable_name(name) {
            return Err(Reject::Plain(
                Message::new("Variable reservada: {name}").arg("name", name),
            ));
        }
        let Some(text) = value.as_str() else {
            return Err(Reject::Error(format!(
                "'{}' object has no attribute 'replace'",
                py_type_name(value)
            )));
        };
        let stripped = text
            .replace("${dir}", "")
            .replace("$dir", "")
            .replace("${persist_dir}", "")
            .replace("$persist_dir", "");
        if stripped.contains('$') || text.contains('%') {
            return Err(Reject::Plain(
                Message::new("Variable no soportada: {name}").arg("name", name),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
