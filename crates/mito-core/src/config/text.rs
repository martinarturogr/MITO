//! Lectura y reescritura de `config.properties`.
//!
//! El formato es el del original (`configuration.parse`): una línea `clave=valor`,
//! sin escapes ni continuaciones, con `#` y `!` como comentario. Se implementa a mano
//! porque los crates de `.properties` usan `:`, escapes y continuaciones.

/// Valores de `config.properties` (o de `env` de un profile) en orden de inserción.
///
/// Una clave repetida conserva la posición de su primera aparición y el valor de la
/// última, como un `dict` de Python.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    entries: Vec<(String, String)>,
}

impl Values {
    /// Valores vacíos.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserta o sustituye (manteniendo la posición) un valor.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// Valor de `key`, si está.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Elimina `key` y devuelve su valor.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    /// ¿Existe `key`?
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Pares en orden de inserción.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Número de claves.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// ¿No hay ninguna clave?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Variables libres `env.<NOMBRE>` sin el prefijo, en orden.
    pub fn env(&self) -> Vec<(String, String)> {
        self.iter()
            .filter_map(|(k, v)| k.strip_prefix("env.").map(|n| (n.to_owned(), v.to_owned())))
            .collect()
    }

    /// Valor de una propiedad reconocida o, si falta, su valor por defecto.
    ///
    /// Devuelve `None` si la clave falta y no es una propiedad reconocida.
    pub fn effective(&self, key: &str) -> Option<&str> {
        self.get(key)
            .or_else(|| super::properties::lookup(key).map(|p| p.default))
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for Values {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut values = Self::new();
        for (key, value) in iter {
            values.insert(key, value);
        }
        values
    }
}

/// `str.strip()` de Python: además del espacio Unicode recorta `\x1c`–`\x1f`.
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
}

/// `str.splitlines()` de Python: separa en `\n`, `\r`, `\r\n`, `\v`, `\f`, `\x1c`–`\x1e`,
/// `\x85`, `U+2028` y `U+2029`, sin línea final vacía.
pub(crate) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(&text[start..index]);
            start = index + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n') {
                chars.next();
                start += 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Interpreta una línea: `Some((clave, valor))` si es `clave=valor` activo.
///
/// Se recorta la línea, la clave y el valor; el primer `=` separa y el resto es el valor.
pub(crate) fn parse_line(line: &str) -> Option<(&str, &str)> {
    let line = py_strip(line);
    if line.is_empty() || line.starts_with(['#', '!']) {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    Some((py_strip(key), py_strip(value)))
}

/// Interpreta el texto de `config.properties`. Nunca falla: ignora lo que no es
/// `clave=valor`. Gana la última aparición de una clave repetida.
pub fn parse(text: &str) -> Values {
    splitlines(text)
        .into_iter()
        .filter_map(parse_line)
        .collect()
}

/// Decodifica UTF-8 tolerando un BOM inicial (`utf-8-sig`). `None` si no es UTF-8.
pub(crate) fn decode(bytes: &[u8]) -> Option<&str> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).ok()
}

/// Reescribe `original` con los valores `values`, conservando comentarios, líneas
/// vacías y líneas sin `=`.
///
/// Reproduce `Configuration.save`: cada línea con clave se reemplaza por
/// `clave=valor` si la clave sigue en `values` (una sola vez) y se elimina si no; las
/// claves nuevas se añaden al final en orden. El texto termina siempre en `\n`.
pub fn render(original: &str, values: &Values) -> String {
    let mut pending = values.clone();
    let mut lines: Vec<String> = Vec::new();
    for line in splitlines(original) {
        match parse_line(line) {
            None => lines.push(line.to_owned()),
            Some((key, _)) => {
                if let Some(value) = pending.remove(key) {
                    lines.push(format!("{key}={value}"));
                }
            }
        }
    }
    lines.extend(pending.iter().map(|(k, v)| format!("{k}={v}")));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_ignora_comentarios_vacias_y_lineas_sin_igual() {
        let values = parse("# c\n! c2\n\nsin igual\n  a = 1  \nb=x=y\nc=\n");
        assert_eq!(
            values.iter().collect::<Vec<_>>(),
            [("a", "1"), ("b", "x=y"), ("c", "")]
        );
    }

    #[test]
    fn parse_no_admite_dos_puntos_escapes_ni_comentarios_de_fin_de_linea() {
        let values = parse("a:1\nb=\\n valor # texto\nc=\\\nd=2");
        assert_eq!(values.get("a"), None);
        assert_eq!(values.get("b"), Some("\\n valor # texto"));
        assert_eq!(values.get("c"), Some("\\"));
        assert_eq!(values.get("d"), Some("2"));
    }

    #[test]
    fn la_ultima_clave_repetida_gana_y_conserva_la_primera_posicion() {
        let values = parse("a=1\nb=2\na=3\n");
        assert_eq!(values.iter().collect::<Vec<_>>(), [("a", "3"), ("b", "2")]);
    }

    #[test]
    fn splitlines_como_python() {
        assert_eq!(splitlines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n"), ["a"]);
        assert_eq!(splitlines("a\n\n"), ["a", ""]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(
            splitlines("a\u{b}b\u{c}c\u{85}d\u{2028}e"),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(
            splitlines("a\u{1c}b\u{1d}c\u{1e}d\u{2029}e"),
            ["a", "b", "c", "d", "e"]
        );
    }

    #[test]
    fn strip_como_python() {
        assert_eq!(py_strip("\u{1f} a \u{a0}"), "a");
        assert_eq!(py_strip(""), "");
    }

    #[test]
    fn un_bom_inicial_se_tolera_al_leer() {
        assert_eq!(decode(b"\xEF\xBB\xBFa=1"), Some("a=1"));
        assert_eq!(decode(b"\xFF"), None);
    }

    #[test]
    fn render_conserva_comentarios_reemplaza_borra_y_anade() {
        let original = "# My configuration\n\nbucket.main=C:/catalog\nenv.OLD=old\nunknown.setting=keep\nsuelta\n";
        let mut values = parse(original);
        values.remove("env.OLD");
        values.insert("env.TOKEN", "a=b");
        values.insert("bucket.main", "otro");
        assert_eq!(
            render(original, &values),
            "# My configuration\n\nbucket.main=otro\nunknown.setting=keep\nsuelta\nenv.TOKEN=a=b\n"
        );
    }

    #[test]
    fn render_elimina_las_lineas_repetidas_y_normaliza_el_salto_final() {
        let values: Values = [("a", "2")].into_iter().collect();
        assert_eq!(render("a=1\r\n# x\r\na=3", &values), "a=2\n# x\n");
        assert_eq!(render("", &Values::new()), "\n");
    }

    proptest! {
        #[test]
        fn render_y_parse_son_coherentes(
            original in "[a-c=# \\n]{0,40}",
            pairs in proptest::collection::vec(("[a-z.]{1,6}", "[a-z =]{0,6}"), 0..5),
        ) {
            let values: Values = pairs
                .iter()
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .filter(|(k, _)| !k.is_empty())
                .collect();
            let text = render(&original, &values);
            prop_assert_eq!(parse(&text), values);
            prop_assert!(text.ends_with('\n'));
        }
    }
}
