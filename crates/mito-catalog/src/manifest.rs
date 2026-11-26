//! Manifiestos Scoop: JSON original, vista tipada tolerante y huella.
//!
//! Contrato: `docs/spec/modules/catalog.md`. El manifiesto se conserva como
//! [`serde_json::Value`] (con el orden original) porque los snapshots y `.mito.json`
//! guardan el JSON completo y la huella se calcula sobre él; [`ManifestView`] es la
//! lectura tipada y tolerante de los campos que usa MITO.

use mito_core::Message;
use mito_core::storage::portable_path;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::error::{Error, Result, bail};
use crate::pyjson::{DumpOptions, dumps, py_str, sha256_hex};

/// Texto o lista de textos (`array()` del original): `null` es una lista vacía y un
/// valor suelto es una lista de un elemento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr<T> {
            Null(()),
            Many(Vec<T>),
            One(T),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Null(()) => Self(Vec::new()),
            Repr::Many(items) => Self(items),
            Repr::One(item) => Self(vec![item]),
        })
    }
}

/// `array(value)` del original sobre valores JSON: `[]` si falta o es `null`, la misma
/// lista si ya lo es y `[value]` en otro caso.
pub fn array(value: Option<&Value>) -> Vec<&Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
    }
}

/// Manifiesto Scoop tal como viene del snapshot.
///
/// Puede no ser un objeto JSON si el snapshot está dañado; las operaciones lo tratan
/// como no soportado en lugar de fallar.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    raw: Value,
}

impl Manifest {
    /// Envuelve el JSON original.
    pub fn new(raw: Value) -> Self {
        Self { raw }
    }

    /// JSON original, sin normalizar.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// Consume el manifiesto y devuelve el JSON original.
    pub fn into_raw(self) -> Value {
        self.raw
    }

    /// El manifiesto como objeto, si lo es.
    pub fn as_object(&self) -> Option<&Map<String, Value>> {
        self.raw.as_object()
    }

    /// Huella del manifiesto (`fingerprint` del original): SHA-256 hexadecimal de
    /// `json.dumps(manifest, sort_keys=True, separators=(',', ':'))`, con escape ASCII.
    /// Se calcula sobre el manifiesto original, **no** sobre [`Self::effective`].
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.raw)
    }

    /// Manifiesto con `architecture.64bit` fusionado sobre la raíz (`effective`).
    ///
    /// # Errors
    ///
    /// `No hay versión Windows x64` si `architecture` no es un objeto con un objeto
    /// `64bit`; el original lanza `TypeError`/`ValueError` según el caso. Un manifiesto
    /// que no es un objeto produce `El manifiesto debe ser un objeto JSON`.
    pub fn effective(&self) -> Result<Self> {
        let Some(map) = self.raw.as_object() else {
            return bail(Message::new("El manifiesto debe ser un objeto JSON"));
        };
        let mut result = map.clone();
        if let Some(architecture) = map.get("architecture") {
            let Some(overrides) = architecture.get("64bit").and_then(Value::as_object) else {
                return bail(Message::new("No hay versión Windows x64"));
            };
            for (key, value) in overrides {
                result.insert(key.clone(), value.clone());
            }
        }
        Ok(Self::new(Value::Object(result)))
    }

    /// Lectura tipada y tolerante del manifiesto efectivo.
    ///
    /// # Errors
    ///
    /// Los de [`Self::effective`].
    pub fn view(&self) -> Result<ManifestView> {
        let effective = self.effective()?;
        Ok(serde_json::from_value(effective.raw).unwrap_or_default())
    }
}

impl From<Value> for Manifest {
    fn from(raw: Value) -> Self {
        Self::new(raw)
    }
}

/// Huella de un JSON de manifiesto (ver [`Manifest::fingerprint`]).
pub fn fingerprint(manifest: &Value) -> String {
    sha256_hex(dumps(manifest, DumpOptions::COMPACT_SORTED).as_bytes())
}

/// Convierte un valor con el tipo pedido o usa el valor por defecto si no encaja.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// `str(version)` del original; ausente o `null` es `None`.
fn lenient_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok((!value.is_null()).then(|| py_str(&value)))
}

