//! Reclamación explícita de esquemas de URL (`claude://`, `vscode://`…) para un
//! componente de MITO (`protocols.py`).
//!
//! Un navegador entrega a Windows los enlaces de esquema personalizado (por ejemplo el
//! retorno de un inicio de sesión) y Windows ejecuta el comando de
//! `HKCU|HKLM\Software\Classes\<esquema>`. La copia del host de la aplicación suele ser
//! la dueña de esa clave, así que el enlace llega al host y no a la instancia de MITO.
//! Reclamar escribe `HKCU\Software\Classes\<esquema>` con el mismo lanzador que «Abrir
//! con» (`mitow.exe run --profile --app -- "%1"`).
//!
//! La reclamación vive solo mientras el diálogo está abierto ([`Session`]): el diálogo la
//! vuelve a afirmar cada [`WATCH_SECONDS`] segundos y la libera al cerrar. El registro
//! anterior se guarda en `core/state/protocols.json` y se restaura al liberar, solo si la
//! clave aún tiene el comando de MITO.
//!
//! Corrección respecto al original (ADR 0013, punto 4): mientras un esquema tenga un
//! registro de MITO, el respaldo es siempre el del dueño original del host. El original
//! lo sustituía por el de la otra aplicación si esta había tomado la clave y se
//! reclamaba de nuevo.

use std::collections::BTreeMap;
use std::path::Path;

use mito_core::storage::{identifier, read_json, write_json};
use mito_core::{Message, MutationLock};
use mito_engine::{Engine, Error, Log, LogLine, Record, Result};
use mito_win::registry::{Hive, Key};
use serde_json::{Map, Value, json};

use crate::open_with::{
    self, Classes, delete_tree, hex, icon_for, launcher, mutation, registry_error,
};

/// Segundos entre dos comprobaciones de un esquema reclamado mientras el diálogo está abierto.
pub const WATCH_SECONDS: u64 = 5;

/// Esquemas que Windows o los navegadores gestionan por sí mismos.
const RESERVED: [&str; 15] = [
    "http",
    "https",
    "file",
    "ftp",
    "mailto",
    "javascript",
    "data",
    "about",
    "blob",
    "res",
    "shell",
    "ms-settings",
    "search-ms",
    "microsoft-edge",
    "mito",
];

const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_DWORD: u32 = 4;
const REG_MULTI_SZ: u32 = 7;
const REG_QWORD: u32 = 11;

/// Estado de un esquema respecto a un profile y un componente, por este orden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// El registro es de este profile y el comando es exactamente el de MITO.
    Mine,
    /// El comando es el lanzador de MITO de otro profile.
    Profile,
    /// El ejecutable está dentro de la raíz de MITO pero no es el lanzador.
    Direct,
    /// Cualquier otra cosa (la aplicación del host, otra app).
    Other,
    /// No hay valor en HKCU ni en HKLM.
    Absent,
}

impl Status {
    /// Etiqueta del estado (se traduce en el borde).
    pub fn label(self) -> Message {
        Message::new(match self {
            Self::Mine => "De este profile",
            Self::Profile => "Otro profile de MITO",
            Self::Direct => "Ejecutable de MITO sin entorno",
            Self::Other => "Otra app",
            Self::Absent => "No registrado",
        })
    }
}

/// Fila del diálogo: un esquema, su estado y el comando que lo resuelve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Esquema en minúsculas.
    pub scheme: String,
    /// Estado.
    pub status: Status,
    /// Comando actual, si hay.
    pub command: Option<String>,
}

/// Registro de una reclamación en `core/state/protocols.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolRecord {
    /// Esquema en minúsculas.
    pub scheme: String,
    /// Profile que reclama.
    pub profile: String,
    /// Id de instalación (`bucket/app`).
    pub installation: String,
    /// Clave del componente.
    pub app: String,
    /// Acceso directo (`None` para la entrada principal).
    pub shortcut: Option<String>,
    /// Árbol de la clave anterior (`None` si no existía), ver [`snapshot`].
    pub backup: Option<Value>,
}

impl ProtocolRecord {
    /// JSON del registro, con los campos en el orden del original.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scheme".into(), json!(self.scheme));
        map.insert("profile".into(), json!(self.profile));
        map.insert("installation".into(), json!(self.installation));
        map.insert("app".into(), json!(self.app));
        map.insert("shortcut".into(), json!(self.shortcut));
        map.insert("backup".into(), self.backup.clone().unwrap_or(Value::Null));
        Value::Object(map)
    }

    /// Lee un registro; `None` si falta algún campo.
    pub fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |name: &str| object.get(name)?.as_str().map(str::to_owned);
        Some(Self {
            scheme: text("scheme")?,
            profile: text("profile")?,
            installation: text("installation")?,
            app: text("app")?,
            shortcut: text("shortcut"),
            backup: object.get("backup").filter(|b| !b.is_null()).cloned(),
        })
    }
}

fn registry_file(root: &Path) -> std::path::PathBuf {
    root.join("core").join("state").join("protocols.json")
}

/// Contenido bruto de `core/state/protocols.json` (vacío si no existe o no es un objeto).
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

