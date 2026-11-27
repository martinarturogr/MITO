//! Registro de un componente instalado y su marcador `.mito.json`.
//!
//! Formato (ADR 0003, `docs/spec/modules/engine.md` §2):
//!
//! ```json
//! { "key": "…", "manifest_sha256": "…", "record": { "key": "…", "version": "…",
//!   "manifest_sha256": "…", "path": "core/apps/<bucket>/<app>", "manifest": {…},
//!   "pinned": {…} } }
//! ```

use mito_catalog::Manifest;
use mito_core::Message;
use serde_json::{Map, Value, json};

use crate::error::{Error, Result, bail};

/// Nombre del marcador de componente dentro de su carpeta.
pub const MARKER: &str = ".mito.json";

/// Registro de una instalación (`record` del marcador y `lock` del original, que es el
/// registro más su `path`).
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Clave `bucket/app`.
    pub key: String,
    /// `version` del manifiesto, tal cual (texto o número).
    pub version: Value,
    /// Huella del manifiesto ([`mito_catalog::fingerprint`]).
    pub manifest_sha256: String,
    /// Ruta relativa a la raíz, con `/` (`core/apps/<bucket>/<app>`).
    pub path: String,
    /// Manifiesto original del bucket (sin `effective`).
    pub manifest: Value,
    /// Retención de versión (`install_version`), si la hay.
    pub pinned: Option<Value>,
}

impl Record {
    /// `str(version)`.
    pub fn version_text(&self) -> String {
        mito_catalog::pyjson::py_str(&self.version)
    }

    /// Manifiesto efectivo (`architecture.64bit` fusionado) como objeto.
    ///
    /// # Errors
    ///
    /// Los de [`Manifest::effective`].
    pub fn effective(&self) -> Result<Map<String, Value>> {
        effective(&self.manifest)
    }

    /// `record` tal como se escribe en el marcador, en su orden.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("key".into(), json!(self.key));
        map.insert("version".into(), self.version.clone());
        map.insert("manifest_sha256".into(), json!(self.manifest_sha256));
        map.insert("path".into(), json!(self.path));
        map.insert("manifest".into(), self.manifest.clone());
        if let Some(pinned) = &self.pinned {
            map.insert("pinned".into(), pinned.clone());
        }
        Value::Object(map)
    }

    /// Contenido completo de `.mito.json`.
    pub fn marker(&self) -> Value {
        json!({
            "key": self.key,
            "manifest_sha256": self.manifest_sha256,
            "record": self.to_value(),
        })
    }

    /// Lee un `record` de un marcador.
    ///
    /// # Errors
    ///
    /// `Identidad o ruta de componente incoherente` si faltan campos obligatorios.
    pub fn from_value(value: &Value) -> Result<Self> {
        let incoherent =
            || Error::domain(Message::new("Identidad o ruta de componente incoherente"));
        let object = value.as_object().ok_or_else(incoherent)?;
        let text = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(incoherent)
        };
        Ok(Self {
            key: text("key")?,
            version: object.get("version").cloned().ok_or_else(incoherent)?,
            manifest_sha256: text("manifest_sha256")?,
            path: text("path")?,
            manifest: object.get("manifest").cloned().ok_or_else(incoherent)?,
            pinned: object.get("pinned").filter(|p| !p.is_null()).cloned(),
        })
    }
}

/// Manifiesto efectivo como objeto.
///
/// # Errors
///
/// Los de [`Manifest::effective`].
pub fn effective(manifest: &Value) -> Result<Map<String, Value>> {
    match Manifest::new(manifest.clone()).effective()?.into_raw() {
        Value::Object(map) => Ok(map),
        _ => bail(Message::new("El manifiesto debe ser un objeto JSON")),
    }
}

/// Colección ordenada de registros por id de instalación (un `dict` de Python).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Records(Vec<(String, Record)>);

impl Records {
    /// Sin registros.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade o reemplaza (conserva la posición si ya existía).
    pub fn insert(&mut self, id: String, record: Record) {
        match self.0.iter_mut().find(|(existing, _)| *existing == id) {
            Some(slot) => slot.1 = record,
            None => self.0.push((id, record)),
        }
    }

    /// Registro del id `id`.
    pub fn get(&self, id: &str) -> Option<&Record> {
        self.0.iter().find(|(k, _)| k == id).map(|(_, r)| r)
    }

    /// ¿Existe el id?
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Pares `(id, registro)` en orden.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Record)> {
        self.0.iter().map(|(k, r)| (k.as_str(), r))
    }

    /// Ids en orden.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    /// Número de registros.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// ¿No hay ninguno?
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> IntoIterator for &'a Records {
    type Item = (&'a str, &'a Record);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a Record)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl FromIterator<(String, Record)> for Records {
    fn from_iter<T: IntoIterator<Item = (String, Record)>>(iter: T) -> Self {
        let mut records = Self::new();
        for (id, record) in iter {
            records.insert(id, record);
        }
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Record {
        Record {
            key: "test/tool".into(),
            version: json!("1.0"),
            manifest_sha256: "abc".into(),
            path: "core/apps/test/tool".into(),
            manifest: json!({"version": "1.0", "bin": [["bin/tool.exe", "tool"]]}),
            pinned: None,
        }
    }

    #[test]
    fn el_marcador_conserva_el_orden_del_original() {
        let marker = sample().marker();
        let keys: Vec<&String> = marker.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["key", "manifest_sha256", "record"]);
        let record = marker["record"].as_object().unwrap();
        let keys: Vec<&String> = record.keys().collect();
        assert_eq!(
            keys,
            ["key", "version", "manifest_sha256", "path", "manifest"]
        );
        let mut pinned = sample();
        pinned.pinned = Some(json!({"version": "1.5"}));
        assert!(
            pinned
                .to_value()
                .as_object()
                .unwrap()
                .contains_key("pinned")
        );
    }

    #[test]
    fn ida_y_vuelta() {
        let record = sample();
        assert_eq!(Record::from_value(&record.to_value()).unwrap(), record);
        assert!(Record::from_value(&json!({"key": "x"})).is_err());
    }
}
