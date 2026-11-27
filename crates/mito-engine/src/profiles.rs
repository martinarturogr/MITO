//! Profiles: `profiles/<name>.json`, variables de entorno validadas y borrado seguro.
//!
//! Formato (ADR 0003): `{"workspace": "<w>", "apps": ["bucket/app", …], "env": {…}}`
//! con los campos en ese orden. Los campos desconocidos se conservan al guardar.

use std::fs;
use std::path::PathBuf;

use mito_core::storage::{identifier, inside, read_json, write_json};
use mito_core::{Layout, Message};
use serde_json::{Map, Value};

use crate::engine::Engine;
use crate::error::{Error, Result, bail};

/// Variables que el motor define y un profile no puede sobrescribir (en mayúsculas).
pub const RESERVED_VARIABLES: [&str; 9] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "HOMEDRIVE",
    "HOMEPATH",
];

/// Contenido de un `profiles/<name>.json`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    /// Carpeta bajo `workspaces/` (ruta portable).
    pub workspace: String,
    /// Ids de instalación activos, sin repetidos; su orden es el orden de activación.
    pub apps: Vec<String>,
    /// Variables del profile, en el orden del archivo.
    pub env: Vec<(String, String)>,
    /// Campos no reconocidos, que se conservan al guardar.
    pub extra: Map<String, Value>,
}

