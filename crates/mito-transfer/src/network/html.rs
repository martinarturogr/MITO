//! Formularios con contraseña de una página de portal (`Form` de `network.py`).
//!
//! Analizador HTML propio y tolerante, con las reglas de `html.parser`: etiquetas y
//! atributos en minúsculas, valores con comillas dobles, simples, sin comillas o sin
//! valor, entidades decodificadas como `html.unescape`, y comentarios, declaraciones y
//! contenido de `script`, `style`, `textarea` y `title` ignorados. Nunca entra en pánico.

use super::entities::unescape;

/// Formulario con contraseña (`Form`): campos en orden de inserción, sin duplicados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalForm {
    /// Atributo `action` (vacío si falta).
    pub action: String,
    /// Campos con nombre y su valor publicado. Los de contraseña llevan siempre `""`.
    pub fields: Vec<(String, String)>,
    /// Nombre del primer campo `text` o `email` (vacío si no hay).
    pub user_field: String,
    /// Nombre del primer campo de contraseña.
    pub password_field: String,
}

/// Formularios con contraseña de `html` (`Form(password_field).feed`).
///
/// Un campo cuenta como contraseña si su `name` es `password_field` o su `type` es
/// `password`. Respecto al original, un atributo sin valor es `""` (F8) y el valor
/// publicado en un campo de contraseña se descarta (F9).
pub fn portal_forms(html: &str, password_field: &str) -> Vec<PortalForm> {
    let mut collector = Collector {
        password_field,
        current: None,
        forms: Vec::new(),
    };
    scan(html, &mut |event| collector.event(event));
    collector.forms
}

struct Open {
    form: PortalForm,
    has_password: bool,
}

struct Collector<'a> {
    password_field: &'a str,
    current: Option<Open>,
    forms: Vec<PortalForm>,
}

enum Event {
    Start {
        name: String,
        attrs: Vec<(String, Option<String>)>,
        /// `<etiqueta ... />`
        empty: bool,
    },
    End(String),
}

fn attribute(attrs: &[(String, Option<String>)], key: &str) -> Option<String> {
    // `dict(attrs)`: la última repetición gana.
    attrs
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone().unwrap_or_default())
}