/// Entrada de `bin`: ruta suelta o `[ejecutable, alias, argumentos?]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum BinEntry {
    /// Solo el ejecutable; el alias es su nombre sin extensión.
    Path(String),
    /// `[ejecutable, alias, argumentos?]`.
    Parts(Vec<String>),
}

/// Entrada de `persist`: ruta única o `[origen, destino]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PersistEntry {
    /// Origen y destino iguales.
    Path(String),
    /// `[origen, destino]`.
    Pair((String, String)),
}

impl PersistEntry {
    /// Origen y destino de la persistencia.
    pub fn source_and_destination(&self) -> (&str, &str) {
        match self {
            Self::Path(path) => (path, path),
            Self::Pair((source, destination)) => (source, destination),
        }
    }
}

/// `installer` de un manifiesto: script en línea o archivo `.ps1` con argumentos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InstallerView {
    /// Líneas del script en línea.
    #[serde(default, deserialize_with = "lenient")]
    pub script: OneOrMany<String>,
    /// Archivo `.ps1` relativo al payload.
    #[serde(default, deserialize_with = "lenient")]
    pub file: Option<String>,
    /// Argumentos del archivo.
    #[serde(default, deserialize_with = "lenient")]
    pub args: OneOrMany<String>,
}

/// Lectura tipada y tolerante de los campos de un manifiesto que usa MITO.
///
/// Los campos con un tipo inesperado quedan vacíos: la decisión de rechazarlos es de
/// [`crate::compat::compatibility`], que trabaja sobre el JSON original.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ManifestView {
    /// `str(version)`.
    #[serde(default, deserialize_with = "lenient_version")]
    pub version: Option<String>,
    /// Descripción.
    #[serde(default, deserialize_with = "lenient")]
    pub description: Option<String>,
    /// Notas (texto o lista de textos).
    #[serde(default, deserialize_with = "lenient")]
    pub notes: OneOrMany<String>,
    /// URL de descarga.
    #[serde(default, deserialize_with = "lenient")]
    pub url: OneOrMany<String>,
    /// Hashes declarados, uno por URL.
    #[serde(default, deserialize_with = "lenient")]
    pub hash: OneOrMany<Option<String>>,
    /// Comandos expuestos.
    #[serde(default, deserialize_with = "lenient")]
    pub bin: OneOrMany<BinEntry>,
    /// Accesos directos `[ejecutable, etiqueta, argumentos, icono?]`.
    #[serde(default, deserialize_with = "lenient")]
    pub shortcuts: Vec<Vec<String>>,
    /// Variables de entorno.
    #[serde(default, deserialize_with = "lenient")]
    pub env_set: Map<String, Value>,
    /// Rutas añadidas al `PATH`.
    #[serde(default, deserialize_with = "lenient")]
    pub env_add_path: OneOrMany<String>,
    /// Rutas persistentes.
    #[serde(default, deserialize_with = "lenient")]
    pub persist: OneOrMany<PersistEntry>,
    /// Dependencias `bucket/app`.
    #[serde(default, deserialize_with = "lenient")]
    pub depends: OneOrMany<String>,
    /// Carpeta del archivo que se extrae.
    #[serde(default, deserialize_with = "lenient")]
    pub extract_dir: OneOrMany<String>,
    /// Destino de la extracción.
    #[serde(default, deserialize_with = "lenient")]
    pub extract_to: OneOrMany<String>,
    /// Instalador Inno Setup.
    #[serde(default, deserialize_with = "lenient")]
    pub innosetup: Option<bool>,
    /// Hook previo a la instalación.
    #[serde(default, deserialize_with = "lenient")]
    pub pre_install: OneOrMany<String>,
    /// Instalador.
    #[serde(default, deserialize_with = "lenient")]
    pub installer: Option<InstallerView>,
}

