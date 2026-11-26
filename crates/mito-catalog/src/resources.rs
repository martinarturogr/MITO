//! Referencias `$bucketsdir` de los hooks (`bucket_resources.required_buckets`).
//!
//! Los scripts de hook pueden usar la carpeta `scripts/` de un bucket. Este módulo solo
//! decide qué buckets hacen falta; preparar (copiar) los recursos corresponde al motor.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use mito_core::Message;
use mito_core::storage::identifier;
use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{Error, Result, bail};
use crate::pyjson::py_str;

/// Expresión literal validada por los tests: compilarla no puede fallar.
#[expect(clippy::expect_used, reason = "expresión regular literal")]
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$(?:bucketsdir\b|\{bucketsdir\})").expect("expresión literal válida")
});

/// Expresión literal validada por los tests: compilarla no puede fallar.
#[expect(clippy::expect_used, reason = "expresión regular literal")]
static SCRIPTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\$\{bucket\}|\$bucket\b|[\w.-]+)[\\/]+scripts(?:[\\/]|\b)")
        .expect("expresión literal válida")
});

/// Buckets referenciados con `$bucketsdir` en `pre_install` e `installer`, ordenados.
///
/// `key` es la clave `bucket/app` del componente: `$bucket` y `${bucket}` se resuelven
/// con su bucket.
///
/// # Errors
///
/// `Pasos no soportados: referencia dinámica a $bucketsdir` si una referencia no sigue el
/// patrón `<bucket>/scripts`, y los de `identifier` para el nombre del bucket.
pub fn required_buckets(key: &str, manifest: &Map<String, Value>) -> Result<Vec<String>> {
    let text = ["pre_install", "installer"]
        .iter()
        .map(|field| manifest.get(*field).map_or_else(String::new, py_str))
        .collect::<Vec<_>>()
        .join("\n");
    let mut result = BTreeSet::new();
    for reference in REFERENCE.find_iter(&text) {
        let tail = text[reference.end()..].trim_start_matches(['\\', '/']);
        let Some(captures) = SCRIPTS.captures(tail) else {
            return bail(Message::new(
                "Pasos no soportados: referencia dinámica a $bucketsdir",
            ));
        };
        let name = &captures[1];
        let bucket = if name.starts_with('$') {
            key.split('/').next().unwrap_or_default()
        } else {
            name
        };
        result.insert(identifier(bucket).map_err(Error::from)?);
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn buckets(key: &str, manifest: &Value) -> Result<Vec<String>> {
        required_buckets(key, manifest.as_object().unwrap())
    }

    #[test]
    fn resuelve_referencias_literales_y_del_bucket_actual() {
        let manifest = json!({
            "pre_install": "Copy-Item \"$bucketsdir\\test\\scripts\\tool\\x.reg\" .",
            "installer": {"script": ["& ${bucketsdir}/Other/scripts/y.ps1", "$bucketsdir/$bucket/scripts/z"]}
        });
        assert_eq!(
            buckets("main/tool", &manifest).unwrap(),
            ["main", "other", "test"]
        );
        assert_eq!(
            buckets(
                "main/tool",
                &json!({"pre_install": "$bucketsdir/${bucket}/scripts/x"})
            )
            .unwrap(),
            ["main"]
        );
        assert_eq!(
            buckets("main/tool", &json!({"version": "1"})).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rechaza_referencias_dinamicas() {
        for text in [
            "$bucketsdir/$computed/scripts/x",
            "$bucketsdir/missing/other/x",
            "$bucketsdir",
        ] {
            let error = buckets("main/tool", &json!({"pre_install": text})).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Pasos no soportados: referencia dinámica a $bucketsdir",
                "{text}"
            );
        }
    }
}