/// Registros de `core/state/protocols.json` (los de forma válida), en su orden.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn registry(root: &Path) -> Result<Vec<ProtocolRecord>> {
    Ok(load(root)?
        .values()
        .filter_map(ProtocolRecord::from_value)
        .collect())
}

/// ¿Cumple `^[a-z][a-z0-9+.-]{1,31}$`?
fn valid_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && (1..=31).contains(&value.chars().count().saturating_sub(1))
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '.' | '-'))
}

/// Normaliza un esquema: sin espacios, en minúsculas y sin `://` ni `:` finales.
///
/// # Errors
///
/// `Esquema no válido: {p1}` y `Esquema reservado del sistema: {p1}` (los reservados y
/// todo lo que empieza por `ms-`).
pub fn scheme_name(text: &str) -> Result<String> {
    let lowered = text.trim().to_lowercase();
    let value = lowered.strip_suffix("://").unwrap_or(&lowered);
    let value = value.strip_suffix(':').unwrap_or(value);
    if !valid_scheme(value) {
        return Err(Error::domain(
            Message::new("Esquema no válido: {p1}").arg("p1", text),
        ));
    }
    if RESERVED.contains(&value) || value.starts_with("ms-") {
        return Err(Error::domain(
            Message::new("Esquema reservado del sistema: {p1}").arg("p1", value),
        ));
    }
    Ok(value.to_owned())
}

/// Comando de MITO para el esquema (el mismo que «Abrir con»).
pub fn command(root: &Path, profile: &str, app: &str, shortcut: Option<&str>) -> String {
    open_with::command(root, profile, app, shortcut)
}

// --- Respaldo del registro ---------------------------------------------------

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect()
}

/// Datos de un valor como los guarda el respaldo: cadenas como texto, enteros como
/// números, `REG_MULTI_SZ` como lista y el resto como `{"hex": …}`.
fn value_to_json(kind: u32, data: &[u8]) -> Value {
    match kind {
        REG_SZ | REG_EXPAND_SZ => {
            let units = utf16(data);
            let end = units
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(units.len());
            Value::String(String::from_utf16_lossy(&units[..end]))
        }
        REG_DWORD if data.len() == 4 => {
            json!(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        REG_QWORD if data.len() == 8 => {
            let mut bytes = [0_u8; 8];
            bytes.copy_from_slice(data);
            json!(u64::from_le_bytes(bytes))
        }
        REG_MULTI_SZ => {
            let units = utf16(data);
            let mut items: Vec<Value> = units
                .split(|&unit| unit == 0)
                .map(|part| Value::String(String::from_utf16_lossy(part)))
                .collect();
            // El terminador doble deja dos elementos vacíos al final.
            while items.last().is_some_and(|item| item == "") {
                items.pop();
            }
            Value::Array(items)
        }
        _ => json!({ "hex": hex(data) }),
    }
}

/// Inversa de [`value_to_json`].
fn value_from_json(kind: u32, data: &Value) -> Option<Vec<u8>> {
    let wide = |text: &str| -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    if let Some(hex_text) = data.get("hex").and_then(Value::as_str) {
        return unhex(hex_text);
    }
    match (kind, data) {
        (REG_SZ | REG_EXPAND_SZ, Value::String(text)) => Some(wide(text)),
        (REG_DWORD, Value::Number(number)) => number
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(|n| n.to_le_bytes().to_vec()),
        (REG_QWORD, Value::Number(number)) => number.as_u64().map(|n| n.to_le_bytes().to_vec()),
        (REG_MULTI_SZ, Value::Array(items)) => {
            let mut bytes = Vec::new();
            for item in items {
                bytes.extend(wide(item.as_str()?));
            }
            bytes.extend([0, 0]);
            Some(bytes)
        }
        _ => None,
    }
}

/// Árbol de la clave `path` de HKCU como JSON, o `None` si no existe:
/// `{"values": [[nombre, tipo, datos], …], "keys": {hijo: árbol, …}}`.
///
/// # Errors
///
/// El error del sistema si no se puede leer.
pub fn snapshot(path: &str) -> Result<Option<Value>> {
    let Some(key) =
        Key::open(Hive::CurrentUser, path, false).map_err(|e| registry_error(path, e))?
    else {
        return Ok(None);
    };
    let mut values = Vec::new();
    for name in key.value_names().map_err(|e| registry_error(path, e))? {
        if let Some(raw) = key.get_raw(&name).map_err(|e| registry_error(path, e))? {
            values.push(json!([name, raw.kind, value_to_json(raw.kind, &raw.data)]));
        }
    }
    let mut keys = Map::new();
    for child in key.subkeys().map_err(|e| registry_error(path, e))? {
        if let Some(tree) = snapshot(&format!(r"{path}\{child}"))? {
            keys.insert(child, tree);
        }
    }
    Ok(Some(json!({ "values": values, "keys": keys })))
}

/// Escribe en HKCU el árbol producido por [`snapshot`], creando las claves que falten.
///
/// # Errors
///
/// El error del sistema, o un valor del árbol que no se puede convertir.
pub fn write_tree(path: &str, tree: &Value) -> Result<()> {
    let key = Key::create(Hive::CurrentUser, path).map_err(|e| registry_error(path, e))?;
    for value in tree
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parts = value.as_array().map(Vec::as_slice).unwrap_or_default();
        let (Some(name), Some(kind), Some(data)) = (
            parts.first().and_then(Value::as_str),
            parts
                .get(1)
                .and_then(Value::as_u64)
                .and_then(|k| u32::try_from(k).ok()),
            parts.get(2),
        ) else {
            return Err(Error::untranslated(format!(
                "Respaldo de protocolo no válido: {path}"
            )));
        };
        let bytes = value_from_json(kind, data).ok_or_else(|| {
            Error::untranslated(format!("Respaldo de protocolo no válido: {path}\\{name}"))
        })?;
        key.set_raw(name, kind, &bytes)
            .map_err(|e| registry_error(path, e))?;
    }
    drop(key);
    if let Some(children) = tree.get("keys").and_then(Value::as_object) {
        for (child, subtree) in children {
            write_tree(&format!(r"{path}\{child}"), subtree)?;
        }
    }
    Ok(())
}

/// Borra `HKCU\<clases>\<esquema>` entera.
///
/// # Errors
///
/// Los de [`scheme_name`] y el error del sistema.
pub fn delete_key(classes: &Classes, scheme: &str) -> Result<()> {
    delete_tree(&classes.user_path(&scheme_name(scheme)?))
}

// --- Estado actual -----------------------------------------------------------

/// Comando de `<hive>\<base>\<esquema>\shell\open\command`, `Some("")` si no es texto y
/// `None` si no hay clave, no tiene `URL Protocol` o no se puede leer.
pub fn read_command(hive: Hive, base: &str, scheme: &str) -> Option<String> {
    let path = format!(r"{base}\{scheme}");
    let key = Key::open(hive, &path, false).ok()??;
    key.get("URL Protocol").ok()??;
    drop(key);
    let key = Key::open(hive, &format!(r"{path}\shell\open\command"), false).ok()??;
    match key.get("").ok()?? {
        mito_win::registry::RegValue::String(text) => Some(text),
        _ => Some(String::new()),
    }
}

/// Comando de un esquema tal como lo resuelve Windows (HKCU antes que HKLM) y su origen.
pub fn current(classes: &Classes, scheme: &str) -> Option<(String, Hive)> {
    if let Some(value) = read_command(Hive::CurrentUser, classes.user(), scheme) {
        return Some((value, Hive::CurrentUser));
    }
    let base = classes.machine()?;
    read_command(Hive::LocalMachine, base, scheme).map(|value| (value, Hive::LocalMachine))
}

/// Primer token de un comando de shell, sin comillas; vacío si no se puede leer.
pub fn executable(command_line: &str) -> String {
    let text = command_line.trim_start();
    let token = if let Some(rest) = text.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => &text[..end + 2],
            None => return String::new(),
        }
    } else {
        text.split_whitespace().next().unwrap_or_default()
    };
    token.trim_matches('"').to_owned()
}

