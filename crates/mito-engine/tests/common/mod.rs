//! Utilidades de los tests de integración: instancia temporal, catálogo local y un
//! descargador falso que sirve ZIPs construidos en el test (ninguno sale a Internet).
#![allow(dead_code, reason = "cada test usa una parte distinta")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

pub mod http;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mito_core::Operation;
use mito_engine::{Downloader, Engine, Env, Extensions, InstallOptions, Records};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

pub const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// ZIP en memoria con los archivos indicados.
pub fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

type Served = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Descargador falso: sirve lo registrado para la URL o, por defecto, un ZIP con
/// `tool.exe` cuyo contenido son los bytes de la URL.
#[derive(Clone, Default)]
pub struct Fake {
    pub served: Served,
    pub hits: Arc<Mutex<Vec<String>>>,
    pub fail: Arc<Mutex<Vec<String>>>,
}

impl Downloader for Fake {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        _expected: Option<&str>,
        _operation: &Operation,
    ) -> mito_transfer::Result<()> {
        self.hits.lock().unwrap().push(url.to_owned());
        if self.fail.lock().unwrap().iter().any(|f| url.contains(f)) {
            return Err(mito_transfer::Error::Network {
                reason: "fallo simulado".into(),
            });
        }
        let bytes = self
            .served
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| zip_of(&[("tool.exe", url.as_bytes())]));
        fs::write(destination, bytes).map_err(|e| mito_core::Error::io(destination, e))?;
        Ok(())
    }
}

/// Instancia temporal con su motor.
pub struct Fx {
    pub dir: tempfile::TempDir,
    pub engine: Engine,
    pub fake: Fake,
}

pub fn fx() -> Fx {
    fx_with(|_| {})
}

pub fn fx_with(configure: impl FnOnce(&mut Extensions)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let mut extensions = Extensions {
        downloader: Box::new(fake.clone()),
        ..Extensions::default()
    };
    configure(&mut extensions);
    let engine = Engine::with_extensions(dir.path(), extensions);
    engine.initialize().unwrap();
    Fx { dir, engine, fake }
}

pub fn base_env() -> Env {
    Env::from([("PATH".to_owned(), r"C:\host".to_owned())])
}

/// Manifiesto básico con `bin: tool.exe` y un hash declarado.
pub fn manifest(version: &str) -> Value {
    json!({"version": version, "url": "https://example.org/tool.zip",
           "hash": ZERO_HASH, "bin": "tool.exe"})
}

impl Fx {
    pub fn root(&self) -> &Path {
        self.engine.root()
    }

    /// Escribe `core/buckets/<bucket>.json`.
    pub fn bucket(&self, bucket: &str, apps: Value) {
        let path = self
            .engine
            .layout()
            .buckets()
            .join(format!("{bucket}.json"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&json!({"apps": apps})).unwrap()).unwrap();
    }

    /// Escribe `core/buckets/main.json`.
    pub fn catalog(&self, apps: Value) {
        self.bucket("main", apps);
    }

    pub fn profile(&self, name: &str) {
        self.engine.create_profile(name, None).unwrap();
    }

    pub fn serve(&self, url: &str, bytes: Vec<u8>) {
        self.fake
            .served
            .lock()
            .unwrap()
            .insert(url.to_owned(), bytes);
    }

    pub fn install(&self, names: &[&str], profile: Option<&str>) -> mito_engine::Result<Records> {
        self.install_with(names, profile, BTreeMap::new())
    }

    pub fn install_with(
        &self,
        names: &[&str],
        profile: Option<&str>,
        manifests: BTreeMap<String, Value>,
    ) -> mito_engine::Result<Records> {
        let yes = |_: &[String]| true;
        let options = InstallOptions {
            profile,
            manifests,
            confirm_unverified: Some(&yes),
            ..InstallOptions::default()
        };
        self.engine
            .install(names, &options, &|_| {}, &Operation::detached())
    }

    pub fn env(&self, profile: &str) -> Env {
        self.engine.environment_from(profile, base_env()).unwrap()
    }

    pub fn apps(&self, profile: &str) -> Vec<String> {
        self.engine.profile(profile).unwrap().apps
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        let mut path = self.root().to_path_buf();
        path.extend(relative.split('/'));
        path
    }

    pub fn staging_is_empty(&self) -> bool {
        fs::read_dir(self.engine.layout().staging())
            .map_or(true, |mut entries| entries.next().is_none())
    }
}

pub fn error_text<T: std::fmt::Debug>(result: mito_engine::Result<T>) -> String {
    result.unwrap_err().to_string()
}
