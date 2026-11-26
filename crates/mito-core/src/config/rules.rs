//! Reglas de validación de la configuración (`validated` del original y las
//! validaciones auxiliares de `network`, `shortcuts`, `pocketools` y `engine`).
//!
//! Los mensajes son las plantillas españolas del original y existen en los bundles de
//! `mito-i18n`.

use std::collections::HashSet;
use std::path::Path;

use super::properties::{self, PREFIXES};
use super::text::{Values, py_strip};
use crate::error::{Result, bail};
use crate::message::Message;
use crate::storage::{IDENTIFIER_MAX, identifier};

/// Variables de entorno que MITO controla y que no se pueden redefinir.
const RESERVED_ENV: [&str; 9] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "HOMEDRIVE",
    "HOMEPATH",
];

/// Valida una URL HTTP/HTTPS sin credenciales (`network.validate_url`).
///
/// Sigue `urllib.parse.urlsplit`: ignora espacios y controles iniciales, descarta
/// tabuladores y saltos de línea internos y deduce el host sin tocar la red.
///
/// # Errors
///
/// `Configura una URL HTTP/HTTPS sin credenciales` si el esquema no es `http` o
/// `https`, no hay host o hay `usuario@`.
pub fn validate_url(url: &str) -> Result<()> {
    if url_is_valid(url) {
        Ok(())
    } else {
        bail(Message::new(
            "Configura una URL HTTP/HTTPS sin credenciales",
        ))
    }
}

fn url_is_valid(url: &str) -> bool {
    let cleaned: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let Some(colon) = cleaned.find(':') else {
        return false;
    };
    let scheme = &cleaned[..colon];
    let valid_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme
        || !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    {
        return false;
    }
    let Some(authority) = cleaned[colon + 1..].strip_prefix("//") else {
        return false;
    };
    let netloc = &authority[..authority.find(['/', '?', '#']).unwrap_or(authority.len())];
    if netloc.contains('@') || netloc.contains('[') != netloc.contains(']') {
        return false;
    }
    let host = match netloc.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => netloc.split(':').next().unwrap_or_default(),
    };
    !host.is_empty()
}

/// Valida la etiqueta de las insignias de los accesos directos (`shortcuts.badge_text`).
///
/// Devuelve la etiqueta normalizada (recortada y en mayúsculas; vacía equivale a `MITO`).
///
/// # Errors
///
/// `La etiqueta admite de 1 a 4 letras o números`.
pub fn badge_text(value: &str) -> Result<String> {
    let trimmed = py_strip(value).to_uppercase();
    let text = if trimmed.is_empty() {
        "MITO".to_owned()
    } else {
        trimmed
    };
    if text.chars().count() > 4 || !text.chars().all(char::is_alphanumeric) {
        return bail(Message::new("La etiqueta admite de 1 a 4 letras o números"));
    }
    Ok(text)
}

/// Valida el origen de una pocketool (`pocketools.validate_source`): URL HTTP(S) sin
/// credenciales, `pocketools/<nombre>` o una carpeta local absoluta.
///
/// # Errors
///
/// `Pocketool: usa una URL Git HTTP/HTTPS o una carpeta local absoluta`, o los errores
/// de [`validate_url`] e [`identifier`].
pub fn validate_source(source: &str) -> Result<()> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return validate_url(source);
    }
    if let Some(name) = source.strip_prefix("pocketools/") {
        let ascii = name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && name.len() <= IDENTIFIER_MAX
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if ascii {
            return identifier(name).map(drop);
        }
    }
    if Path::new(source).is_absolute() {
        return Ok(());
    }
    bail(Message::new(
        "Pocketool: usa una URL Git HTTP/HTTPS o una carpeta local absoluta",
    ))
}