/// Validación de `installer` (`installer_definition` del original).
///
/// Devuelve `None` si no hay instalador (ausente, `null` o `{}`) y el propio objeto
/// `installer` si es válido.
///
/// # Errors
///
/// `Pasos no soportados: …` o `installer.args/script debe ser texto o una lista de
/// líneas`, además de los errores de `portable_path` para `installer.file`.
pub fn installer_definition(manifest: &Map<String, Value>) -> Result<Option<&Map<String, Value>>> {
    let unsupported = |what: &str| bail(Message::new("Pasos no soportados: {p1}").arg("p1", what));
    let installer = match manifest.get("installer") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(map)) if map.is_empty() => return Ok(None),
        Some(Value::Object(map)) => map,
        Some(_) => return unsupported("installer"),
    };
    let has_file = installer.contains_key("file");
    let mut extra: Vec<&String> = installer
        .keys()
        .filter(|key| {
            if has_file {
                !matches!(key.as_str(), "file" | "args")
            } else {
                key.as_str() != "script"
            }
        })
        .collect();
    if !extra.is_empty() {
        extra.sort();
        let names: Vec<String> = extra
            .iter()
            .map(|name| format!("installer.{name}"))
            .collect();
        return unsupported(&names.join(", "));
    }
    let lines_hint = |hook: &str| {
        bail(Message::new("{hook} debe ser texto o una lista de líneas").arg("hook", hook))
    };
    if has_file {
        let file = installer.get("file").and_then(Value::as_str);
        let Some(file) = file.filter(|f| crate::pypath::suffix(f).eq_ignore_ascii_case(".ps1"))
        else {
            return bail(Message::new("Pasos no soportados: {p1}").arg("p1", "installer.file"));
        };
        portable_path(file).map_err(Error::from)?;
        return match installer.get("args") {
            None | Some(Value::Null) => Ok(Some(installer)),
            Some(args) if is_text_or_lines(args) => Ok(Some(installer)),
            Some(_) => lines_hint("installer.args"),
        };
    }
    match installer.get("script") {
        Some(script) if is_text_or_lines(script) => Ok(Some(installer)),
        _ => lines_hint("installer.script"),
    }
}

