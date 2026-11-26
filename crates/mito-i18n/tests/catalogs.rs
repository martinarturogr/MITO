//! Los catálogos incrustados cubren los mensajes de MITO y conservan sus parámetros.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use mito_i18n::{LANGUAGES, bundle, catalog_json, fields};
use regex::Regex;
use serde::de::{Deserializer, MapAccess, Visitor};

/// Carga un catálogo rechazando claves duplicadas (que `serde_json` aceptaría).
fn load(json: &str) -> BTreeMap<String, String> {
    struct Strict(BTreeMap<String, String>);

    impl<'de> serde::Deserialize<'de> for Strict {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Entries;
            impl<'de> Visitor<'de> for Entries {
                type Value = Strict;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("un objeto plano")
                }

                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
                    let mut entries = BTreeMap::new();
                    while let Some((key, value)) = map.next_entry::<String, String>()? {
                        if entries.insert(key.clone(), value).is_some() {
                            return Err(serde::de::Error::custom(format!("clave repetida: {key}")));
                        }
                    }
                    Ok(Strict(entries))
                }
            }
            deserializer.deserialize_map(Entries)
        }
    }

    serde_json::from_str::<Strict>(json).unwrap().0
}

#[test]
fn los_catalogos_cubren_los_mensajes_y_conservan_los_marcadores() {
    let source = load(catalog_json("español"));
    let english = load(catalog_json("english"));
    assert_eq!(LANGUAGES.len(), 24);
    assert_eq!(
        source.keys().collect::<Vec<_>>(),
        english.keys().collect::<Vec<_>>()
    );
    for (key, value) in &source {
        assert_eq!(key, value, "español.json es la identidad");
        assert_ne!(english[key], "");
        assert_eq!(fields(value), fields(&english[key]), "{key}");
    }
    for language in LANGUAGES {
        let localized = load(catalog_json(language));
        assert_eq!(
            localized.keys().collect::<Vec<_>>(),
            source.keys().collect::<Vec<_>>(),
            "{language}"
        );
        for (key, value) in &localized {
            assert!(!value.is_empty(), "{language}: {key}");
            assert!(fields(key).is_some(), "{language}: {key}");
            assert_eq!(fields(key), fields(value), "{language}: {key}");
        }
        // Ninguna entrada válida se pierde al cargar el catálogo en tiempo de ejecución.
        assert_eq!(bundle(language).len(), source.len(), "{language}");
    }
}

fn rust_files(folder: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn unescape(literal: &str) -> String {
    let mut out = String::new();
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Equivalente de la comprobación estática del original (cada `tr('…')` literal está
/// en `español.json`): toda plantilla de `Message::new("…")` de `mito-core` debe estar
/// en el catálogo para que se pueda traducir. Los mensajes de pruebas no cuentan.
#[test]
fn toda_plantilla_de_mito_core_esta_en_el_catalogo_espanol() {
    let source = load(catalog_json("español"));
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mito-core/src");
    let mut files = Vec::new();
    rust_files(&sources, &mut files);
    assert_ne!(files.len(), 0);
    let pattern = Regex::new(r#"Message::new\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
    let mut checked = 0;
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        let production: String = text
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        for found in pattern.captures_iter(&production) {
            let template = unescape(&found[1]);
            assert!(
                source.contains_key(&template),
                "{}: plantilla sin traducción: {template:?}",
                file.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 30,
        "se esperaban muchas plantillas, hay {checked}"
    );
}
