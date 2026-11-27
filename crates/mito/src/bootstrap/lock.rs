//! `core/bootstrap.lock.json`: las herramientas privadas fijadas por SHA-256 (ADR 0010).
//!
//! Solo contiene `tools`; `python` y `wheels` del original ya no existen. Si la instancia
//! no trae el archivo se usa el incluido en el binario.

use std::fs;
use std::path::PathBuf;

use mito_core::storage::{identifier, read_json};
use mito_core::{Layout, Message};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// Lock por defecto, incluido en el binario.
pub const EMBEDDED: &str = include_str!("../../../../core/bootstrap.lock.json");

/// Una herramienta privada de `core/tools/<id>/`.
#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    /// Identificador y nombre de la carpeta.
    pub id: String,
    /// Versión; su cambio reinstala la herramienta.
    pub version: String,
    /// Ficheros (relativos a la carpeta) que deben existir.
    #[serde(default)]
    pub required: Vec<String>,
    /// Descargas, en orden.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

/// Una descarga de una herramienta.
#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    /// Nombre local del archivo descargado.
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// URL inmutable.
    pub url: String,
    /// SHA-256 esperado, en minúsculas.
    pub sha256: String,
    /// Cómo se instala.
    pub install: Install,
}

/// Cómo se instala un artefacto.
#[derive(Debug, Clone, Deserialize)]
pub struct Install {
    /// `file`, `zip` o `sevenZip`.
    #[serde(rename = "type")]
    pub kind: String,
    /// `file`: nombre destino (por defecto, el del archivo).
    #[serde(default)]
    pub target: Option<String>,
    /// `zip`: carpeta del ZIP que se copia (por defecto, `.`).
    #[serde(default)]
    pub source: Option<String>,
    /// `sevenZip`: ejecutable (relativo al payload) que extrae el archivo.
    #[serde(default)]
    pub executable: Option<String>,
}

/// El lock y el SHA-256 de sus bytes.
#[derive(Debug, Clone)]
pub struct Lock {
    /// Herramientas, en el orden de preparación.
    pub tools: Vec<Tool>,
    /// SHA-256 hexadecimal del archivo.
    pub sha256: String,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    tools: Vec<Tool>,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

impl Lock {
    /// Interpreta los bytes de un lock.
    ///
    /// # Errors
    ///
    /// Un JSON inválido, o un `id` de herramienta que no es un nombre seguro de carpeta.
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let content = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let raw: Raw = serde_json::from_slice(content)
            .map_err(|e| anyhow::anyhow!("bootstrap.lock.json no es válido: {e}"))?;
        for tool in &raw.tools {
            identifier(&tool.id)?;
        }
        Ok(Self {
            tools: raw.tools,
            sha256: hex(bytes),
        })
    }

    /// Lock de la instancia (`core/bootstrap.lock.json`) o, si falta, el incluido.
    ///
    /// # Errors
    ///
    /// Los de [`Self::parse`] y de lectura.
    pub fn load(layout: &Layout) -> anyhow::Result<Self> {
        let path = layout.bootstrap_lock();
        match fs::read(&path) {
            Ok(bytes) => Self::parse(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::parse(EMBEDDED.as_bytes()),
            Err(e) => Err(mito_core::Error::io(&path, e).into()),
        }
    }
}

impl Tool {
    /// Carpeta de la herramienta: `core/tools/<id>`.
    pub fn folder(&self, layout: &Layout) -> PathBuf {
        layout.tools().join(&self.id)
    }

    /// Nombre para la bienvenida.
    pub fn label(&self) -> String {
        match self.id.as_str() {
            "7zip" => "7-Zip".to_owned(),
            "windows-terminal" => "Windows Terminal".to_owned(),
            "innounp" => "innounp (extracción de instaladores Inno Setup)".to_owned(),
            other => other.to_owned(),
        }
    }

    /// ¿Está lista? Marcador `.mito-tool.json` con la versión del lock y todos los
    /// `required` como ficheros. Un marcador corrupto equivale a una herramienta ausente.
    pub fn is_ready(&self, layout: &Layout) -> bool {
        let folder = self.folder(layout);
        let marker = folder.join(".mito-tool.json");
        let version_matches = read_json::<Value>(&marker)
            .ok()
            .and_then(|data| {
                data.get("version")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .is_some_and(|version| version == self.version);
        version_matches
            && self
                .required
                .iter()
                .all(|file| mito_core::storage::inside(&folder, file).is_ok_and(|p| p.is_file()))
    }
}

/// Mensaje de «Herramienta incompleta» para una herramienta.
pub fn incomplete(tool: &str) -> mito_core::Error {
    mito_core::Error::domain(Message::new("Herramienta incompleta: {p1}").arg("p1", tool))
}
