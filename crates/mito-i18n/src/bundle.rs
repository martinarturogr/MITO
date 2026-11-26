//! Catálogos de traducción incrustados en el binario.
//!
//! Cada idioma es un JSON plano `{"<plantilla española>": "<texto traducido>"}`. Al
//! cargarlo se conservan solo las entradas cuyo valor es una cadena y cuyos campos
//! `{nombre}` coinciden con los de la clave; las demás se descartan sin error.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use serde_json::Value;

use crate::{DEFAULT_LANGUAGE, LANGUAGES};

/// Catálogos de los 24 idiomas, en el orden de [`LANGUAGES`].
const EMBEDDED: [&str; 24] = [
    include_str!("../locales/español.json"),
    include_str!("../locales/english.json"),
    include_str!("../locales/български.json"),
    include_str!("../locales/hrvatski.json"),
    include_str!("../locales/čeština.json"),
    include_str!("../locales/dansk.json"),
    include_str!("../locales/nederlands.json"),
    include_str!("../locales/eesti.json"),
    include_str!("../locales/suomi.json"),
    include_str!("../locales/français.json"),
    include_str!("../locales/deutsch.json"),
    include_str!("../locales/ελληνικά.json"),
    include_str!("../locales/magyar.json"),
    include_str!("../locales/gaeilge.json"),
    include_str!("../locales/italiano.json"),
    include_str!("../locales/latviešu.json"),
    include_str!("../locales/lietuvių.json"),
    include_str!("../locales/malti.json"),
    include_str!("../locales/polski.json"),
    include_str!("../locales/português.json"),
    include_str!("../locales/română.json"),
    include_str!("../locales/slovenčina.json"),
    include_str!("../locales/slovenščina.json"),
    include_str!("../locales/svenska.json"),
];

/// Nombres de los campos `{nombre}` de una plantilla (`Formatter().parse`).
///
/// `{{` y `}}` son llaves literales; el nombre llega hasta `!` o `:`. Devuelve `None`
/// si la plantilla está mal formada (llave sin cerrar o `}` suelta).
pub fn fields(template: &str) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '}' => return None,
            '{' => {
                let mut depth = 1;
                let mut inner = String::new();
                for next in chars.by_ref() {
                    match next {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    inner.push(next);
                }
                if depth != 0 {
                    return None;
                }
                let name = inner.split(['!', ':']).next().unwrap_or_default();
                names.insert(name.to_owned());
            }
            _ => {}
        }
    }
    Some(names)
}

/// Catálogo de un idioma: plantilla española → texto traducido.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bundle {
    messages: HashMap<String, String>,
}

impl Bundle {
    /// Catálogo vacío.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Interpreta un JSON de catálogo. Nunca falla: un JSON inválido o que no es un
    /// objeto da un catálogo vacío, y las entradas no válidas se descartan.
    pub fn parse(json: &str) -> Self {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(json) else {
            return Self::empty();
        };
        let messages = data
            .into_iter()
            .filter_map(|(key, value)| {
                let Value::String(text) = value else {
                    return None;
                };
                let (a, b) = (fields(&key)?, fields(&text)?);
                (a == b).then_some((key, text))
            })
            .collect();
        Self { messages }
    }

    /// Traducción de una plantilla, si el catálogo la tiene.
    pub fn get(&self, template: &str) -> Option<&str> {
        self.messages.get(template).map(String::as_str)
    }

    /// Número de entradas válidas.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// JSON incrustado de un idioma, o del inglés si `language` no es uno de los 24.
pub fn catalog_json(language: &str) -> &'static str {
    let index = LANGUAGES
        .iter()
        .position(|l| *l == language)
        .or_else(|| LANGUAGES.iter().position(|l| *l == DEFAULT_LANGUAGE))
        .unwrap_or(0);
    EMBEDDED[index]
}

/// Catálogo incrustado de `language`, validado una sola vez por proceso. Un idioma
/// fuera de [`LANGUAGES`] da el catálogo en inglés (nunca se lee un archivo arbitrario).
pub fn bundle(language: &str) -> &'static Bundle {
    static CACHE: [OnceLock<Bundle>; 24] = [const { OnceLock::new() }; 24];
    let index = LANGUAGES
        .iter()
        .position(|l| *l == language)
        .or_else(|| LANGUAGES.iter().position(|l| *l == DEFAULT_LANGUAGE))
        .unwrap_or(0);
    CACHE[index].get_or_init(|| Bundle::parse(EMBEDDED[index]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(template: &str) -> Vec<String> {
        fields(template).unwrap().into_iter().collect()
    }

    #[test]
    fn campos_de_una_plantilla() {
        assert_eq!(names("Profile {p1} creado"), ["p1"]);
        assert_eq!(names("{p0}: {p2}"), ["p0", "p2"]);
        assert_eq!(names("{{literal}} y {a!r} {b:>5}"), ["a", "b"]);
        assert_eq!(names("sin campos"), Vec::<String>::new());
        assert_eq!(names("{p1} {p1}"), names("{p1}"));
        assert_eq!(names("{}"), [""]);
        assert!(fields("{abierta").is_none());
        assert!(fields("cerrada}").is_none());
    }

    #[test]
    fn solo_se_conservan_entradas_validas() {
        let bundle = Bundle::parse(
            r#"{"Abrir": 42, "Profile {p1} creado": "Profile {missing}", "Cerrar": "Close",
                "Mal {": "x", "Rota": "Y {"}"#,
        );
        assert_eq!(bundle.len(), 1);
        assert_eq!(bundle.get("Cerrar"), Some("Close"));
        assert_eq!(bundle.get("Abrir"), None);
        assert_eq!(bundle.get("Profile {p1} creado"), None);
    }

    #[test]
    fn json_invalido_o_que_no_es_objeto_da_un_catalogo_vacio() {
        for bad in ["", "no es json", "[]", "\"x\"", "42", "null"] {
            assert!(Bundle::parse(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn un_idioma_desconocido_usa_el_ingles() {
        let english = bundle("english");
        assert!(std::ptr::eq(bundle("../../outside"), english));
        assert!(std::ptr::eq(bundle("french"), english));
        assert!(std::ptr::eq(bundle("ENGLISH"), english));
        assert!(!std::ptr::eq(bundle("español"), english));
        assert_eq!(english.get("Abrir"), Some("Open"));
        assert_eq!(catalog_json("../x"), catalog_json("english"));
    }
}
