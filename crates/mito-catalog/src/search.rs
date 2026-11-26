//! Búsqueda y ranking del catálogo (`search.py`).
//!
//! Contrato: `docs/spec/modules/search.md`. Cada palabra de la consulta coincide por
//! igualdad (números) o por prefijo (letras); el orden es cobertura, aciertos en el
//! identificador, aciertos en el bucket, app exacta y, por último, la clave.
//!
//! Diferencias de Unicode respecto a Python: `casefold()` se hace con el plegado de
//! mayúsculas completo de la crate `caseless` y la normalización NFKD con
//! `unicode-normalization`; las tablas de Unicode pueden ir una versión por delante o por
//! detrás de las de `CPython`, lo que solo afecta a caracteres añadidos entre ambas.

use std::cmp::Reverse;
use std::collections::BTreeSet;

use mito_core::Message;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

use crate::error::{Result, bail};
use crate::manifest::Manifest;
use crate::pyjson::py_str;

/// Fila del catálogo para buscar: clave `bucket/app`, manifiesto y estado.
///
/// El estado (`S`) no interviene en el ranking; solo se transporta.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<S> {
    /// Clave `bucket/app`.
    pub key: String,
    /// Manifiesto original del snapshot.
    pub manifest: Manifest,
    /// Estado de compatibilidad u otro dato del llamador.
    pub status: S,
}

impl<S> Entry<S> {
    /// Crea una fila.
    pub fn new(key: impl Into<String>, manifest: Manifest, status: S) -> Self {
        Self {
            key: key.into(),
            manifest,
            status,
        }
    }
}

