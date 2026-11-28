//! `html.unescape` para los valores de atributo del formulario del portal.
//!
//! Soporta todas las referencias numéricas con las reglas de Python (`html._replace_charref`)
//! y las referencias con nombre de HTML 4 (Latin-1, `quot`, `amp`, `lt`, `gt`, `apos` y los
//! signos tipográficos habituales). Una referencia con nombre desconocida se deja tal cual,
//! que es lo que hace `html.unescape` con las que no conoce.

/// Nombres de los caracteres Latin-1 `U+00A0`..`U+00FF`, en orden.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

/// Nombres que `html.unescape` acepta también sin `;` final.
const LEGACY: [(&str, char); 8] = [
    ("amp", '&'),
    ("AMP", '&'),
    ("lt", '<'),
    ("LT", '<'),
    ("gt", '>'),
    ("GT", '>'),
    ("quot", '"'),
    ("QUOT", '"'),
];

/// Nombres que solo se aceptan con `;`.
const NAMED: [(&str, char); 31] = [
    ("apos", '\''),
    ("OElig", '\u{152}'),
    ("oelig", '\u{153}'),
    ("Scaron", '\u{160}'),
    ("scaron", '\u{161}'),
    ("Yuml", '\u{178}'),
    ("circ", '\u{2C6}'),
    ("tilde", '\u{2DC}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("zwnj", '\u{200C}'),
    ("zwj", '\u{200D}'),
    ("lrm", '\u{200E}'),
    ("rlm", '\u{200F}'),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("sbquo", '\u{201A}'),
    ("ldquo", '\u{201C}'),
    ("rdquo", '\u{201D}'),
    ("bdquo", '\u{201E}'),
    ("dagger", '\u{2020}'),
    ("Dagger", '\u{2021}'),
    ("permil", '\u{2030}'),
    ("lsaquo", '\u{2039}'),
    ("rsaquo", '\u{203A}'),
    ("euro", '\u{20AC}'),
    ("trade", '\u{2122}'),
    ("hellip", '\u{2026}'),
];

/// Carácter de una referencia con nombre. `key` incluye el `;` si lo hubo.
fn lookup(key: &str) -> Option<char> {
    let (name, semicolon) = match key.strip_suffix(';') {
        Some(name) => (name, true),
        None => (key, false),
    };
    let latin = LATIN1
        .iter()
        .position(|candidate| *candidate == name)
        .and_then(|index| char::from_u32(0xA0 + u32::try_from(index).ok()?));
    let upper = match name {
        "COPY" => Some('\u{A9}'),
        "REG" => Some('\u{AE}'),
        _ => None,
    };
    let legacy = LEGACY.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
    if let Some(found) = latin.or(legacy).or(upper) {
        return Some(found);
    }
    if semicolon {
        NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
    } else {
        None
    }
}

/// Windows-1252 para `U+0080`..`U+009F` (`_invalid_charrefs`).
fn windows_1252(number: u32) -> char {
    let mapped = match number {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        other => other,
    };
    char::from_u32(mapped).unwrap_or('\u{FFFD}')
}

/// Texto de una referencia numérica (`html._replace_charref`).
fn charref(number: u32) -> String {
    match number {
        0 | 0xD800..=0xDFFF | 0x0011_0000.. => "\u{FFFD}".to_owned(),
        0x0D => "\r".to_owned(),
        0x80..=0x9F => windows_1252(number).to_string(),
        0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F | 0xFDD0..=0xFDEF => String::new(),
        n if n & 0xFFFE == 0xFFFE => String::new(),
        n => char::from_u32(n).map(String::from).unwrap_or_default(),
    }
}

/// `html.unescape`.
pub(crate) fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let (replacement, used) = reference(rest);
        out.push_str(&replacement);
        rest = &rest[used..];
    }
    out.push_str(rest);
    out
}

/// Reemplazo y bytes consumidos de la referencia que empieza en `text` (que empieza por `&`).
fn reference(text: &str) -> (String, usize) {
    let body = &text[1..];
    if let Some(numeric) = body.strip_prefix('#') {
        let (digits, radix, skip) = match numeric.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16, 2),
            None => (numeric, 10, 1),
        };
        let length = digits
            .bytes()
            .take_while(|b| (*b as char).is_digit(radix))
            .count();
        if length == 0 {
            return ("&".to_owned(), 1);
        }
        let number = u32::from_str_radix(&digits[..length], radix).unwrap_or(u32::MAX);
        let semicolon = usize::from(digits[length..].starts_with(';'));
        return (charref(number), 1 + skip + length + semicolon);
    }
    let name: String = body
        .chars()
        .take_while(|c| !matches!(c, '\t' | '\n' | '\u{c}' | ' ' | '<' | '&' | '#' | ';'))
        .take(32)
        .collect();
    if name.is_empty() {
        return ("&".to_owned(), 1);
    }
    let with_semicolon = body[name.len()..].starts_with(';');
    let key = if with_semicolon {
        format!("{name};")
    } else {
        name.clone()
    };
    if let Some(found) = lookup(&key) {
        return (found.to_string(), 1 + key.len());
    }
    // Prefijo más largo que sea un nombre sin `;` (`&ampfoo` → `&foo`).
    let chars: Vec<(usize, char)> = key.char_indices().collect();
    for cut in (2..chars.len()).rev() {
        let end = chars[cut].0;
        if let Some(found) = lookup(&key[..end]) {
            return (found.to_string(), 1 + end);
        }
    }
    ("&".to_owned(), 1)
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn decodifica_como_html_unescape() {
        let cases = [
            ("t&amp;k", "t&k"),
            ("/a?x=1&amp;y=2", "/a?x=1&y=2"),
            ("&#x41;&#66;&#X43;", "ABC"),
            ("&lt;b&gt;&quot;&apos;", "<b>\"'"),
            ("&amp", "&"),
            ("&ampfoo", "&foo"),
            ("&notit;", "\u{ac}it;"),
            ("&copy 2024", "\u{a9} 2024"),
            ("&nosuch; y &", "&nosuch; y &"),
            ("a & b &#; &#x;", "a & b &#; &#x;"),
            ("&#0;&#x110000;&#xD800;", "\u{fffd}\u{fffd}\u{fffd}"),
            ("&#128;&#150;", "\u{20ac}\u{2013}"),
            ("&#1;&#xFFFE;", ""),
            ("&#99999999999999999999;", "\u{fffd}"),
            ("sin entidades", "sin entidades"),
            ("&eacute;&mdash;", "\u{e9}\u{2014}"),
            ("&mdash", "&mdash"),
        ];
        for (input, expected) in cases {
            assert_eq!(unescape(input), expected, "{input}");
        }
    }
}