/// Valida las variables de entorno libres de la configuración o de un profile
/// (`Engine.validate_env`).
///
/// # Errors
///
/// `Variable de entorno no válida: <nombre>` si el nombre está vacío o contiene `=`,
/// `;` o `\0`, o el valor contiene `\0`; `Variable reservada por MITO: <nombre>` para
/// `PATH`, `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`,
/// `HOMEDRIVE`, `HOMEPATH` y `MITO_*`; `Variable de entorno repetida: <nombre>` si dos
/// nombres solo difieren en mayúsculas.
pub fn validate_env<'a>(variables: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<()> {
    let mut seen = HashSet::new();
    for (key, value) in variables {
        if key.is_empty() || key.contains(['=', ';', '\0']) || value.contains('\0') {
            return bail(Message::new("Variable de entorno no válida: ").with_suffix(key));
        }
        let upper = key.to_uppercase();
        if RESERVED_ENV.contains(&upper.as_str()) || upper.starts_with("MITO_") {
            return bail(Message::new("Variable reservada por MITO: ").with_suffix(key));
        }
        if !seen.insert(upper) {
            return bail(Message::new("Variable de entorno repetida: ").with_suffix(key));
        }
    }
    Ok(())
}

/// `float(texto)` de Python para los números finitos que admite la configuración:
/// acepta signo, exponente, `inf` y `nan` (que luego se rechazan) y guiones bajos
/// entre dígitos (`1_0`).
fn py_float(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'_' {
            let before = index.checked_sub(1).and_then(|i| bytes.get(i));
            let after = bytes.get(index + 1);
            if !before.is_some_and(u8::is_ascii_digit) || !after.is_some_and(u8::is_ascii_digit) {
                return None;
            }
        }
    }
    text.replace('_', "").parse().ok()
}