impl Profile {
    /// Profile vacío con ese workspace (`{workspace, apps: [], env: {}}`).
    pub fn new(workspace: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            ..Self::default()
        }
    }

    /// JSON del profile, con los campos en el orden del original.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("workspace".into(), Value::String(self.workspace.clone()));
        map.insert(
            "apps".into(),
            Value::Array(self.apps.iter().cloned().map(Value::String).collect()),
        );
        map.insert(
            "env".into(),
            Value::Object(
                self.env
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ),
        );
        for (key, value) in &self.extra {
            map.insert(key.clone(), value.clone());
        }
        Value::Object(map)
    }

    /// Valida y convierte el JSON de un profile (`Engine.profile`).
    ///
    /// # Errors
    ///
    /// `apps debe ser una lista de identificadores de instalación`, `apps contiene
    /// instalaciones repetidas`, los de [`validate_env`] y los de `inside` para el
    /// workspace; `El profile no define workspace` si falta.
    pub fn from_value(value: &Value) -> Result<Self> {
        let object = value.as_object();
        let apps: Option<Vec<String>> = object
            .and_then(|o| o.get("apps"))
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect()
            });
        let Some(apps) = apps else {
            return bail(Message::new(
                "apps debe ser una lista de identificadores de instalación",
            ));
        };
        let unique: std::collections::HashSet<&String> = apps.iter().collect();
        if unique.len() != apps.len() {
            return bail(Message::new("apps contiene instalaciones repetidas"));
        }
        let empty = Value::Object(Map::new());
        let env = validate_env(object.and_then(|o| o.get("env")).unwrap_or(&empty))?;
        let workspace = object
            .and_then(|o| o.get("workspace"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let Some(workspace) = workspace else {
            return bail(Message::new("El profile no define workspace"));
        };
        let extra = object
            .into_iter()
            .flatten()
            .filter(|(key, _)| !matches!(key.as_str(), "workspace" | "apps" | "env"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(Self {
            workspace,
            apps,
            env,
            extra,
        })
    }
}

/// Valida las variables de entorno de un profile o de la configuración
/// (`Engine.validate_env`) y las devuelve en su orden.
///
/// # Errors
///
/// `env debe ser un objeto de variables de entorno`, `Variable de entorno no válida: …`
/// (clave vacía o con `=`, `;` o NUL, valor que no es texto o con NUL), `Variable
/// reservada por MITO: …` y `Variable de entorno repetida: …` (sin distinguir
/// mayúsculas). Se informa el primer problema en el orden del objeto.
pub fn validate_env(overrides: &Value) -> Result<Vec<(String, String)>> {
    let Some(object) = overrides.as_object() else {
        return bail(Message::new(
            "env debe ser un objeto de variables de entorno",
        ));
    };
    let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(object.len());
    for (key, value) in object {
        let Some(text) = value.as_str() else {
            // Los problemas de las claves anteriores tienen prioridad.
            mito_core::config::validate_env(pairs.iter().copied())?;
            return bail(Message::new("Variable de entorno no válida: ").with_suffix(key));
        };
        pairs.push((key, text));
    }
    mito_core::config::validate_env(pairs.iter().copied())?;
    Ok(pairs
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

/// Variables `env.*` de `config.properties` (`Engine.global_env`), validadas.
///
/// Solo cuentan las líneas que, recortadas, empiezan por `env.`. La clave es el texto
/// hasta el primer `=` (recortado) y el valor el resto (recortado).
///
/// # Errors
///
/// `Variable global sin =: …`, `Variable de entorno repetida: …` y los de
/// [`validate_env`].
pub fn global_env(layout: &Layout) -> Result<Vec<(String, String)>> {
    let path = layout.config_file();
    let text = match fs::read(&path) {
        Ok(bytes) => {
            let bytes = bytes
                .strip_prefix(b"\xEF\xBB\xBF")
                .unwrap_or(&bytes)
                .to_vec();
            String::from_utf8(bytes).map_err(|e| {
                Error::io(
                    &path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                )
            })?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let mut values: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("env.") else {
            continue;
        };
        let Some((key, value)) = rest.split_once('=') else {
            return bail(Message::new("Variable global sin =: ").with_suffix(rest.trim()));
        };
        let key = key.trim();
        if values.iter().any(|(existing, _)| existing == key) {
            return bail(Message::new("Variable de entorno repetida: ").with_suffix(key));
        }
        values.push((key.to_owned(), value.trim().to_owned()));
    }
    let object: Map<String, Value> = values
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    validate_env(&Value::Object(object))
}

/// Ruta de `profiles/<name>.json` con el nombre normalizado.
fn profile_path(layout: &Layout, name: &str) -> Result<PathBuf> {
    Ok(layout
        .profiles()
        .join(format!("{}.json", identifier(name)?)))
}

/// Nombres de los profiles, ordenados (`Engine.profiles`).
pub fn list(layout: &Layout) -> Vec<String> {
    let Ok(entries) = fs::read_dir(layout.profiles()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_json = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            (is_json && path.is_file())
                .then(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

/// Lee y valida `profiles/<name>.json` (`Engine.profile`).
///
/// # Errors
///
/// `No existe el profile {p1}` y los de [`Profile::from_value`].
pub fn load(layout: &Layout, name: &str) -> Result<Profile> {
    let path = profile_path(layout, name)?;
    if !path.exists() {
        return bail(Message::new("No existe el profile {p1}").arg("p1", name));
    }
    let value: Value = read_json(&path)?;
    let profile = Profile::from_value(&value)?;
    inside(&layout.workspaces(), &profile.workspace)?;
    Ok(profile)
}

/// Escribe `profiles/<name>.json` de forma atómica (`Engine.save_profile`).
///
/// # Errors
///
/// Los de `identifier` y los de escritura.
pub fn save(layout: &Layout, name: &str, profile: &Profile) -> Result<()> {
    let path = profile_path(layout, name)?;
    write_json(&path, &profile.to_value())?;
    Ok(())
}

impl Engine {
    /// Nombres de los profiles, ordenados.
    pub fn profiles(&self) -> Vec<String> {
        list(self.layout())
    }

    /// Lee y valida un profile.
    ///
    /// # Errors
    ///
    /// Ver [`load`].
    pub fn profile(&self, name: &str) -> Result<Profile> {
        load(self.layout(), name)
    }

    /// Variables `env.*` de `config.properties`.
    ///
    /// # Errors
    ///
    /// Ver [`global_env`].
    pub fn global_env(&self) -> Result<Vec<(String, String)>> {
        global_env(self.layout())
    }

    /// Guarda un profile. Es el único punto de escritura de profiles del motor.
    ///
    /// # Errors
    ///
    /// Ver [`save`].
    pub fn save_profile(&self, name: &str, profile: &Profile) -> Result<()> {
        self.fault("save-profile", name)?;
        save(self.layout(), name, profile)
    }

    /// Crea un profile vacío y devuelve su nombre normalizado. El workspace por defecto
    /// es el propio nombre; puede compartirse entre profiles.
    ///
    /// # Errors
    ///
    /// `El profile {p1} ya existe`, nombres no válidos y errores de escritura.
    pub fn create_profile(&self, name: &str, workspace: Option<&str>) -> Result<String> {
        let name = identifier(name)?;
        let workspace = identifier(workspace.filter(|w| !w.is_empty()).unwrap_or(&name))?;
        let _lock = self.mutation()?;
        if self.profiles().contains(&name) {
            return bail(Message::new("El profile {p1} ya existe").arg("p1", &name));
        }
        self.initialize()?;
        let folder = self.layout().workspaces().join(&workspace);
        match fs::create_dir(&folder) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && folder.is_dir() => {}
            Err(e) => return Err(Error::io(&folder, e)),
        }
        self.save_profile(&name, &Profile::new(workspace))?;
        self.extensions().terminal.prepare(self)?;
        Ok(name)
    }

    /// Crea `default` si la instancia no tiene ningún profile (el mismo que incluye el
    /// ZIP de release). Devuelve `None` si ya había alguno.
    ///
    /// # Errors
    ///
    /// Errores de [`Self::create_profile`], salvo que otro proceso lo haya creado antes.
    pub fn ensure_default_profile(&self) -> Result<Option<String>> {
        if !self.profiles().is_empty() {
            return Ok(None);
        }
        match self.create_profile("default", None) {
            Ok(name) => Ok(Some(name)),
            // Otro proceso lo creó entre la comprobación y el lock.
            Err(_) if !self.profiles().is_empty() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Borra un profile y devuelve el primero que queda (orden alfabético).
    ///
    /// Se guarda una copia en `core/state/deleted-profiles/<name>.json`. No se borran
    /// `home/`, el workspace ni las copias y datos por profile de los componentes.
    ///
    /// # Errors
    ///
    /// `No se puede eliminar el último profile` y los de [`Self::profile`].
    pub fn delete_profile(&self, name: &str) -> Result<String> {
        let name = identifier(name)?;
        let _lock = self.mutation()?;
        let data = self.profile(&name)?;
        if self.profiles().len() <= 1 {
            return bail(Message::new("No se puede eliminar el último profile"));
        }
        let backup = self
            .layout()
            .state()
            .join("deleted-profiles")
            .join(format!("{name}.json"));
        write_json(&backup, &data.to_value())?;
        let path = inside(&self.layout().profiles(), &format!("{name}.json"))?;
        fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
        // «Abrir con» abriría un profile que ya no existe.
        self.extensions().host.unregister_profile(self, &name)?;
        self.extensions().terminal.prepare(self)?;
        Ok(self.profiles().into_iter().next().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_formato_conserva_el_orden_y_los_campos_extra() {
        let value =
            json!({"workspace": "w", "apps": ["a/b"], "env": {"Z": "1", "A": "2"}, "ui": 3});
        let profile = Profile::from_value(&value).unwrap();
        assert_eq!(profile.env[0].0, "Z");
        let text = serde_json::to_string(&profile.to_value()).unwrap();
        assert_eq!(
            text,
            r#"{"workspace":"w","apps":["a/b"],"env":{"Z":"1","A":"2"},"ui":3}"#
        );
    }

    #[test]
    fn env_se_valida_en_orden() {
        let error = |value: Value| validate_env(&value).unwrap_err().to_string();
        assert_eq!(
            error(json!({"HOME": "x"})),
            "Variable reservada por MITO: HOME"
        );
        assert_eq!(
            error(json!({"Path": "x"})),
            "Variable reservada por MITO: Path"
        );
        assert_eq!(
            error(json!({"mito_root": "x"})),
            "Variable reservada por MITO: mito_root"
        );
        assert_eq!(
            error(json!({"A": "1", "a": "2"})),
            "Variable de entorno repetida: a"
        );
        assert_eq!(error(json!({"A": 1})), "Variable de entorno no válida: A");
        assert_eq!(
            error(json!({"BAD=NAME": "x"})),
            "Variable de entorno no válida: BAD=NAME"
        );
        assert_eq!(
            error(json!({"HOME": "x", "B": 1})),
            "Variable reservada por MITO: HOME"
        );
        assert_eq!(
            error(json!([1])),
            "env debe ser un objeto de variables de entorno"
        );
        assert!(validate_env(&json!({"OK": ""})).is_ok());
    }

    #[test]
    fn apps_se_valida() {
        let error = |value: Value| Profile::from_value(&value).unwrap_err().to_string();
        assert_eq!(
            error(json!({"workspace": "w", "apps": "x"})),
            "apps debe ser una lista de identificadores de instalación"
        );
        assert_eq!(
            error(json!({"workspace": "w", "apps": ["a", "a"]})),
            "apps contiene instalaciones repetidas"
        );
        assert_eq!(error(json!({"apps": []})), "El profile no define workspace");
    }
}
