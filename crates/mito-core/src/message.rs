//! Mensajes de usuario traducibles.
//!
//! Igual que en el original, la clave de traducción es la plantilla en español y los
//! argumentos se interpolan **después** de traducir. Un [`Message`] guarda la plantilla
//! y sus argumentos sin traducir; `mito-i18n` la traduce en el borde (TUI), y su
//! [`Display`](std::fmt::Display) produce el texto español (CLI y log).

use std::borrow::Cow;
use std::fmt::{self, Write as _};

/// Plantilla de mensaje con argumentos con nombre (`{p1}`, `{name}`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    template: Cow<'static, str>,
    args: Vec<(Cow<'static, str>, String)>,
    suffix: String,
}

impl Message {
    /// Crea un mensaje sin argumentos.
    pub fn new(template: impl Into<Cow<'static, str>>) -> Self {
        Self {
            template: template.into(),
            args: Vec::new(),
            suffix: String::new(),
        }
    }

    /// Añade un texto que se concatena **sin traducir** tras la traducción.
    ///
    /// El original escribe `tr('Variable reservada por MITO: ') + key`: la clave de
    /// traducción termina en `: ` y el dato se pega después. Esto lo reproduce sin
    /// inventar claves que no están en los catálogos.
    #[must_use]
    pub fn with_suffix(mut self, suffix: impl fmt::Display) -> Self {
        self.suffix = suffix.to_string();
        self
    }

    /// Texto añadido tras la traducción (vacío si no hay).
    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// Añade un argumento con nombre. Un nombre repetido sustituye al anterior.
    #[must_use]
    pub fn arg(mut self, name: impl Into<Cow<'static, str>>, value: impl fmt::Display) -> Self {
        let name = name.into();
        let value = value.to_string();
        if let Some(slot) = self.args.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = value;
        } else {
            self.args.push((name, value));
        }
        self
    }

    /// Plantilla original en español: es la clave de traducción.
    pub fn template(&self) -> &str {
        &self.template
    }

    /// Argumentos en el orden en que se añadieron.
    pub fn args(&self) -> &[(Cow<'static, str>, String)] {
        &self.args
    }

    /// Interpola los argumentos en `template` (normalmente una traducción de
    /// [`Self::template`]).
    ///
    /// Sigue `str.format` de Python: `{nombre}` se sustituye, `{{` y `}}` son llaves
    /// literales. Devuelve `None` si la plantilla usa un nombre sin argumento o tiene
    /// llaves desequilibradas, para que el traductor pueda usar el siguiente fallback.
    /// El [`suffix`](Self::suffix), si existe, se añade al final.
    pub fn render_with(&self, template: &str) -> Option<String> {
        let mut out = String::with_capacity(template.len() + self.suffix.len());
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    out.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    out.push('}');
                }
                '{' => {
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(ch) => name.push(ch),
                            None => return None,
                        }
                    }
                    let (_, value) = self.args.iter().find(|(n, _)| *n == name)?;
                    out.push_str(value);
                }
                '}' => return None,
                other => out.push(other),
            }
        }
        out.push_str(&self.suffix);
        Some(out)
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.render_with(&self.template) {
            Some(text) => f.write_str(&text),
            None => write!(f, "{}{}", self.template, self.suffix),
        }
    }
}

/// Representación de una cadena como la produce `repr()` de Python.
///
/// El original usa `f'{value!r}'` en varios mensajes (`Nombre no válido: 'a b'`); se
/// replica para que los textos coincidan.
pub fn py_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => {
                let code = u32::from(c);
                // Escribir en un String no puede fallar.
                let _ = if code <= 0xff {
                    write!(out, "\\x{code:02x}")
                } else {
                    write!(out, "\\u{code:04x}")
                };
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpola_argumentos_con_nombre() {
        let message = Message::new("Profile {name} creado en {p1}")
            .arg("name", "dev")
            .arg("p1", "C:\\mito");
        assert_eq!(message.to_string(), "Profile dev creado en C:\\mito");
    }

    #[test]
    fn las_llaves_dobles_son_literales() {
        let message = Message::new("{{x}} = {v}").arg("v", 1);
        assert_eq!(message.to_string(), "{x} = 1");
    }

    #[test]
    fn una_traduccion_con_parametros_distintos_no_se_puede_usar() {
        let message = Message::new("Hola {name}").arg("name", "Ana");
        assert_eq!(message.render_with("Hello {nombre}"), None);
        assert_eq!(message.render_with("Hello {name"), None);
        assert_eq!(
            message.render_with("Hello {name}"),
            Some("Hello Ana".to_owned())
        );
    }

    #[test]
    fn el_sufijo_se_pega_sin_traducir() {
        let message = Message::new("Variable reservada por MITO: ").with_suffix("PATH {x}");
        assert_eq!(message.to_string(), "Variable reservada por MITO: PATH {x}");
        assert_eq!(
            message.render_with("Reserved by MITO: "),
            Some("Reserved by MITO: PATH {x}".to_owned())
        );
    }

    #[test]
    fn sin_argumentos_la_plantilla_se_muestra_tal_cual() {
        assert_eq!(
            Message::new("Instalación cancelada").to_string(),
            "Instalación cancelada"
        );
    }

    #[test]
    fn repr_como_python() {
        assert_eq!(py_repr("bad name"), "'bad name'");
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr("a\\b\n"), "'a\\\\b\\n'");
        assert_eq!(py_repr("\u{1}"), "'\\x01'");
        assert_eq!(py_repr("ñ"), "'ñ'");
    }
}