/// Valida pares `(clave, valor)` de `config.properties` y devuelve los valores
/// normalizados (`validated(..., profile=False)`).
///
/// Las claves y los valores se recortan. Se comprueba, por orden: la sintaxis de cada
/// par, las variables `env.*` y después cada propiedad (lista cerrada, URL, números,
/// familias `bucket.`, `proxy_authentication_form_field.` y `pocketool.`). Las claves
/// desconocidas se aceptan sin validar.
///
/// # Errors
///
/// El primer problema encontrado, con los mensajes del original.
pub fn validated<K: AsRef<str>, V: AsRef<str>>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> Result<Values> {
    let mut result = Values::new();
    for (key, value) in pairs {
        let key = py_strip(key.as_ref());
        let value = py_strip(value.as_ref());
        if key.is_empty() || key.contains(['=', '\r', '\n', '\0']) || key.starts_with(['#', '!']) {
            return bail(Message::new("Introduce una clave válida"));
        }
        if value.contains(['\r', '\n', '\0']) {
            return bail(Message::new("{p0}: el valor debe ocupar una línea").arg("p0", key));
        }
        if result.contains(key) {
            return bail(Message::new("Clave repetida: {p1}").arg("p1", key));
        }
        result.insert(key, value);
    }
    let env = result.env();
    validate_env(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
    let mut pocketools = HashSet::new();
    for (key, value) in result.iter() {
        validate_property(key, value, &mut pocketools)?;
    }
    Ok(result)
}

fn validate_property(key: &str, value: &str, pocketools: &mut HashSet<String>) -> Result<()> {
    if properties::lookup(key)
        .and_then(|p| p.choices)
        .is_some_and(|choices| !choices.contains(&value))
    {
        return bail(Message::new("{p0}: elige uno de los valores permitidos").arg("p0", key));
    }
    match key {
        // Pueden estar vacías (sin proxy), pero si no lo están han de ser URL válidas.
        "http_proxy" | "https_proxy" | "proxy_authentication_url" => {
            if !value.is_empty() {
                validate_url(value)?;
            }
        }
        // Se validan siempre, también vacías.
        "network.check_url" | "mito.update_url" => validate_url(value)?,
        "network.timeout_seconds" => {
            if !py_float(value).is_some_and(|n| n.is_finite() && n > 0.0 && n <= 60.0) {
                return bail(
                    Message::new("{p0}: introduce un número mayor que 0 y hasta 60").arg("p0", key),
                );
            }
        }
        "network.check_status" => {
            let code = value.parse::<u32>().ok();
            if !value.bytes().all(|b| b.is_ascii_digit())
                || !code.is_some_and(|c| (100..=599).contains(&c))
            {
                return bail(
                    Message::new("{p0}: introduce un código HTTP entre 100 y 599").arg("p0", key),
                );
            }
        }
        "shortcut.badgeText" => {
            badge_text(value)?;
        }
        _ => {}
    }
    if let Some(name) = key.strip_prefix(PREFIXES[0]) {
        identifier(name)?;
        if value.is_empty() {
            return bail(Message::new("{p0}: introduce una fuente de catálogo").arg("p0", key));
        }
    }
    if key.strip_prefix(PREFIXES[1]).is_some_and(str::is_empty) {
        return bail(Message::new("Falta el nombre del campo del formulario"));
    }
    if let Some(name) = key.strip_prefix(PREFIXES[2]) {
        let name = identifier(name)?;
        if !pocketools.insert(name.clone()) {
            return bail(Message::new("Pocketool repetida: {p1}").arg("p1", name));
        }
        validate_source(value)?;
        if value.starts_with("pocketools/") && value != format!("pocketools/{name}") {
            return bail(Message::new(
                "La carpeta local debe coincidir con el nombre del paquete.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(result: Result<impl std::fmt::Debug>) -> String {
        result.unwrap_err().to_string()
    }

    fn check(pairs: &[(&str, &str)]) -> Result<Values> {
        validated(pairs.iter().copied())
    }

    #[test]
    fn urls_validas_e_invalidas() {
        for good in [
            "https://example.com",
            "HTTP://example.com:8080/a?b=c#d",
            "http://[::1]:80/x",
            "  \thttps://example.com",
            "https://exa\nmple.com",
        ] {
            assert!(validate_url(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "example.com",
            "ftp://example.com",
            "file:///C:/x",
            "https://",
            "https:///ruta",
            "http://user:password@example.org",
            "http://user@example.org",
            "http://@example.org",
            "https:example.com",
            "http://[::1/x",
            "1http://x.com",
        ] {
            assert_eq!(
                error(validate_url(bad)),
                "Configura una URL HTTP/HTTPS sin credenciales",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn etiqueta_de_insignia() {
        assert_eq!(badge_text("").unwrap(), "MITO");
        assert_eq!(badge_text(" ab1 ").unwrap(), "AB1");
        for bad in ["abcde", "a b", "a-b", "ß ß"] {
            assert_eq!(
                error(badge_text(bad)),
                "La etiqueta admite de 1 a 4 letras o números",
                "{bad}"
            );
        }
    }

    #[test]
    fn origen_de_pocketool() {
        assert!(validate_source("https://example.com/repo.git").is_ok());
        assert!(validate_source("pocketools/local").is_ok());
        assert!(validate_source("C:\\datos\\paquete").is_ok());
        assert!(validate_source("C:/datos").is_ok());
        for bad in [
            "relativa/carpeta",
            "pocketools/",
            "pocketools/a/b",
            "/rooted",
        ] {
            assert!(validate_source(bad).is_err(), "{bad}");
        }
        assert!(validate_source("https://u:p@example.com/r").is_err());
        assert!(validate_source("pocketools/con").is_err());
    }

    #[test]
    fn variables_de_entorno() {
        assert!(validate_env([("TOKEN", "a=b"), ("x", "")]).is_ok());
        assert_eq!(
            error(validate_env([("PATH", "x")])),
            "Variable reservada por MITO: PATH"
        );
        assert_eq!(
            error(validate_env([("mito_x", "x")])),
            "Variable reservada por MITO: mito_x"
        );
        assert_eq!(
            error(validate_env([("X", "1"), ("x", "2")])),
            "Variable de entorno repetida: x"
        );
        for bad in [("", "v"), ("A=B", "v"), ("A;B", "v"), ("A", "v\0")] {
            assert!(
                error(validate_env([bad])).starts_with("Variable de entorno no válida: "),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn numeros_como_float_de_python() {
        for good in ["1", "0.5", "60", "1e1", "+2", ".5", "1_0", "5."] {
            assert!(
                check(&[("network.timeout_seconds", good)]).is_ok(),
                "{good}"
            );
        }
        for bad in [
            "0", "-1", "60.1", "nan", "inf", "-inf", "abc", "", "1__0", "_1", "1_", "1e999",
        ] {
            assert_eq!(
                error(check(&[("network.timeout_seconds", bad)])),
                "network.timeout_seconds: introduce un número mayor que 0 y hasta 60",
                "{bad}"
            );
        }
    }

    #[test]
    fn codigo_http() {
        for good in ["100", "204", "599"] {
            assert!(check(&[("network.check_status", good)]).is_ok(), "{good}");
        }
        for bad in ["99", "600", "999", "", "2x", "-204", "+204", "20 4"] {
            assert_eq!(
                error(check(&[("network.check_status", bad)])),
                "network.check_status: introduce un código HTTP entre 100 y 599",
                "{bad}"
            );
        }
    }

    #[test]
    fn sintaxis_de_pares() {
        for key in ["", "  ", "a=b", "a\nb", "a\0", "#c", "!c"] {
            assert_eq!(
                error(check(&[(key, "v")])),
                "Introduce una clave válida",
                "{key:?}"
            );
        }
        assert_eq!(
            error(check(&[("env.X", "a\nb")])),
            "env.X: el valor debe ocupar una línea"
        );
        assert_eq!(error(check(&[("a", "1"), ("a", "2")])), "Clave repetida: a");
        let values = check(&[(" env.X ", " a ")]).unwrap();
        assert_eq!(values.get("env.X"), Some("a"));
    }

    #[test]
    fn listas_cerradas_y_urls() {
        assert_eq!(
            error(check(&[("terminal.defaultShell", "bash")])),
            "terminal.defaultShell: elige uno de los valores permitidos"
        );
        assert!(check(&[("terminal.defaultShell", "powershell")]).is_ok());
        assert!(check(&[("mito.language", "español")]).is_ok());
        for bad in ["french", "", "ENGLISH", "../english"] {
            assert!(check(&[("mito.language", bad)]).is_err(), "{bad}");
        }
        assert!(check(&[("http_proxy", "")]).is_ok());
        assert!(check(&[("network.check_url", "")]).is_err());
        assert!(check(&[("mito.update_url", "")]).is_err());
        assert!(check(&[("proxy_authentication_url", "ftp://x")]).is_err());
        assert!(check(&[("no_proxy", "cualquier cosa")]).is_ok());
    }

    #[test]
    fn familias_dinamicas() {
        assert!(check(&[("bucket.main", "https://github.com/ScoopInstaller/Main")]).is_ok());
        assert!(check(&[("bucket.main", "C:/catalog")]).is_ok());
        assert_eq!(
            error(check(&[("bucket.main", "")])),
            "bucket.main: introduce una fuente de catálogo"
        );
        assert_eq!(
            error(check(&[("bucket.bad name", "x")])),
            "Nombre no válido: 'bad name'"
        );
        assert_eq!(
            error(check(&[("proxy_authentication_form_field.", "x")])),
            "Falta el nombre del campo del formulario"
        );
        assert!(check(&[("proxy_authentication_form_field.user", "")]).is_ok());
        assert!(check(&[("pocketool.local", "pocketools/local")]).is_ok());
        assert_eq!(
            error(check(&[("pocketool.local", "pocketools/otra")])),
            "La carpeta local debe coincidir con el nombre del paquete."
        );
        assert_eq!(
            error(check(&[
                ("pocketool.Local", "C:/a"),
                ("pocketool.local", "C:/b")
            ])),
            "Pocketool repetida: local"
        );
    }

    #[test]
    fn las_claves_desconocidas_y_posthog_se_aceptan_sin_validar() {
        let values = check(&[
            ("unknown.setting", "keep"),
            ("posthog.enabled", "True"),
            ("posthog.host", "no es una url"),
        ])
        .unwrap();
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn entradas_invalidas_del_original() {
        for pairs in [
            vec![("enable_proxy_authentication", "maybe")],
            vec![("env.PATH", "bad")],
            vec![("env.X", "one"), ("env.x", "two")],
            vec![("env.X", "a\nb")],
            vec![("network.timeout_seconds", "nan")],
            vec![("network.check_status", "999")],
            vec![("http_proxy", "http://user:password@example.org")],
            vec![("shortcut.badgeColor", "blanco")],
            vec![("terminal.defaultShell", "bash")],
        ] {
            assert!(check(&pairs).is_err(), "{pairs:?}");
        }
    }
}