/// Palabras de un texto: minúsculas sin acentos, solo `[a-z]+` y `[0-9]+`.
///
/// `casefold` → NFKD → se descartan las marcas combinantes → se extraen las secuencias
/// de letras ASCII y de dígitos (`java21` produce `java` y `21`).
pub fn tokens(text: &str) -> Vec<String> {
    let folded = caseless::default_case_fold_str(text);
    let plain: String = folded
        .nfkd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .collect();
    let mut words = Vec::new();
    let mut current = String::new();
    let mut digits = false;
    for c in plain.chars() {
        let class = if c.is_ascii_lowercase() {
            Some(false)
        } else if c.is_ascii_digit() {
            Some(true)
        } else {
            None
        };
        match class {
            Some(is_digit) if !current.is_empty() && is_digit == digits => current.push(c),
            Some(is_digit) => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                digits = is_digit;
                current.push(c);
            }
            None if !current.is_empty() => words.push(std::mem::take(&mut current)),
            None => {}
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// `str(manifest.get(field, ''))`: vacío si falta; `None` si es `null`; la
/// representación de Python para listas y objetos.
fn field_text(manifest: &Manifest, field: &str) -> String {
    manifest
        .as_object()
        .and_then(|map| map.get(field))
        .map_or_else(String::new, py_str)
}

fn hits(word: &str, values: &[String]) -> bool {
    if word.bytes().all(|b| b.is_ascii_digit()) {
        values.iter().any(|value| value == word)
    } else {
        values.iter().any(|value| value.starts_with(word))
    }
}

/// Ordena las coincidencias de `entries` para `query`, sin las filas de los buckets en
/// `disabled`.
///
/// Con una consulta sin palabras devuelve todas las filas de buckets habilitados
/// ordenadas por clave. Con palabras, descarta las filas sin cobertura y ordena por
/// `(-cobertura, -aciertos en el identificador, -aciertos en el bucket, -app exacta,
/// clave)` con una ordenación estable.
///
/// # Errors
///
/// `Clave de catálogo no válida` si una clave no contiene `/` (el original lanza
/// `ValueError` y toda la búsqueda falla); solo se comprueba cuando hay palabras.
pub fn rank_entries<'a, S>(
    entries: &'a [Entry<S>],
    query: &str,
    disabled: &BTreeSet<String>,
) -> Result<Vec<&'a Entry<S>>> {
    let words = tokens(query);
    let bucket_of = |key: &'a str| key.split('/').next().unwrap_or_default();
    if words.is_empty() {
        let mut rows: Vec<&Entry<S>> = entries
            .iter()
            .filter(|row| !disabled.contains(bucket_of(&row.key)))
            .collect();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        return Ok(rows);
    }
    let mut ranked = Vec::new();
    for row in entries {
        let Some((bucket, app)) = row.key.split_once('/') else {
            return bail(Message::new("Clave de catálogo no válida: {p1}").arg("p1", &row.key));
        };
        if disabled.contains(bucket) {
            continue;
        }
        let bucket_words = tokens(bucket);
        let app_words = tokens(app);
        let description = tokens(&field_text(&row.manifest, "description"));
        let extra = tokens(&format!(
            "{} {}",
            field_text(&row.manifest, "version"),
            field_text(&row.manifest, "notes")
        ));
        let identifier_words: Vec<String> =
            bucket_words.iter().chain(&app_words).cloned().collect();
        let everywhere: Vec<String> = identifier_words
            .iter()
            .chain(&description)
            .chain(&extra)
            .cloned()
            .collect();
        let count = |values: &[String]| words.iter().filter(|word| hits(word, values)).count();
        let coverage = count(&everywhere);
        if coverage == 0 {
            continue;
        }
        let key_hits = count(&identifier_words);
        let bucket_hits = count(&bucket_words);
        let exact_app = usize::from(app_words == words);
        ranked.push((
            (
                Reverse(coverage),
                Reverse(key_hits),
                Reverse(bucket_hits),
                Reverse(exact_app),
                row.key.as_str(),
            ),
            row,
        ));
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ranked.into_iter().map(|(_, row)| row).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    type Row = Entry<&'static str>;

    fn entries() -> Vec<Row> {
        [
            ("main/activemq", "Java message server", "6.1"),
            ("extras/anypoint-studio", "Based on Eclipse IDE", "7"),
            ("extras/eclipse", "Eclipse IDE", "4"),
            ("java/temurin21-jdk", "Java development kit", "21.0.1"),
            ("java/temurin11-jdk", "Java development kit", "11.0.1"),
            ("main/python", "Python programming language", "3.14"),
            ("versions/python313", "Python programming language", "3.13"),
        ]
        .into_iter()
        .map(|(key, description, version)| {
            Entry::new(
                key,
                Manifest::new(json!({"description": description, "version": version})),
                "Compatible",
            )
        })
        .collect()
    }

    fn keys(rows: &[&Row]) -> Vec<String> {
        rows.iter().map(|row| row.key.clone()).collect()
    }

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn disabled(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn el_identificador_precede_a_la_descripcion() {
        let rows = entries();
        assert_eq!(
            rank_entries(&rows, "eclipse", &none()).unwrap()[0].key,
            "extras/eclipse"
        );
        assert!(
            rank_entries(&rows, "java", &none()).unwrap()[0]
                .key
                .starts_with("java/")
        );
    }

    #[test]
    fn barras_y_varias_palabras() {
        let rows = entries();
        for query in ["python", "python/", "/python"] {
            assert_eq!(
                rank_entries(&rows, query, &none()).unwrap()[0].key,
                "main/python",
                "{query}"
            );
        }
        // `lts` no aparece en ningún campo: la cobertura parcial se conserva.
        assert_eq!(
            rank_entries(&rows, "java 21 lts", &none()).unwrap()[0].key,
            "java/temurin21-jdk"
        );
        assert_eq!(
            rank_entries(&rows, "java21", &none()).unwrap()[0].key,
            "java/temurin21-jdk"
        );
    }

    #[test]
    fn buckets_deshabilitados() {
        let rows = entries();
        let found = rank_entries(&rows, "python", &disabled(&["main"])).unwrap();
        assert_eq!(found[0].key, "versions/python313");
        assert_eq!(
            keys(&rank_entries(&rows, "python", &disabled(&["main", "versions"])).unwrap()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn consulta_vacia_y_descripcion_cambiada() {
        let mut rows = entries();
        let found = rank_entries(&rows, "", &disabled(&["main"])).unwrap();
        let mut expected: Vec<String> = rows
            .iter()
            .filter(|r| !r.key.starts_with("main/"))
            .map(|r| r.key.clone())
            .collect();
        expected.sort();
        assert_eq!(keys(&found), expected);
        assert_eq!(
            keys(&rank_entries(&rows, "uniquephrase", &none()).unwrap()),
            Vec::<String>::new()
        );
        rows[0].manifest = Manifest::new(json!({"description": "uniquephrase", "version": "6.1"}));
        assert_eq!(
            rank_entries(&rows, "uniquephrase", &none()).unwrap()[0].key,
            "main/activemq"
        );
    }

    #[test]
    fn tokenizacion_como_el_original() {
        assert_eq!(tokens("java21"), ["java", "21"]);
        assert_eq!(tokens("Caf\u{e9} \u{c5}ngstr\u{f6}m"), ["cafe", "angstrom"]);
        assert_eq!(tokens("a-b.c/d  e"), ["a", "b", "c", "d", "e"]);
        // casefold completo: la `ß` se pliega a `ss`.
        assert_eq!(tokens("Stra\u{df}e"), ["strasse"]);
        // NFKD descompone la ligadura y el superíndice; `ø` no se descompone.
        assert_eq!(tokens("\u{fb01}x \u{b2}"), ["fix", "2"]);
        assert_eq!(tokens("\u{f8}"), Vec::<String>::new());
        assert_eq!(tokens("/ - ."), Vec::<String>::new());
    }

    /// Valores obtenidos con `re.findall` sobre `casefold` + NFKD de Python 3.14.
    #[test]
    fn tokenizacion_unicode_coincide_con_python() {
        let cases: [(&str, &[&str]); 23] = [
            ("Stra\u{df}e", &["strasse"]),
            ("\u{130}st", &["ist"]),
            ("\u{1c5}", &["dz"]),
            ("\u{fb03}x", &["ffix"]),
            ("\u{3a3}\u{391}\u{3a3}", &[]),
            ("\u{216b}", &["xii"]),
            ("\u{1e9e}", &["ss"]),
            ("\u{149}", &["n"]),
            ("\u{fdfa}", &[]),
            ("\u{2460}b", &["1", "b"]),
            ("Caf\u{e9}\u{301}", &["cafe"]),
            ("\u{212a}", &["k"]),
            ("\u{b5}g", &["g"]),
            ("\u{2126} ohm", &["ohm"]),
            ("\u{131}i", &["i"]),
            ("\u{fb00}", &["ff"]),
            ("\u{1e9b}x", &["sx"]),
            ("a\u{ad}b", &["a", "b"]),
            ("\u{3392}", &["z"]),
            ("\u{2160}a1", &["ia", "1"]),
            ("\u{bd}", &["1", "2"]),
            ("\u{1d400}x", &["x"]),
            ("\u{ff21}\u{ff22}\u{ff11}", &["ab", "1"]),
        ];
        for (text, expected) in cases {
            assert_eq!(tokens(text), expected, "{text:?}");
        }
    }

    #[test]
    fn los_numeros_solo_coinciden_por_igualdad_y_las_letras_por_prefijo() {
        let rows = vec![
            Entry::new("main/a210", Manifest::new(json!({})), ()),
            Entry::new("main/a21", Manifest::new(json!({})), ()),
        ];
        let found = rank_entries(&rows, "21", &none()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "main/a21");
        assert_eq!(rank_entries(&rows, "a", &none()).unwrap().len(), 2);
    }

    #[test]
    fn orden_por_cobertura_identificador_bucket_app_exacta_y_clave() {
        let rows = vec![
            Entry::new("zeta/tool", Manifest::new(json!({})), ()),
            Entry::new("alfa/tool", Manifest::new(json!({})), ()),
            Entry::new("main/tool-extra", Manifest::new(json!({})), ()),
            Entry::new("tool/other", Manifest::new(json!({})), ()),
            Entry::new("main/x", Manifest::new(json!({"description": "tool"})), ()),
        ];
        let found = rank_entries(&rows, "tool", &none()).unwrap();
        let order: Vec<&str> = found.iter().map(|r| r.key.as_str()).collect();
        // Bucket, luego app exacta, luego la clave; la descripción queda al final.
        assert_eq!(
            order,
            [
                "tool/other",
                "alfa/tool",
                "zeta/tool",
                "main/tool-extra",
                "main/x"
            ]
        );
    }

    #[test]
    fn version_y_notas_cuentan_y_los_valores_raros_se_convierten_como_python() {
        let rows = vec![
            Entry::new(
                "main/a",
                Manifest::new(json!({"version": 21.5, "notes": ["Usa Rust", "y mas"]})),
                (),
            ),
            Entry::new("main/b", Manifest::new(json!({"description": null})), ()),
            Entry::new("main/c", Manifest::new(json!(5)), ()),
        ];
        assert_eq!(
            rank_entries(&rows, "rust", &none()).unwrap()[0].key,
            "main/a"
        );
        assert_eq!(
            rank_entries(&rows, "21 5", &none()).unwrap()[0].key,
            "main/a"
        );
        // `description: null` se convierte con `str(None)`.
        assert_eq!(
            rank_entries(&rows, "none", &none()).unwrap()[0].key,
            "main/b"
        );
    }

    #[test]
    fn una_clave_sin_barra_falla_solo_con_palabras() {
        let rows = vec![Entry::new("sinbarra", Manifest::new(json!({})), ())];
        assert_eq!(
            rank_entries(&rows, "x", &none()).unwrap_err().to_string(),
            "Clave de catálogo no válida: sinbarra"
        );
        assert_eq!(rank_entries(&rows, "", &none()).unwrap().len(), 1);
    }

    #[test]
    fn claves_repetidas_no_se_deduplican() {
        let rows = vec![
            Entry::new("main/a", Manifest::new(json!({})), 1),
            Entry::new("main/a", Manifest::new(json!({})), 2),
        ];
        let found = rank_entries(&rows, "a", &none()).unwrap();
        assert_eq!(found.iter().map(|r| r.status).collect::<Vec<_>>(), [1, 2]);
    }
}
