//! Instancia temporal, catálogo local y descargador falso para las pruebas (sin red).
#![allow(clippy::unused_self, clippy::needless_pass_by_value)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use mito_core::{Message, Operation};
use mito_engine::{Downloader, Engine, Extensions, InstallOptions};
use serde_json::{Value, json};

/// Hash declarado de los manifiestos de prueba (el descargador falso no lo comprueba).
pub(crate) const ZERO_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Descargador falso: escribe como contenido los bytes de la URL.
#[derive(Clone, Default)]
pub(crate) struct Fake {
    /// URL pedidas, en orden.
    pub hits: Arc<Mutex<Vec<String>>>,
    /// Subcadenas de URL que fallan con un error de red.
    pub fail: Arc<Mutex<Vec<String>>>,
    /// Subcadenas de URL que fallan con un error de integridad.
    pub tamper: Arc<Mutex<Vec<String>>>,
    /// Subcadenas de URL que cancelan la operación durante la descarga.
    pub cancel: Arc<Mutex<Vec<String>>>,
    /// Subcadenas de URL cuya descarga entra en pánico.
    pub explode: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn matches(list: &Mutex<Vec<String>>, url: &str) -> bool {
        list.lock().unwrap().iter().any(|f| url.contains(f))
    }
}

impl Downloader for Fake {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        _expected: Option<&str>,
        operation: &Operation,
    ) -> mito_transfer::Result<()> {
        self.hits.lock().unwrap().push(url.to_owned());
        assert!(!Self::matches(&self.explode, url), "descarga con pánico");
        if Self::matches(&self.cancel, url) {
            operation.cancel();
        }
        if self.fail.lock().unwrap().iter().any(|f| url.contains(f)) {
            return Err(mito_transfer::Error::Network {
                reason: "fallo simulado".into(),
            });
        }
        if self.tamper.lock().unwrap().iter().any(|f| url.contains(f)) {
            return Err(mito_transfer::Error::Integrity {
                message: Box::new(Message::new("Descarga alterada")),
                url: url.to_owned(),
                size: Some(12),
                host: "proxy.local".to_owned(),
                portal: false,
            });
        }
        fs::write(destination, url.as_bytes())
            .map_err(|source| mito_core::Error::io(destination, source))?;
        Ok(())
    }
}

/// Traductor sin catálogos: devuelve el texto español de cada plantilla (como el original
/// con el idioma `español`).
pub(crate) fn spanish() -> mito_i18n::Translator {
    mito_i18n::Translator::with_bundles(
        "english",
        mito_i18n::Bundle::empty(),
        mito_i18n::Bundle::empty(),
    )
}

/// Instancia temporal con su motor y una carpeta que hace de origen del bucket `main`.
pub(crate) struct Fx {
    /// Raíz de la instancia (se borra al soltar la instancia de prueba).
    pub _dir: tempfile::TempDir,
    /// Origen local del bucket (`<origen>/<app>.json`).
    pub source: tempfile::TempDir,
    /// Motor de la instancia.
    pub engine: Arc<Engine>,
    /// Descargador inyectado.
    pub fake: Fake,
}

pub(crate) fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let extensions = Extensions {
        downloader: Box::new(fake.clone()),
        ..Extensions::default()
    };
    let engine = Engine::with_extensions(dir.path(), extensions);
    engine.initialize().unwrap();
    Fx {
        _dir: dir,
        source,
        engine: Arc::new(engine),
        fake,
    }
}

impl Fx {
    /// Manifiesto básico con un `.exe` como descarga y un hash declarado.
    pub fn manifest(&self, version: &str) -> Value {
        json!({"version": version, "description": "Una herramienta",
               "url": "https://example.org/tool.exe", "hash": ZERO_HASH, "bin": "tool.exe"})
    }

    /// Escribe el snapshot `core/buckets/<bucket>.json` con el origen local.
    pub fn bucket(&self, bucket: &str, apps: &Value) {
        let path = self
            .engine
            .layout()
            .buckets()
            .join(format!("{bucket}.json"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let snapshot = json!({"schema": 1, "source": self.source.path().display().to_string(),
                              "apps": apps});
        fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    }

    /// Escribe el snapshot del bucket `main`.
    pub fn catalog(&self, apps: Value) {
        self.bucket("main", &apps);
    }

    /// Publica `manifest` en el origen local: lo que `refresh_keys` encuentra.
    pub fn publish(&self, app: &str, manifest: &Value) {
        let path = self.source.path().join(format!("{app}.json"));
        fs::write(path, serde_json::to_vec(manifest).unwrap()).unwrap();
    }

    /// Instala `key` sin pasar por la interfaz.
    pub fn install(&self, key: &str, profile: Option<&str>) {
        let yes = |_: &[String]| true;
        let options = InstallOptions {
            profile,
            confirm_unverified: Some(&yes),
            ..InstallOptions::default()
        };
        self.engine
            .install(&[key], &options, &|_| {}, &Operation::detached())
            .unwrap();
    }
}