/// Comparación de rutas como `os.path.normcase(os.path.normpath(..))` en Windows.
fn normcase(text: &str) -> String {
    let mut value = text.replace('/', "\\").to_lowercase();
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value
}

/// ¿Está `path` dentro de `root`?
pub fn inside_root(root: &Path, path: &str) -> bool {
    let base = normcase(&root.to_string_lossy());
    let value = normcase(path);
    value == base
        || value
            .strip_prefix(&base)
            .is_some_and(|rest| rest.starts_with('\\') || base.ends_with('\\'))
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Estado del esquema y su comando actual (ver [`Status`]).
pub fn status(
    classes: &Classes,
    root: &Path,
    scheme: &str,
    profile: &str,
    installation: &str,
    records: &Map<String, Value>,
) -> (Status, Option<String>) {
    let Some((value, _)) = current(classes, scheme) else {
        return (Status::Absent, None);
    };
    if let Some(record) = records.get(scheme).and_then(ProtocolRecord::from_value)
        && record.profile == profile
        && record.installation == installation
        && value
            == command(
                root,
                &record.profile,
                &record.app,
                record.shortcut.as_deref(),
            )
    {
        return (Status::Mine, Some(value));
    }
    let program = executable(&value);
    if inside_root(root, &program) {
        let mito = normcase(&launcher(root).to_string_lossy());
        if normcase(&program) == mito && value.contains(" run ") {
            return (Status::Profile, Some(value));
        }
        return (Status::Direct, Some(value));
    }
    (Status::Other, Some(value))
}

/// Esquemas de HKCU|HKLM `base` (en minúsculas, sin extensiones ni nombres no válidos).
fn schemes(hive: Hive, base: &str) -> Vec<String> {
    let Ok(Some(key)) = Key::open(hive, base, false) else {
        return Vec::new();
    };
    key.subkeys()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .map(|name| name.to_lowercase())
        .filter(|name| valid_scheme(name))
        .collect()
}

/// Esquemas cuyo comando ejecuta un ejecutable con el mismo nombre de archivo que alguno
/// declarado por el componente. HKCU gana; HKLM solo aporta los no vistos.
///
/// # Errors
///
/// Los de `application_entries`.
pub fn candidates(
    classes: &Classes,
    engine: &Engine,
    installation: &Record,
) -> Result<Vec<String>> {
    let names: Vec<String> = engine
        .application_entries(installation)?
        .iter()
        .map(|entry| file_name(&entry.executable))
        .collect();
    let mut bases = vec![(Hive::CurrentUser, classes.user())];
    if let Some(machine) = classes.machine() {
        bases.push((Hive::LocalMachine, machine));
    }
    let mut found: Vec<String> = Vec::new();
    for (hive, base) in bases {
        for name in schemes(hive, base) {
            if found.contains(&name) || RESERVED.contains(&name.as_str()) || name.starts_with("ms-")
            {
                continue;
            }
            if let Some(value) = read_command(hive, base, &name)
                && !value.is_empty()
                && names.contains(&file_name(&executable(&value)))
            {
                found.push(name);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Fila del diálogo para un esquema.
///
/// # Errors
///
/// Los de [`scheme_name`] y de lectura del JSON.
pub fn describe(
    classes: &Classes,
    engine: &Engine,
    profile: &str,
    installation: &str,
    scheme: &str,
) -> Result<Row> {
    let scheme = scheme_name(scheme)?;
    let records = load(engine.root())?;
    let (state, value) = status(
        classes,
        engine.root(),
        &scheme,
        profile,
        installation,
        &records,
    );
    Ok(Row {
        scheme,
        status: state,
        command: value,
    })
}

/// Filas del diálogo: los candidatos del componente con su estado.
///
/// # Errors
///
/// `El componente no está instalado` y los de [`candidates`] y [`describe`].
pub fn rows(
    classes: &Classes,
    engine: &Engine,
    profile: &str,
    installation: &str,
) -> Result<Vec<Row>> {
    let installed = engine.installed()?;
    let lock = installed
        .get(installation)
        .ok_or_else(|| Error::domain(Message::new("El componente no está instalado")))?;
    candidates(classes, engine, lock)?
        .iter()
        .map(|scheme| describe(classes, engine, profile, installation, scheme))
        .collect()
}

// --- Reclamar y liberar ------------------------------------------------------

/// Reclamaciones hechas desde un diálogo abierto; [`Session::release`] las deshace todas y
/// al soltar la sesión se liberan las que queden.
pub struct Session<'a> {
    classes: Classes,
    engine: &'a Engine,
    profile: String,
    installation: String,
    log: Log<'a>,
    schemes: BTreeMap<String, Option<String>>,
}

impl<'a> Session<'a> {
    /// Sesión para un profile y un componente.
    pub fn new(
        classes: Classes,
        engine: &'a Engine,
        profile: &str,
        installation: &str,
        log: Log<'a>,
    ) -> Self {
        Self {
            classes,
            engine,
            profile: profile.to_owned(),
            installation: installation.to_owned(),
            log,
            schemes: BTreeMap::new(),
        }
    }

    /// Esquemas reclamados por esta sesión y su acceso directo.
    pub fn schemes(&self) -> &BTreeMap<String, Option<String>> {
        &self.schemes
    }

    /// Fila de un esquema.
    ///
    /// # Errors
    ///
    /// Los de [`describe`].
    pub fn describe(&self, scheme: &str) -> Result<Row> {
        describe(
            &self.classes,
            self.engine,
            &self.profile,
            &self.installation,
            scheme,
        )
    }

    /// Reclama el esquema y lo recuerda para vigilarlo y liberarlo.
    ///
    /// # Errors
    ///
    /// Los de [`claim`].
    pub fn claim(&mut self, scheme: &str, shortcut: Option<&str>) -> Result<Row> {
        let record = claim(
            &self.classes,
            self.engine,
            &self.profile,
            &self.installation,
            scheme,
            shortcut,
            self.log,
        )?;
        self.schemes
            .insert(record.scheme.clone(), shortcut.map(str::to_owned));
        self.describe(&record.scheme)
    }

    /// Vuelve a afirmar un esquema reclamado que otra aplicación sobrescribió. Devuelve
    /// `true` si lo recuperó y `false` si no está reclamado o sigue siendo de MITO.
    ///
    /// # Errors
    ///
    /// Los de [`claim`].
    pub fn check(&self, scheme: &str) -> Result<bool> {
        let Some(shortcut) = self.schemes.get(scheme) else {
            return Ok(false);
        };
        let records = load(self.engine.root())?;
        let (state, _) = status(
            &self.classes,
            self.engine.root(),
            scheme,
            &self.profile,
            &self.installation,
            &records,
        );
        if state == Status::Mine {
            return Ok(false);
        }
        claim(
            &self.classes,
            self.engine,
            &self.profile,
            &self.installation,
            scheme,
            shortcut.as_deref(),
            &|_| {},
        )?;
        Ok(true)
    }

    /// Libera un esquema (o todos los reclamados) y lo olvida; no toca los de otros
    /// profiles, componentes o sesiones.
    ///
    /// # Errors
    ///
    /// Los de lectura y escritura del JSON.
    pub fn release(&mut self, scheme: Option<&str>) -> Result<()> {
        let selected: Vec<String> = match scheme {
            Some(scheme) => vec![scheme.to_owned()],
            None => self.schemes.keys().cloned().collect(),
        };
        let (profile, installation) = (&self.profile, &self.installation);
        let result = unregister(
            &self.classes,
            self.engine.root(),
            &|record| {
                selected.contains(&record.scheme)
                    && &record.profile == profile
                    && &record.installation == installation
            },
            self.log,
        );
        for name in &selected {
            self.schemes.remove(name);
        }
        result.map(|_| ())
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if !self.schemes.is_empty() {
            let _ = self.release(None);
        }
    }
}

/// Escribe la clave del esquema con el lanzador de MITO. Si ya había un registro del
/// esquema se conserva su respaldo (el del dueño original del host); si no, se toma uno
/// nuevo de la clave actual, que es lo que restaura [`unregister`].
///
/// # Errors
///
/// `El componente debe estar activo en el profile`, los de [`scheme_name`] y
/// `application_target`, y los de registro y disco. Si la escritura falla se restaura el
/// respaldo.
pub fn claim(
    classes: &Classes,
    engine: &Engine,
    profile: &str,
    installation: &str,
    scheme: &str,
    shortcut: Option<&str>,
    log: Log<'_>,
) -> Result<ProtocolRecord> {
    let profile = identifier(profile)?;
    let scheme = scheme_name(scheme)?;
    let _lock: MutationLock = mutation(engine)?;
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
    let app = lock.key.clone();
    let mut records = load(engine.root())?;
    let path = classes.user_path(&scheme);
    let backup = match records.get(&scheme).and_then(ProtocolRecord::from_value) {
        // Todavía hay una reclamación de MITO: la copia es la de antes de MITO, aunque otra
        // aplicación (o la de otro profile) haya tomado la clave desde entonces.
        Some(previous) => previous.backup,
        // Ausente o ajena: es lo que restaurará la liberación.
        None => snapshot(&path)?,
    };
    let record = ProtocolRecord {
        scheme: scheme.clone(),
        profile: profile.clone(),
        installation: installation.to_owned(),
        app: app.clone(),
        shortcut: shortcut.map(str::to_owned),
        backup: backup.clone(),
    };
    let icon = icon_for(engine, lock, &profile, shortcut)?;
    let line = command(engine.root(), &profile, &app, shortcut);
    let written = (|| -> Result<()> {
        delete_tree(&path)?;
        write_tree(
            &path,
            &json!({
                "values": [["", REG_SZ, format!("URL:{scheme}")], ["URL Protocol", REG_SZ, ""]],
                "keys": {
                    "DefaultIcon": {"values": [["", REG_SZ, format!("{},0", icon.display())]]},
                    "shell": {"keys": {"open": {"keys": {"command": {
                        "values": [["", REG_SZ, line]]
                    }}}}}
                }
            }),
        )?;
        records.insert(scheme.clone(), record.to_value());
        store(engine.root(), &records)
    })();
    if let Err(error) = written {
        let _ = delete_tree(&path);
        if let Some(backup) = &backup {
            let _ = write_tree(&path, backup);
        }
        classes.notify();
        return Err(error);
    }
    classes.notify();
    log(LogLine::from(
        Message::new("Protocolo {p1} reclamado para {p3}.")
            .arg("p1", &scheme)
            .arg("p3", format!("{profile} - {app}")),
    ));
    Ok(record)
}

/// Restaura la clave guardada solo mientras siga ejecutando el comando de MITO; si otra
/// aplicación la retomó, no la toca.
///
/// # Errors
///
/// Los de [`scheme_name`] y de registro.
pub fn release_record(
    classes: &Classes,
    root: &Path,
    record: &ProtocolRecord,
    log: Log<'_>,
) -> Result<()> {
    let scheme = scheme_name(&record.scheme)?;
    let value = read_command(Hive::CurrentUser, classes.user(), &scheme);
    let mine = command(
        root,
        &record.profile,
        &record.app,
        record.shortcut.as_deref(),
    );
    if value.as_deref() == Some(mine.as_str()) {
        delete_key(classes, &scheme)?;
        if let Some(backup) = &record.backup {
            write_tree(&classes.user_path(&scheme), backup)?;
        }
        log(LogLine::from(
            Message::new("Protocolo {p1} liberado; registro anterior restaurado.")
                .arg("p1", &scheme),
        ));
    } else {
        log(LogLine::from(
            Message::new(
                "Protocolo {p1} liberado; ya apuntaba a otra aplicación y no se ha modificado.",
            )
            .arg("p1", &scheme),
        ));
    }
    Ok(())
}

/// Libera los registros para los que `select` es cierto y conserva el resto; un fallo por
/// registro se anota en `log` y el registro se conserva.
///
/// # Errors
///
/// Los de lectura y escritura del JSON.
pub fn unregister(
    classes: &Classes,
    root: &Path,
    select: &dyn Fn(&ProtocolRecord) -> bool,
    log: Log<'_>,
) -> Result<Vec<ProtocolRecord>> {
    let mut records = load(root)?;
    let mut removed = Vec::new();
    for (scheme, value) in records.clone() {
        let Some(record) = ProtocolRecord::from_value(&value) else {
            continue;
        };
        if !select(&record) {
            continue;
        }
        match release_record(classes, root, &record, log) {
            Ok(()) => {
                records.remove(&scheme);
                removed.push(record);
            }
            Err(error) => log(LogLine::from(
                Message::new("No se pudo liberar el protocolo {p1}: {p3}")
                    .arg("p1", &scheme)
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value,
    reason = "tests"
)]
mod tests {
    use super::*;
    use crate::open_with::testing::{Instance, Sandbox};

    const HOST: &str = r"C:\Users\me\AppData\Local\AnthropicClaude\app-1\claude.exe";

    fn quiet(_: LogLine) {}

    struct Fixture {
        sandbox: Sandbox,
        instance: Instance,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                sandbox: Sandbox::new(),
                instance: Instance::new("main/claude", "claude.exe", "Claude"),
            }
        }

        fn classes(&self) -> &Classes {
            &self.sandbox.classes
        }

        fn engine(&self) -> &Engine {
            &self.instance.engine
        }

        /// La clave que deja la app del host: texto, binario, DWORD y comando.
        fn host_key(&self, scheme: &str, program: &str) -> String {
            let path = self.classes().user_path(scheme);
            let key = Key::create(Hive::CurrentUser, &path).unwrap();
            key.set_string("", &format!("URL:{scheme}")).unwrap();
            key.set_string("URL Protocol", "").unwrap();
            key.set_raw("Blob", 3, &[0, 1]).unwrap();
            key.set_raw("Flags", 4, &7_u32.to_le_bytes()).unwrap();
            let command =
                Key::create(Hive::CurrentUser, &format!(r"{path}\shell\open\command")).unwrap();
            let line = format!(r#""{program}" -- "%1""#);
            command.set_string("", &line).unwrap();
            line
        }

        fn command(&self, scheme: &str) -> Option<String> {
            read_command(Hive::CurrentUser, self.classes().user(), scheme)
        }

        fn snapshot(&self, scheme: &str) -> Option<Value> {
            snapshot(&self.classes().user_path(scheme)).unwrap()
        }

        fn claim(&self, profile: &str, scheme: &str, shortcut: Option<&str>) -> ProtocolRecord {
            claim(
                self.classes(),
                self.engine(),
                profile,
                "main/claude",
                scheme,
                shortcut,
                &quiet,
            )
            .unwrap()
        }

        fn release_all(&self) {
            unregister(self.classes(), self.instance.root(), &|_| true, &quiet).unwrap();
        }

        fn status(&self, profile: &str) -> Status {
            describe(
                self.classes(),
                self.engine(),
                profile,
                "main/claude",
                "claude",
            )
            .unwrap()
            .status
        }
    }

    #[test]
    fn los_esquemas_se_validan_y_los_reservados_se_rechazan() {
        assert_eq!(scheme_name(" Claude:// ").unwrap(), "claude");
        assert_eq!(scheme_name("claude:").unwrap(), "claude");
        for text in [
            "",
            "x",
            "1abc",
            "a b",
            "http",
            "HTTPS",
            "file",
            "ms-settings",
            "ms-teams",
            "mito",
        ] {
            assert!(scheme_name(text).is_err(), "{text:?}");
        }
        assert_eq!(
            scheme_name("a b").unwrap_err().to_string(),
            "Esquema no válido: a b"
        );
        assert_eq!(
            scheme_name("HTTPS").unwrap_err().to_string(),
            "Esquema reservado del sistema: https"
        );
        assert!(scheme_name(&"a".repeat(32)).is_ok());
        assert!(scheme_name(&"a".repeat(33)).is_err());
    }

    #[test]
    fn los_candidatos_coinciden_con_el_nombre_del_ejecutable_declarado() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        fx.host_key("other", r"C:\Tools\other.exe");
        let installed = fx.engine().installed().unwrap();
        let lock = installed.get("main/claude").unwrap();
        assert_eq!(
            candidates(fx.classes(), fx.engine(), lock).unwrap(),
            ["claude"]
        );
    }

    #[test]
    fn reclamar_respalda_y_liberar_restaura_exactamente() {
        let fx = Fixture::new();
        let original = fx.host_key("claude", HOST);
        let before = fx.snapshot("claude");
        fx.claim("default", "claude", Some("Claude"));
        let line = fx.command("claude").unwrap();
        assert!(line.contains(&*launcher(fx.instance.root()).to_string_lossy()));
        assert!(line.contains("run --profile default --app main/claude --shortcut Claude"));
        assert!(line.ends_with(r#" -- "%1""#));
        assert_eq!(fx.status("default"), Status::Mine);
        assert_eq!(fx.status("work"), Status::Profile);
        fx.release_all();
        assert_eq!(fx.command("claude"), Some(original));
        assert_eq!(fx.snapshot("claude"), before);
        assert!(registry(fx.instance.root()).unwrap().is_empty());
        // El binario y el DWORD vuelven con su tipo.
        let key = Key::open(Hive::CurrentUser, &fx.classes().user_path("claude"), false)
            .unwrap()
            .unwrap();
        assert_eq!(key.get_raw("Blob").unwrap().unwrap().data, [0, 1]);
        assert_eq!(key.get_raw("Flags").unwrap().unwrap().kind, 4);
    }

    #[test]
    fn el_respaldo_en_disco_tiene_el_formato_del_original() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        fx.claim("default", "claude", Some("Claude"));
        let text = std::fs::read_to_string(registry_file(fx.instance.root())).unwrap();
        let file: Value = serde_json::from_str(&text).unwrap();
        let record = &file["claude"];
        assert_eq!(record["scheme"], "claude");
        assert_eq!(record["shortcut"], "Claude");
        let values = &record["backup"]["values"];
        assert_eq!(values[0], json!(["", 1, "URL:claude"]));
        assert_eq!(values[1], json!(["URL Protocol", 1, ""]));
        assert_eq!(values[2], json!(["Blob", 3, {"hex": "0001"}]));
        assert_eq!(values[3], json!(["Flags", 4, 7]));
        assert_eq!(
            record["backup"]["keys"]["shell"]["keys"]["open"]["keys"]["command"]["values"][0][2],
            format!(r#""{HOST}" -- "%1""#)
        );
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "scheme",
                "profile",
                "installation",
                "app",
                "shortcut",
                "backup"
            ]
        );
    }

    #[test]
    fn liberar_no_toca_una_clave_que_otra_aplicacion_retomo() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        fx.claim("default", "claude", None);
        let taken = fx.host_key("claude", r"C:\Host\app-2\claude.exe");
        assert_eq!(fx.status("default"), Status::Other);
        fx.release_all();
        assert_eq!(fx.command("claude"), Some(taken));
        assert!(registry(fx.instance.root()).unwrap().is_empty());
    }

    #[test]
    fn la_sesion_reafirma_lo_sobrescrito_y_restaura_el_primer_respaldo() {
        let fx = Fixture::new();
        let original = fx.host_key("claude", HOST);
        let mut session = Session::new(
            fx.classes().clone(),
            fx.engine(),
            "default",
            "main/claude",
            &quiet,
        );
        let row = session.claim("claude", Some("Claude")).unwrap();
        assert_eq!(row.status, Status::Mine);
        assert!(!session.check("claude").unwrap());
        // La copia de MITO se registra a sí misma directamente, como las apps Electron.
        let direct = fx
            .instance
            .root()
            .join("core")
            .join("apps")
            .join("main")
            .join("claude")
            .join("claude.exe");
        fx.host_key("claude", &direct.to_string_lossy());
        assert_eq!(fx.status("default"), Status::Direct);
        assert!(session.check("claude").unwrap());
        assert_eq!(fx.status("default"), Status::Mine);
        assert!(fx.command("claude").unwrap().contains("--shortcut Claude"));
        session.release(None).unwrap();
        assert_eq!(fx.command("claude"), Some(original));
        assert!(session.schemes().is_empty());
        assert!(registry(fx.instance.root()).unwrap().is_empty());
        assert!(!session.check("claude").unwrap());
    }

    #[test]
    fn la_sesion_no_toca_una_clave_tomada_tras_la_ultima_comprobacion() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        let mut session = Session::new(
            fx.classes().clone(),
            fx.engine(),
            "default",
            "main/claude",
            &quiet,
        );
        session.claim("claude", None).unwrap();
        let taken = fx.host_key("claude", r"C:\Host\app-2\claude.exe");
        session.release(None).unwrap();
        assert_eq!(fx.command("claude"), Some(taken));
    }

    #[test]
    fn soltar_la_sesion_libera_lo_que_quedaba() {
        let fx = Fixture::new();
        let original = fx.host_key("claude", HOST);
        {
            let mut session = Session::new(
                fx.classes().clone(),
                fx.engine(),
                "default",
                "main/claude",
                &quiet,
            );
            session.claim("claude", None).unwrap();
            assert_ne!(fx.command("claude"), Some(original.clone()));
        }
        assert_eq!(fx.command("claude"), Some(original));
    }

    #[test]
    fn un_esquema_ausente_se_borra_al_liberar() {
        let fx = Fixture::new();
        fx.claim("default", "claude", None);
        assert!(fx.command("claude").is_some());
        fx.release_all();
        assert_eq!(fx.snapshot("claude"), None);
    }

    #[test]
    fn un_ejecutable_directo_de_mito_se_distingue() {
        let fx = Fixture::new();
        let direct = fx
            .instance
            .root()
            .join("core")
            .join("apps")
            .join("main")
            .join("claude")
            .join("claude.exe");
        fx.host_key("claude", &direct.to_string_lossy());
        assert_eq!(fx.status("default"), Status::Direct);
    }

    #[test]
    fn otro_profile_toma_el_esquema_pero_conserva_el_respaldo_del_host() {
        let fx = Fixture::new();
        let original = fx.host_key("claude", HOST);
        fx.claim("default", "claude", None);
        fx.claim("work", "claude", None);
        assert_eq!(registry(fx.instance.root()).unwrap()[0].profile, "work");
        unregister(
            fx.classes(),
            fx.instance.root(),
            &|record| record.profile == "work",
            &quiet,
        )
        .unwrap();
        assert_eq!(fx.command("claude"), Some(original));
    }

    /// ADR 0013, punto 4: si otra aplicación toma la clave y se reclama de nuevo, el
    /// respaldo sigue siendo el del host original (el original guardaba el de la otra app).
    #[test]
    fn reclamar_de_nuevo_tras_una_sobrescritura_conserva_el_respaldo_del_host() {
        let fx = Fixture::new();
        let original = fx.host_key("claude", HOST);
        let before = fx.snapshot("claude");
        fx.claim("default", "claude", None);
        // Otra aplicación toma la clave (estado «otra app»).
        fx.host_key("claude", r"C:\Host\app-2\claude.exe");
        assert_eq!(fx.status("default"), Status::Other);
        // El usuario reclama de nuevo desde el diálogo, sin pasar por `check`.
        fx.claim("default", "claude", None);
        assert_eq!(fx.status("default"), Status::Mine);
        let backup = registry(fx.instance.root()).unwrap()[0].backup.clone();
        assert_eq!(backup, before);
        fx.release_all();
        assert_eq!(fx.command("claude"), Some(original));
        assert_eq!(fx.snapshot("claude"), before);
    }

    #[test]
    fn un_componente_inactivo_se_rechaza() {
        let fx = Fixture::new();
        let error = claim(
            fx.classes(),
            fx.engine(),
            "default",
            "main/other",
            "claude",
            None,
            &quiet,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "El componente debe estar activo en el profile"
        );
        assert!(registry(fx.instance.root()).unwrap().is_empty());
        assert_eq!(fx.snapshot("claude"), None);
    }

    #[test]
    fn las_filas_listan_los_candidatos_con_su_estado() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        let rows = rows(fx.classes(), fx.engine(), "default", "main/claude").unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.scheme.as_str(), row.status))
                .collect::<Vec<_>>(),
            [("claude", Status::Other)]
        );
        assert_eq!(rows[0].command, Some(format!(r#""{HOST}" -- "%1""#)));
        assert_eq!(
            super::rows(fx.classes(), fx.engine(), "default", "main/x")
                .unwrap_err()
                .to_string(),
            "El componente no está instalado"
        );
    }

    #[test]
    fn los_mensajes_del_registro_son_los_del_contrato() {
        let fx = Fixture::new();
        fx.host_key("claude", HOST);
        let lines = std::sync::Mutex::new(Vec::new());
        let log = |line: LogLine| lines.lock().unwrap().push(line.message.to_string());
        claim(
            fx.classes(),
            fx.engine(),
            "default",
            "main/claude",
            "claude",
            None,
            &log,
        )
        .unwrap();
        unregister(fx.classes(), fx.instance.root(), &|_| true, &log).unwrap();
        // Liberar una clave ajena.
        claim(
            fx.classes(),
            fx.engine(),
            "default",
            "main/claude",
            "claude",
            None,
            &quiet,
        )
        .unwrap();
        fx.host_key("claude", r"C:\Host\x\claude.exe");
        unregister(fx.classes(), fx.instance.root(), &|_| true, &log).unwrap();
        assert_eq!(
            *lines.lock().unwrap(),
            [
                "Protocolo claude reclamado para default - main/claude.",
                "Protocolo claude liberado; registro anterior restaurado.",
                "Protocolo claude liberado; ya apuntaba a otra aplicación y no se ha modificado.",
            ]
        );
    }

    #[test]
    fn el_ejecutable_es_el_primer_token_sin_comillas() {
        assert_eq!(
            executable(r#""C:\Mi App\a.exe" -- "%1""#),
            r"C:\Mi App\a.exe"
        );
        assert_eq!(executable(r"C:\a\b.exe --x"), r"C:\a\b.exe");
        assert_eq!(executable(r#""C:\sin cierre"#), "");
        assert_eq!(executable(""), "");
    }

    #[test]
    fn dentro_de_la_raiz_compara_por_componentes() {
        let root = Path::new(r"C:\Mito");
        assert!(inside_root(root, r"c:\mito\core\x.exe"));
        assert!(inside_root(root, r"C:/Mito/core/x.exe"));
        assert!(!inside_root(root, r"C:\Mito2\x.exe"));
        assert!(!inside_root(root, ""));
    }

    #[test]
    fn el_tipo_de_los_valores_viaja_de_ida_y_vuelta() {
        let cases: [(u32, Vec<u8>); 6] = [
            (1, "ñ\0".encode_utf16().flat_map(u16::to_le_bytes).collect()),
            (
                2,
                "%PATH%\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
            (3, vec![0, 1, 255]),
            (4, 9_u32.to_le_bytes().to_vec()),
            (11, 9_u64.to_le_bytes().to_vec()),
            (
                7,
                "a\0b\0\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
        ];
        for (kind, bytes) in cases {
            let value = value_to_json(kind, &bytes);
            assert_eq!(value_from_json(kind, &value), Some(bytes), "{kind}");
        }
        assert_eq!(value_to_json(7, &[]), json!([]));
    }

    #[test]
    fn nunca_se_toca_hkcu_software_classes_en_los_tests() {
        let fx = Fixture::new();
        assert!(fx.classes().user().starts_with(r"Software\MITO-tests\"));
        assert_eq!(fx.classes().machine(), None);
    }
}