impl Collector<'_> {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start { name, attrs, empty } => {
                self.start(&name, &attrs);
                if empty {
                    self.end(&name);
                }
            }
            Event::End(name) => self.end(&name),
        }
    }

    fn start(&mut self, name: &str, attrs: &[(String, Option<String>)]) {
        match name {
            "form" => {
                self.current = Some(Open {
                    form: PortalForm {
                        action: attribute(attrs, "action").unwrap_or_default(),
                        fields: Vec::new(),
                        user_field: String::new(),
                        password_field: String::new(),
                    },
                    has_password: false,
                });
            }
            "input" => {
                let Some(open) = &mut self.current else {
                    return;
                };
                let Some(field) = attribute(attrs, "name").filter(|n| !n.is_empty()) else {
                    return;
                };
                let kind = attribute(attrs, "type")
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "text".to_owned())
                    .to_lowercase();
                let password = field == self.password_field || kind == "password";
                let value = if password {
                    String::new()
                } else {
                    attribute(attrs, "value").unwrap_or_default()
                };
                match open.form.fields.iter_mut().find(|(n, _)| *n == field) {
                    Some(slot) => slot.1 = value,
                    None => open.form.fields.push((field.clone(), value)),
                }
                if password {
                    open.has_password = true;
                    if open.form.password_field.is_empty() {
                        open.form.password_field = field;
                    }
                } else if open.form.user_field.is_empty()
                    && matches!(kind.as_str(), "text" | "email")
                {
                    open.form.user_field = field;
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if name == "form"
            && let Some(open) = self.current.take()
            && open.has_password
        {
            self.forms.push(open.form);
        }
    }
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

/// Elementos cuyo contenido no se interpreta como marcado.
fn raw_text(name: &str) -> bool {
    matches!(name, "script" | "style" | "textarea" | "title")
}

fn position(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

/// Recorre `html` emitiendo etiquetas de inicio y de fin.
fn scan(html: &str, sink: &mut dyn FnMut(Event)) {
    let bytes = html.as_bytes();
    let mut at = 0;
    while let Some(lt) = bytes
        .get(at..)
        .and_then(|rest| rest.iter().position(|b| *b == b'<'))
        .map(|offset| offset + at)
    {
        at = lt;
        let rest = &bytes[at..];
        if rest.starts_with(b"<!--") {
            let body = at + 4;
            at = match comment_end(bytes, body) {
                Some(end) => end,
                None => return,
            };
        } else if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            match position(bytes, at, b">") {
                Some(end) => at = end + 1,
                None => return,
            }
        } else if rest.starts_with(b"</") {
            let after = rest.get(2).copied();
            if after.is_some_and(|b| b.is_ascii_alphabetic()) {
                let start = at + 2;
                let stop = bytes[start..]
                    .iter()
                    .position(|b| is_space(*b) || matches!(b, b'/' | b'>'))
                    .map_or(bytes.len(), |p| p + start);
                let Some(close) = position(bytes, stop, b">") else {
                    return;
                };
                sink(Event::End(html[start..stop].to_ascii_lowercase()));
                at = close + 1;
            } else {
                match position(bytes, at, b">") {
                    Some(end) => at = end + 1,
                    None => return,
                }
            }
        } else if rest.get(1).is_some_and(u8::is_ascii_alphabetic) {
            let Some((event, end)) = start_tag(html, at) else {
                return;
            };
            let raw = match &event {
                Event::Start { name, empty, .. } if !empty && raw_text(name) => Some(name.clone()),
                _ => None,
            };
            sink(event);
            at = end;
            if let Some(name) = raw {
                match raw_end(bytes, at, &name) {
                    Some(close) => at = close,
                    None => return,
                }
            }
        } else {
            at += 1;
        }
    }
}

/// Fin de un comentario que empieza en `body` (tras `<!--`): `-->`, `--!>`, `<!-->` o `<!--->`.
fn comment_end(bytes: &[u8], body: usize) -> Option<usize> {
    if bytes.get(body) == Some(&b'>') {
        return Some(body + 1);
    }
    if bytes[body..].starts_with(b"->") {
        return Some(body + 2);
    }
    let plain = position(bytes, body, b"-->").map(|p| p + 3);
    let bang = position(bytes, body, b"--!>").map(|p| p + 4);
    match (plain, bang) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Posición del `</nombre` que cierra un elemento de texto crudo (sin distinguir mayúsculas).
fn raw_end(bytes: &[u8], from: usize, name: &str) -> Option<usize> {
    let mut at = from;
    while let Some(lt) = position(bytes, at, b"</") {
        let tag_start = lt + 2;
        let tag_end = tag_start + name.len();
        let matches_name = bytes
            .get(tag_start..tag_end)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name.as_bytes()));
        if matches_name
            && bytes
                .get(tag_end)
                .is_none_or(|b| is_space(*b) || matches!(b, b'/' | b'>'))
        {
            return Some(lt);
        }
        at = lt + 2;
    }
    None
}

/// Etiqueta de inicio que empieza en `at` (un `<` seguido de una letra) y posición tras `>`.
/// `None` si la etiqueta no termina (el resto es texto).
fn start_tag(html: &str, at: usize) -> Option<(Event, usize)> {
    let bytes = html.as_bytes();
    let name_start = at + 1;
    let mut pos = bytes[name_start..]
        .iter()
        .position(|b| is_space(*b) || matches!(b, b'/' | b'>' | 0))
        .map_or(bytes.len(), |p| p + name_start);
    let name = html[name_start..pos].to_ascii_lowercase();
    let mut attrs: Vec<(String, Option<String>)> = Vec::new();
    loop {
        while bytes.get(pos).is_some_and(|b| is_space(*b)) {
            pos += 1;
        }
        match bytes.get(pos)? {
            b'>' => {
                return Some((
                    Event::Start {
                        name,
                        attrs,
                        empty: false,
                    },
                    pos + 1,
                ));
            }
            b'/' => {
                if bytes.get(pos + 1) == Some(&b'>') {
                    return Some((
                        Event::Start {
                            name,
                            attrs,
                            empty: true,
                        },
                        pos + 2,
                    ));
                }
                pos += 1;
                continue;
            }
            _ => {}
        }
        // Nombre del atributo: el primer carácter puede ser `=`.
        let key_start = pos;
        pos += 1;
        while bytes
            .get(pos)
            .is_some_and(|b| !is_space(*b) && !matches!(b, b'/' | b'=' | b'>'))
        {
            pos += 1;
        }
        let key = html[key_start..pos].to_ascii_lowercase();
        let mut look = pos;
        while bytes.get(look).is_some_and(|b| is_space(*b)) {
            look += 1;
        }
        let mut value = None;
        if bytes.get(look) == Some(&b'=') {
            while bytes.get(look).is_some_and(|b| *b == b'=' || is_space(*b)) {
                look += 1;
            }
            let first = *bytes.get(look)?;
            if matches!(first, b'"' | b'\'') {
                let start = look + 1;
                let close = bytes[start..].iter().position(|b| *b == first)? + start;
                value = Some(unescape(&html[start..close]));
                pos = close + 1;
            } else {
                let end = bytes[look..]
                    .iter()
                    .position(|b| is_space(*b) || *b == b'>')
                    .map_or(bytes.len(), |p| p + look);
                value = Some(unescape(&html[look..end]));
                pos = end;
            }
        }
        attrs.push((key, value));
    }
}

#[cfg(test)]
mod tests;
