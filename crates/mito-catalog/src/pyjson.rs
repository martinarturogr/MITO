//! Reproducción de `json.dumps`, `str()` y `repr()` de Python sobre valores JSON.
//!
//! El original calcula huellas y claves de estado con `json.dumps` y muestra valores
//! con `str()`. Para que una instancia creada por el original no vea sus componentes
//! como cambiados (ADR 0008, punto 9), aquí se imita su salida exacta: escape ASCII
//! (`\uXXXX` con pares sustitutos), separadores configurables, orden de claves por punto
//! de código y la representación `repr` de los números de coma flotante.
//!
//! Límite conocido: `serde_json` (sin `arbitrary_precision`) convierte a `f64` los
//! enteros que no caben en 64 bits, mientras Python los conserva. Ningún manifiesto
//! Scoop real los usa.

use std::fmt::Write as _;

use mito_core::message::py_repr as py_str_repr;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Opciones de [`dumps`] equivalentes a los argumentos de `json.dumps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpOptions {
    /// Separador entre elementos (`separators[0]`).
    pub item_separator: &'static str,
    /// Separador entre clave y valor (`separators[1]`).
    pub key_separator: &'static str,
    /// Ordenar las claves de los objetos por punto de código (`sort_keys`).
    pub sort_keys: bool,
    /// Escapar todo lo que no sea ASCII imprimible (`ensure_ascii`).
    pub ensure_ascii: bool,
}

impl DumpOptions {
    /// `json.dumps(v, sort_keys=True, separators=(',', ':'))`: la huella del manifiesto.
    pub const COMPACT_SORTED: Self = Self {
        item_separator: ",",
        key_separator: ":",
        sort_keys: true,
        ensure_ascii: true,
    };

    /// `json.dumps(v, sort_keys=True)`: separadores por defecto (`, ` y `: `).
    pub const DEFAULT_SORTED: Self = Self {
        item_separator: ", ",
        key_separator: ": ",
        sort_keys: true,
        ensure_ascii: true,
    };
}

/// Serializa `value` como lo haría `json.dumps` con `options`.
pub fn dumps(value: &Value, options: DumpOptions) -> String {
    let mut out = String::new();
    write_value(&mut out, value, options);
    out
}

/// SHA-256 en hexadecimal minúscula (`hashlib.sha256(...).hexdigest()`).
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, byte| {
            // Escribir en un String no puede fallar.
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn write_value(out: &mut String, value: &Value, options: DumpOptions) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number_repr(number)),
        Value::String(text) => write_string(out, text, options.ensure_ascii),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(options.item_separator);
                }
                write_value(out, item, options);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if options.sort_keys {
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push_str(options.item_separator);
                }
                write_string(out, key, options.ensure_ascii);
                out.push_str(options.key_separator);
                write_value(out, item, options);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, text: &str, ensure_ascii: bool) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            c if ensure_ascii || u32::from(c) < 0x20 => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    // Escribir en un String no puede fallar.
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn number_repr(number: &serde_json::Number) -> String {
    if let Some(integer) = number.as_i64() {
        integer.to_string()
    } else if let Some(integer) = number.as_u64() {
        integer.to_string()
    } else {
        number.as_f64().map_or_else(String::new, py_float_repr)
    }
}

/// `repr(float)` de Python: el menor número de dígitos que reproduce el valor, con
/// notación científica fuera de `1e-4 <= |x| < 1e16`.
pub fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned();
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let point = exponent + 1;
    let sign = if value < 0.0 { "-" } else { "" };
    let length = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    if -4 < point && point <= 16 {
        if point <= 0 {
            let zeros = "0".repeat(usize::try_from(-point).unwrap_or(0));
            format!("{sign}0.{zeros}{digits}")
        } else if point >= length {
            let zeros = "0".repeat(usize::try_from(point - length).unwrap_or(0));
            format!("{sign}{digits}{zeros}.0")
        } else {
            let (whole, fraction) = digits.split_at(usize::try_from(point).unwrap_or(0));
            format!("{sign}{whole}.{fraction}")
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{sign}{first}{fraction}e{exponent_sign}{:02}",
            exponent.abs()
        )
    }
}

/// Nombre del tipo de Python de un valor JSON (`type(x).__name__`).
pub fn py_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Valor de verdad de Python (`bool(x)`).
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_none_or(|f| f != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `repr(x)` de Python sobre un valor JSON decodificado.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number_repr(number),
        Value::String(text) => py_str_repr(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => py_dict_repr(map),
    }
}

fn py_dict_repr(map: &Map<String, Value>) -> String {
    let inner: Vec<String> = map
        .iter()
        .map(|(key, item)| format!("{}: {}", py_str_repr(key), py_repr(item)))
        .collect();
    format!("{{{}}}", inner.join(", "))
}

/// `str(x)` de Python: el texto tal cual para cadenas y `repr` para el resto.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => py_repr(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256(text: &str) -> String {
        sha256_hex(text.as_bytes())
    }

    /// Casos generados con Python 3.14: `json.dumps(json.loads(texto), sort_keys=True, …)`.
    const CASES: [(&str, &str, &str, &str, &str); 5] = [
        (
            r#"{"version":"1.0","url":"https://x/y.zip"}"#,
            r#"{"url":"https://x/y.zip","version":"1.0"}"#,
            "eca4f02a6930e1f070542b04f253b77e1030f903046250dca4407cf82791ffbb",
            r#"{"url": "https://x/y.zip", "version": "1.0"}"#,
            "dcf7b7bb00dee1d8e0d1d10c46cfafa9944fd811ea62f79c89e7256df6159553",
        ),
        (
            r#"{"version":"2","description":"Caf\u00e9 \u00f1 \u65e5\u672c \ud83d\ude00 \u007f \u0001 \" \\ / \n \t \b \f \r"}"#,
            r#"{"description":"Caf\u00e9 \u00f1 \u65e5\u672c \ud83d\ude00 \u007f \u0001 \" \\ / \n \t \b \f \r","version":"2"}"#,
            "9562d765ba8580173ce29184c234f7d88500e8687374ba675054a4e1a1ed7361",
            r#"{"description": "Caf\u00e9 \u00f1 \u65e5\u672c \ud83d\ude00 \u007f \u0001 \" \\ / \n \t \b \f \r", "version": "2"}"#,
            "9d7d5357f787ea5defc1a223fece46cd8c3cb509bd46a421ec69c5b19e4cb53c",
        ),
        (
            r#"{"b":[1,2.5,1e20,1e-7,0.1,true,false,null,{"z":1,"a":2}],"a":1.0,"c":-0.0,"d":1E2,"e":123456789012345678,"f":1e16,"g":1e15,"h":0.0001,"i":0.00001,"j":12345.678e3,"k":-1.5e-10,"l":5e-324,"m":1.7976931348623157e308}"#,
            r#"{"a":1.0,"b":[1,2.5,1e+20,1e-07,0.1,true,false,null,{"a":2,"z":1}],"c":-0.0,"d":100.0,"e":123456789012345678,"f":1e+16,"g":1000000000000000.0,"h":0.0001,"i":1e-05,"j":12345678.0,"k":-1.5e-10,"l":5e-324,"m":1.7976931348623157e+308}"#,
            "6b5052373179817f3e825d06134728b4d92db240e919cdc65b60ad4b1d07cb4e",
            r#"{"a": 1.0, "b": [1, 2.5, 1e+20, 1e-07, 0.1, true, false, null, {"a": 2, "z": 1}], "c": -0.0, "d": 100.0, "e": 123456789012345678, "f": 1e+16, "g": 1000000000000000.0, "h": 0.0001, "i": 1e-05, "j": 12345678.0, "k": -1.5e-10, "l": 5e-324, "m": 1.7976931348623157e+308}"#,
            "e9c5480925ca07711f53ea640f4681cc0be2af08bece558338543bf628fceb00",
        ),
        (
            r#"{"\u00e9":1,"z":2,"B":3,"a":4,"\uffff":5,"\ud83d\ude00":6,"":7}"#,
            r#"{"":7,"B":3,"a":4,"z":2,"\u00e9":1,"\uffff":5,"\ud83d\ude00":6}"#,
            "5b2d3cee432a31008ab5de7ed7ecd2a40dd29921c31f8d3dfe0c550748d58173",
            r#"{"": 7, "B": 3, "a": 4, "z": 2, "\u00e9": 1, "\uffff": 5, "\ud83d\ude00": 6}"#,
            "da16088b2624a8d0e0b8f3021fb0004f516e084a3414c10ff9eb27a54bce6941",
        ),
        (
            r#"{"version":"1.2.3","architecture":{"64bit":{"url":"https://e.org/a.zip","hash":["sha256:0"]}},"bin":[["a.exe","a","--x y"]],"env_set":{"A":"$dir"},"notes":["Ejemplo \u2014 nota"],"suggest":{}, "empty":[]}"#,
            r#"{"architecture":{"64bit":{"hash":["sha256:0"],"url":"https://e.org/a.zip"}},"bin":[["a.exe","a","--x y"]],"empty":[],"env_set":{"A":"$dir"},"notes":["Ejemplo \u2014 nota"],"suggest":{},"version":"1.2.3"}"#,
            "46c6519c4fa88fba44ef18e81f1ec24816086b9f35f2060bd516d237e076cad4",
            r#"{"architecture": {"64bit": {"hash": ["sha256:0"], "url": "https://e.org/a.zip"}}, "bin": [["a.exe", "a", "--x y"]], "empty": [], "env_set": {"A": "$dir"}, "notes": ["Ejemplo \u2014 nota"], "suggest": {}, "version": "1.2.3"}"#,
            "e1a3b3f98d9a31e6eb46cd9f86ae7512fb2e4b0d57c235f0b1fcc805bc811095",
        ),
    ];

    #[test]
    fn dumps_coincide_con_python() {
        for (source, compact, compact_sha, default, default_sha) in CASES {
            let value: Value = serde_json::from_str(source).unwrap();
            let actual = dumps(&value, DumpOptions::COMPACT_SORTED);
            assert_eq!(actual, compact, "{source}");
            assert_eq!(sha256(&actual), compact_sha, "{source}");
            let actual = dumps(&value, DumpOptions::DEFAULT_SORTED);
            assert_eq!(actual, default, "{source}");
            assert_eq!(sha256(&actual), default_sha, "{source}");
        }
    }

    #[test]
    fn repr_de_flotantes() {
        for (value, expected) in [
            (1.0, "1.0"),
            (0.5, "0.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5e300, "1.5e+300"),
            (0.0001, "0.0001"),
            (0.000_01, "1e-05"),
            (123.456, "123.456"),
            (-2.5e-7, "-2.5e-07"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        ] {
            assert_eq!(py_float_repr(value), expected);
        }
    }

    #[test]
    fn str_y_repr_como_python() {
        let value: Value =
            serde_json::from_str(r#"{"a":["x",1,2.0,true,null,{"k":"it's"}],"n":{}}"#).unwrap();
        assert_eq!(
            py_str(&value),
            r#"{'a': ['x', 1, 2.0, True, None, {'k': "it's"}], 'n': {}}"#
        );
        assert_eq!(py_str(&Value::String("tal cual".into())), "tal cual");
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_type_name(&Value::Null), "NoneType");
        assert!(!py_truthy(&serde_json::json!(0.0)));
        assert!(py_truthy(&serde_json::json!([0])));
    }
}
