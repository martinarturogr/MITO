//! JSON con el formato de `json.dumps(record, ensure_ascii=True)` de Python.
//!
//! Separadores `", "` y `": "`, todo lo que no es ASCII escapado como `\uXXXX` (con
//! pares sustitutos fuera del BMP) y sin saltos de línea. Es el formato de cada línea
//! de `logs/mito.log`.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// Serializa `value` como lo haría `json.dumps(value, ensure_ascii=True)`.
pub fn python_dumps(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_string(out, key);
                out.push_str(": ");
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, text: &str) {
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
            c if (' '..='\u{7e}').contains(&c) || c == '\u{7f}' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    // Escribir en un String no puede fallar.
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

/// Instante en UTC como `datetime.now(timezone.utc).isoformat()`:
/// `2026-11-26T21:15:00.123456+00:00`, sin fracción si los microsegundos son 0.
pub fn iso_utc(instant: SystemTime) -> String {
    let (seconds, micros) = match instant.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => (
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
            elapsed.subsec_micros(),
        ),
        Err(_) => (0, 0),
    };
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    if micros != 0 {
        let _ = write!(text, ".{micros:06}");
    }
    text.push_str("+00:00");
    text
}

/// Fecha civil (año, mes, día) de un número de días desde 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Algoritmo de Howard Hinnant (calendario gregoriano proléptico).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn separadores_y_orden_como_python() {
        let value = json!({"b": 1, "a": [true, null, "x"], "c": {}});
        assert_eq!(
            python_dumps(&value),
            r#"{"b": 1, "a": [true, null, "x"], "c": {}}"#
        );
    }

    #[test]
    fn ensure_ascii_escapa_como_python() {
        // La barra invertida se pone aparte para que los literales sean inequívocos.
        let bs = '\\';
        assert_eq!(python_dumps(&json!("ñ")), format!("\"{bs}u00f1\""));
        assert_eq!(
            python_dumps(&json!("😀")),
            format!("\"{bs}ud83d{bs}ude00\"")
        );
        assert_eq!(
            python_dumps(&json!("a\"b\\c\n\r\t\u{8}\u{c}\u{1}\u{7f}")),
            "\"a\\\"b\\\\c\\n\\r\\t\\b\\f\\u0001\u{7f}\""
        );
        assert_eq!(python_dumps(&json!("/")), r#""/""#);
    }

    #[test]
    fn fecha_iso_con_y_sin_microsegundos() {
        assert_eq!(iso_utc(UNIX_EPOCH), "1970-01-01T00:00:00+00:00");
        let instant = UNIX_EPOCH + Duration::new(1_795_727_700, 123_456_000);
        assert_eq!(iso_utc(instant), "2026-11-26T21:15:00.123456+00:00");
        let leap = UNIX_EPOCH + Duration::new(1_709_210_096, 5_000);
        assert_eq!(iso_utc(leap), "2024-02-29T12:34:56.000005+00:00");
        let fin = UNIX_EPOCH + Duration::new(951_782_400, 0);
        assert_eq!(iso_utc(fin), "2000-02-29T00:00:00+00:00");
    }

    proptest! {
        #[test]
        fn la_salida_es_json_valido_y_ascii(text in ".{0,40}") {
            let dumped = python_dumps(&Value::String(text.clone()));
            prop_assert!(dumped.is_ascii());
            prop_assert!(!dumped.contains('\n'));
            let back: Value = serde_json::from_str(&dumped).unwrap();
            prop_assert_eq!(back, Value::String(text));
        }
    }
}