/// ¿Es texto o una lista cuyos elementos son todos texto?
pub fn is_text_or_lines(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Array(items) => items.iter().all(Value::is_string),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(value: &Value) -> Manifest {
        Manifest::new(value.clone())
    }

    #[test]
    fn effective_sustituye_los_campos_de_64bit_y_conserva_el_resto() {
        let m = manifest(&json!({
            "version": "1",
            "url": "https://old/a.zip",
            "env_set": {"A": "1"},
            "architecture": {"64bit": {"url": "https://new/a.zip"}, "32bit": {"url": "x"}}
        }));
        let effective = m.effective().unwrap();
        assert_eq!(effective.raw()["url"], "https://new/a.zip");
        assert_eq!(effective.raw()["env_set"], json!({"A": "1"}));
        // El original no se modifica.
        assert_eq!(m.raw()["url"], "https://old/a.zip");
    }

    #[test]
    fn effective_sin_64bit_falla() {
        for architecture in [
            json!({"32bit": {}}),
            json!("64bit"),
            json!(null),
            json!({"64bit": 3}),
        ] {
            let m = manifest(&json!({"version": "1", "architecture": architecture}));
            assert_eq!(
                m.effective().unwrap_err().to_string(),
                "No hay versión Windows x64"
            );
        }
        assert_eq!(
            manifest(&json!([1])).effective().unwrap_err().to_string(),
            "El manifiesto debe ser un objeto JSON"
        );
    }

    #[test]
    fn array_como_el_original() {
        assert_eq!(array(None), Vec::<&Value>::new());
        assert_eq!(array(Some(&Value::Null)), Vec::<&Value>::new());
        assert_eq!(array(Some(&json!("a"))), vec![&json!("a")]);
        assert_eq!(array(Some(&json!([1, 2]))).len(), 2);
        assert_eq!(array(Some(&json!(""))), vec![&json!("")]);
    }

    #[test]
    fn la_vista_es_tolerante_con_formas_variables() {
        let m = manifest(&json!({
            "version": 1.5,
            "notes": "una nota",
            "url": ["https://e.org/a.zip"],
            "hash": "sha256:abc",
            "bin": ["a.exe", ["b.exe", "b", "--x"]],
            "persist": ["data", ["cfg", "config"]],
            "depends": "main/otra",
            "env_set": {"X": "1"},
            "installer": {"script": "Write-Output 1"},
            "innosetup": "no es booleano",
            "shortcuts": 7
        }));
        let view = m.view().unwrap();
        assert_eq!(view.version.as_deref(), Some("1.5"));
        assert_eq!(view.notes.0, ["una nota"]);
        assert_eq!(view.hash.0, [Some("sha256:abc".to_owned())]);
        assert_eq!(view.bin.0.len(), 2);
        assert_eq!(
            view.persist.0[1].source_and_destination(),
            ("cfg", "config")
        );
        assert_eq!(view.depends.0, ["main/otra"]);
        assert_eq!(view.innosetup, None);
        assert_eq!(view.shortcuts, Vec::<Vec<String>>::new());
        assert_eq!(view.installer.unwrap().script.0, ["Write-Output 1"]);
    }

    #[test]
    fn la_huella_ignora_el_orden_y_no_usa_effective() {
        let a = manifest(&json!({"version": "1", "architecture": {"64bit": {"url": "u"}}}));
        let b = manifest(&json!({"architecture": {"64bit": {"url": "u"}}, "version": "1"}));
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), a.effective().unwrap().fingerprint());
        assert_eq!(a.fingerprint().len(), 64);
    }

    #[test]
    fn huella_con_valores_de_python() {
        let m = manifest(
            &serde_json::from_str(r#"{"version":"1.0","url":"https://x/y.zip"}"#).unwrap(),
        );
        assert_eq!(
            m.fingerprint(),
            "eca4f02a6930e1f070542b04f253b77e1030f903046250dca4407cf82791ffbb"
        );
    }

    fn definition(installer: &Value) -> Result<bool> {
        let manifest = json!({"installer": installer});
        let map = manifest.as_object().unwrap();
        installer_definition(map).map(|d| d.is_some())
    }

    #[test]
    fn instaladores_validos_e_invalidos() {
        assert!(!definition(&json!(null)).unwrap());
        assert!(!definition(&json!({})).unwrap());
        assert!(definition(&json!({"script": ""})).unwrap());
        assert!(definition(&json!({"script": []})).unwrap());
        assert!(definition(&json!({"script": ["a", "b"]})).unwrap());
        assert!(definition(&json!({"file": "setup.PS1", "args": ["/S"]})).unwrap());
        let cases = [
            (json!(true), "Pasos no soportados: installer"),
            (json!(false), "Pasos no soportados: installer"),
            (json!("setup.exe"), "Pasos no soportados: installer"),
            (json!([]), "Pasos no soportados: installer"),
            (
                json!({"file": "setup.exe"}),
                "Pasos no soportados: installer.file",
            ),
            (json!({"file": 3}), "Pasos no soportados: installer.file"),
            (
                json!({"script": "x", "args": "y"}),
                "Pasos no soportados: installer.args",
            ),
            (
                json!({"script": "x", "zeta": 1, "alfa": 2}),
                "Pasos no soportados: installer.alfa, installer.zeta",
            ),
            (
                json!({"file": "a.ps1", "script": "x"}),
                "Pasos no soportados: installer.script",
            ),
            (
                json!({"script": null}),
                "installer.script debe ser texto o una lista de líneas",
            ),
            (
                json!({"script": 7}),
                "installer.script debe ser texto o una lista de líneas",
            ),
            (
                json!({"script": ["a", 1]}),
                "installer.script debe ser texto o una lista de líneas",
            ),
            (
                json!({"file": "a.ps1", "args": 1}),
                "installer.args debe ser texto o una lista de líneas",
            ),
            (json!({"file": "../a.ps1"}), "Ruta no portable: ../a.ps1"),
        ];
        for (installer, expected) in cases {
            assert_eq!(
                definition(&installer).unwrap_err().to_string(),
                expected,
                "{installer}"
            );
        }
    }
}
