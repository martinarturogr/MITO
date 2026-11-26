//! Flujo completo con manifiestos de ejemplo: ZIP de bucket → snapshot → catálogo →
//! compatibilidad → búsqueda → huella.
#![allow(
    clippy::unwrap_used,
    reason = "los helpers de tests de integración pueden abortar"
)]

use std::fs;
use std::io::Write as _;
use std::path::Path;

use mito_catalog::{
    BucketSources, Catalog, Compatibility, FetchError, Fetcher, Manifest, compatibility,
    rank_entries,
};
use mito_core::{Layout, Operation};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

const SEVEN_ZIP: &str = include_str!("fixtures/7zip.json");
const CAFE: &str = include_str!("fixtures/cafe.json");
const MODULE: &str = include_str!("fixtures/module.json");

/// Huellas calculadas con `hashlib.sha256(json.dumps(m, sort_keys=True,
/// separators=(',', ':')).encode()).hexdigest()` de Python 3.14 sobre los mismos archivos.
const FINGERPRINTS: [(&str, &str); 3] = [
    (
        "7zip",
        "3ae7567612ca2e0fe9d5c12a8a872a42262710a8c3e5ff5c4cd7b080f743bd54",
    ),
    (
        "cafe",
        "a959b8da60782dbab8b87d0536da5ffdd07f63b3676416bca8cba75d74ba8051",
    ),
    (
        "module",
        "c9b685c8fc310d709ce361674f2f5703e0552915629b3a6bf1113a97123e2b91",
    ),
];

struct ZipFetcher;

impl Fetcher for ZipFetcher {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        _operation: &Operation,
    ) -> Result<(), FetchError> {
        assert_eq!(url, "https://api.github.com/repos/example/main/zipball");
        assert_eq!(expected, None);
        let file = fs::File::create(destination).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, content) in [
            ("example-main-abc123/bucket/7zip.json", SEVEN_ZIP),
            ("example-main-abc123/bucket/cafe.json", CAFE),
            ("example-main-abc123/bucket/module.json", MODULE),
            ("example-main-abc123/scripts/helper.ps1", "Write-Output 1"),
            ("example-main-abc123/README.md", "# bucket"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        Ok(())
    }
}

fn imported_catalog() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    let mut sources = BucketSources::new();
    sources
        .insert("main", "https://github.com/example/main")
        .unwrap();
    let catalog = Catalog::new(Layout::new(root.path()), sources);
    let count = catalog
        .import_bucket(
            "main",
            "https://github.com/example/main",
            &ZipFetcher,
            &Operation::detached(),
        )
        .unwrap();
    assert_eq!(count, 3);
    (root, catalog)
}

#[test]
fn importar_clasificar_buscar_y_resolver() {
    let (_root, catalog) = imported_catalog();
    let entries = catalog.search_entries().unwrap();
    let statuses: Vec<(String, String)> = entries
        .iter()
        .map(|entry| (entry.key.clone(), entry.status.text()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("main/7zip".to_owned(), "Compatible".to_owned()),
            ("main/cafe".to_owned(), "Compatible".to_owned()),
            (
                "main/module".to_owned(),
                "Pasos no soportados: psmodule".to_owned()
            ),
        ]
    );

    let none = std::collections::BTreeSet::new();
    let found = rank_entries(&entries, "archiver", &none).unwrap();
    assert_eq!(found[0].key, "main/7zip");
    // La búsqueda ignora acentos y mayúsculas, y mira la descripción.
    let found = rank_entries(&entries, "CAFÉ recetas", &none).unwrap();
    assert_eq!(found[0].key, "main/cafe");
    let found = rank_entries(&entries, "modulo", &none).unwrap();
    assert_eq!(found[0].key, "main/module");

    let (key, manifest) = catalog.resolve("7zip").unwrap();
    assert_eq!(key, "main/7zip");
    assert_eq!(compatibility(&key, &manifest), Compatibility::Compatible);
    assert_eq!(
        manifest.effective().unwrap().raw()["url"],
        "https://www.7-zip.org/a/7z2409-x64.msi"
    );
}

#[test]
fn el_snapshot_conserva_el_json_original_y_la_huella_coincide_con_python() {
    let (_root, catalog) = imported_catalog();
    let snapshot: Value =
        serde_json::from_slice(&fs::read(catalog.snapshot_path("main")).unwrap()).unwrap();
    for (name, expected) in FINGERPRINTS {
        let manifest = Manifest::new(snapshot["apps"][name].clone());
        assert_eq!(manifest.fingerprint(), expected, "{name}");
    }
    // El texto original (con espacios y orden de claves propios) equivale al del snapshot.
    let original: Value = serde_json::from_str(CAFE).unwrap();
    assert_eq!(snapshot["apps"]["cafe"], original);
    let keys: Vec<_> = snapshot["apps"]["cafe"]
        .as_object()
        .unwrap()
        .keys()
        .take(3)
        .collect();
    assert_eq!(keys, ["version", "description", "homepage"]);
    assert_eq!(snapshot["apps"]["cafe"]["ratio"], json!(1.5e-7));
}

#[test]
fn la_vista_tipada_lee_listas_y_textos() {
    let (_root, catalog) = imported_catalog();
    let (_, manifest) = catalog.resolve("main/cafe").unwrap();
    let view = manifest.view().unwrap();
    assert_eq!(view.version.as_deref(), Some("2.1.0"));
    assert_eq!(view.url.0.len(), 2);
    assert_eq!(view.hash.0[1], Some("skip".to_owned()));
    assert_eq!(view.extract_to.0, ["app", "data"]);
    assert_eq!(view.depends.0, ["main/7zip"]);
    assert_eq!(view.pre_install.0, ["Write-Output 'preparando'"]);
}
