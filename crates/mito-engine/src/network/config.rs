//! Claves de red de `config.properties` (`Network.__init__`, `proxy_environment` y
//! `start_network_poll` del original).

use std::time::Duration;

use mito_core::Message;
use mito_core::config::Values;
use mito_transfer::network::validate_url;

use crate::environment::Env;
use crate::error::{Error, Result};

/// Prefijo de los campos extra del POST del portal.
const FORM_FIELD_PREFIX: &str = "proxy_authentication_form_field.";

/// Claves de red de `config.properties` ya validadas (`Network.__init__`).
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfig {
    /// `network.timeout_seconds` (por defecto 4; mayor que 0 y hasta 60).
    pub timeout: Duration,
    /// `network.check_url` (por defecto `https://www.gstatic.com/generate_204`).
    pub check_url: String,
    /// `network.check_status` (por defecto 204).
    pub expected: u16,
    /// `proxy_authentication_url` (vacía si no hay).
    pub auth_url: String,
    /// `enable_proxy_authentication` y `proxy_authentication_url` no vacía.
    pub enabled: bool,
    /// `proxy_authentication_username_field` (por defecto `username`).
    pub username_field: String,
    /// `proxy_authentication_password_field` (por defecto `password`).
    pub password_field: String,
    /// `proxy_authentication_require_https`: solo `false` lo desactiva.
    pub require_https: bool,
    /// `proxy_authentication_form_field.<nombre>`: campos extra del POST, en orden.
    pub form_fields: Vec<(String, String)>,
}

/// `float(texto)` de Python (sin espacios, que `Values` ya recorta): admite guiones bajos
/// entre dígitos, `inf` y `nan`.
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
    text.trim().replace('_', "").parse().ok()
}

fn invalid(template: &'static str, key: &str) -> Error {
    Error::domain(Message::new(template).arg("p0", key))
}

impl NetworkConfig {
    /// Lee y valida las claves de red de `values`, cada una con su valor por defecto.
    ///
    /// Respecto al original (F2), un número que no lo es o un código fuera de rango es un
    /// error de dominio con mensaje, no un `ValueError`.
    ///
    /// # Errors
    ///
    /// - `network.timeout_seconds` no numérico o fuera de `(0, 60]`.
    /// - `network.check_url` o `proxy_authentication_url` inválidas
    ///   (`Configura una URL HTTP/HTTPS sin credenciales`).
    /// - `network.check_status` que no es un entero de 100 a 599.
    pub fn from_values(values: &Values) -> Result<Self> {
        let get = |key: &str, default: &'static str| values.get(key).unwrap_or(default);
        let key = "network.timeout_seconds";
        let seconds = py_float(get(key, "4"))
            .ok_or_else(|| invalid("{p0}: introduce un número mayor que 0 y hasta 60", key))?;
        if !(seconds > 0.0 && seconds <= 60.0) {
            return Err(Error::domain(Message::new(
                "network.timeout_seconds debe estar entre 0 y 60",
            )));
        }
        let check_url = get("network.check_url", "https://www.gstatic.com/generate_204");
        validate_url(check_url)?;
        let key = "network.check_status";
        let status = get(key, "204");
        let expected = status
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| status.parse::<u16>().ok())
            .flatten()
            .filter(|code| (100..=599).contains(code))
            .ok_or_else(|| invalid("{p0}: introduce un código HTTP entre 100 y 599", key))?;
        let auth_url = get("proxy_authentication_url", "");
        if !auth_url.is_empty() {
            validate_url(auth_url)?;
        }
        let enabled = get("enable_proxy_authentication", "false").to_lowercase() == "true"
            && !auth_url.is_empty();
        Ok(Self {
            timeout: Duration::from_secs_f64(seconds),
            check_url: check_url.to_owned(),
            expected,
            auth_url: auth_url.to_owned(),
            enabled,
            username_field: get("proxy_authentication_username_field", "username").to_owned(),
            password_field: get("proxy_authentication_password_field", "password").to_owned(),
            require_https: get("proxy_authentication_require_https", "true").to_lowercase()
                != "false",
            form_fields: values
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_prefix(FORM_FIELD_PREFIX)
                        .map(|name| (name.to_owned(), value.to_owned()))
                })
                .collect(),
        })
    }
}

/// `network.poll_seconds` (`start_network_poll`): intervalo del sondeo periódico de la
/// interfaz, o `None` si está desactivado.
///
/// Un valor que no es un número da 20 s; 0, negativo, `nan` e infinito desactivan el
/// sondeo; el mínimo es 5 s.
pub fn poll_interval(values: &Values) -> Option<Duration> {
    let seconds = py_float(values.get("network.poll_seconds").unwrap_or("20")).unwrap_or(20.0);
    if !(seconds > 0.0 && seconds.is_finite()) {
        return None;
    }
    Some(Duration::try_from_secs_f64(seconds.max(5.0)).unwrap_or(Duration::MAX))
}

/// `proxy_environment`: aplica a `env` las claves `http_proxy`, `https_proxy` y
/// `no_proxy` de `values`.
///
/// Cada clave presente borra del entorno cualquier variable homónima (sin distinguir
/// mayúsculas) y escribe `NOMBRE` en mayúsculas, **también vacío**: un valor vacío
/// suprime el proxy heredado de Windows.
///
/// # Errors
///
/// `Configura una URL HTTP/HTTPS sin credenciales` si `http_proxy` o `https_proxy` no
/// están vacíos y no son una URL HTTP/HTTPS sin credenciales.
pub fn proxy_environment(values: &Values, env: &mut Env) -> Result<()> {
    for name in ["http_proxy", "https_proxy", "no_proxy"] {
        let Some(value) = values.get(name) else {
            continue;
        };
        if !value.is_empty() && name != "no_proxy" {
            mito_core::config::validate_url(value)?;
        }
        env.retain(|key, _| key.to_lowercase() != name);
        env.insert(name.to_uppercase(), value.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
