//! `mito doctor`: comprobación del estado de la instancia.
//!
//! El original informa de Python, Pillow, Textual y el SDK de MCP; en Rust no existen
//! (ADR 0005 y 0008). Se conserva lo demás: raíz, `home` único, herramientas privadas,
//! catálogo y profiles, y se añaden la versión, el límite de la ruta, las carpetas de la
//! instancia, los buckets y el idioma.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use mito_core::config::read_global;
use mito_core::storage::read_json;
use mito_engine::Engine;
use serde_json::Value;

use crate::console::Console;

/// Longitud máxima recomendada de la raíz (`MAX_ROOT` de `path_limit.py`).
pub const MAX_ROOT: usize = 32;

/// Herramientas privadas que se comprueban (`core/tools/<nombre>/.mito-tool.json`).
const TOOLS: [&str; 2] = ["windows-terminal", "7zip"];

/// Imprime el informe de `doctor`.
///
/// # Errors
///
/// Los de lectura del catálogo, la configuración y las herramientas, y el primer profile
/// cuyo entorno no se puede construir (como el original, que termina con ese error).
pub fn run(engine: &Engine, console: &Console) -> anyhow::Result<()> {
    let layout = engine.layout();
    let root = engine.root();
    console.line(format!("MITO: {}", env!("CARGO_PKG_VERSION")));
    console.line(format!("Raíz: {}", root.display()));
    let length = root
        .display()
        .to_string()
        .trim_end_matches(['\\', '/'])
        .chars()
        .count();
    if length > MAX_ROOT {
        console.line(format!(
            "Longitud de la raíz: {length} caracteres (máximo recomendado {MAX_ROOT}): \
             mueve la carpeta más cerca de la raíz del disco"
        ));
    } else {
        console.line(format!(
            "Longitud de la raíz: {length} caracteres (máximo recomendado {MAX_ROOT})"
        ));
    }
    console.line(format!("Home único: {}", layout.home().display()));
    let setting = read_global(layout)?
        .effective("mito.language")
        .unwrap_or(mito_i18n::DEFAULT_SETTING)
        .to_owned();
    let language = mito_i18n::Translator::from_layout(layout)?.language();
    console.line(format!("Idioma: {language} (mito.language = {setting})"));
    for (label, path) in [
        ("config.properties", layout.config_file()),
        ("profiles", layout.profiles()),
        ("workspaces", layout.workspaces()),
        ("core/apps", layout.apps()),
        ("core/buckets", layout.buckets()),
        ("logs", layout.logs()),
    ] {
        let state = if path.exists() { "existe" } else { "falta" };
        console.line(format!("{label}: {} ({state})", path.display()));
    }
    for tool in TOOLS {
        console.line(tool_line(&layout.tools().join(tool), tool)?);
    }
    let catalog = engine.catalog()?;
    let apps = catalog.all()?;
    let mut per_bucket: BTreeMap<String, usize> = BTreeMap::new();
    for (key, _) in &apps {
        let bucket = key.split('/').next().unwrap_or_default();
        *per_bucket.entry(bucket.to_owned()).or_default() += 1;
    }
    for name in catalog.bucket_names()? {
        let origin = catalog
            .sources()
            .get(&name)
            .unwrap_or("sin origen configurado");
        let snapshot = per_bucket.get(&name).map_or_else(
            || "sin snapshot".to_owned(),
            |count| format!("snapshot de {count} manifiestos"),
        );
        console.line(format!("Bucket {name}: {origin} ({snapshot})"));
    }
    let profiles = engine.profiles();
    console.line(format!(
        "Catálogo: {} apps | Profiles: {}",
        apps.len(),
        profiles.len()
    ));
    for name in &profiles {
        engine.environment(name)?;
        console.line(format!("OK profile {name}"));
    }
    Ok(())
}

/// Línea de una herramienta privada: su versión o el aviso de que falta.
fn tool_line(folder: &Path, tool: &str) -> anyhow::Result<String> {
    let marker = folder.join(".mito-tool.json");
    if !marker.exists() {
        return Ok(format!("Falta {tool}; ejecuta bootstrap"));
    }
    let data: Value = read_json(&marker)?;
    let version = data
        .get("version")
        .map(mito_catalog::pyjson::py_str)
        .with_context(|| format!("{} no declara la versión", marker.display()))?;
    Ok(format!("{tool}: {version}"))
}
