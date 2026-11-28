//! Sondas de red y detección de campos del portal (`Probe`, `probe` y `portal_fields`).

use std::time::Duration;

use super::address::REDIRECTS;
use super::html::portal_forms;
use super::proxies::Proxies;
use super::session::{Reply, Session, SessionOptions};

/// Resultado clasificado de una sonda (`Probe.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// Respondió el estado esperado.
    Ok,
    /// El proxy exige autenticación (407).
    Auth,
    /// Redirección: un portal cautivo.
    Portal,
    /// Sin respuesta.
    Unreachable,
    /// Otra respuesta: la ruta está bloqueada.
    Blocked,
}

/// Una sola petición sin seguir redirecciones (`Probe`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// Código HTTP, o el del `CONNECT` fallido; 0 si no hubo respuesta.
    pub status: u16,
    /// Cabecera `Location` de una respuesta que no es 2xx.
    pub location: String,
    /// Esquemas de `Proxy-Authenticate` sin repetir, ordenados y separados por `, `.
    pub authenticate: String,
    /// Clase del error de transporte (solo informativa).
    pub error: String,
    /// URL consultada.
    pub url: String,
}

impl Probe {
    /// Clasificación de la sonda, en el orden del original.
    pub fn kind(&self, expected: u16) -> ProbeKind {
        if self.status == expected {
            ProbeKind::Ok
        } else if self.status == 407 {
            ProbeKind::Auth
        } else if REDIRECTS.contains(&self.status) && !self.location.is_empty() {
            ProbeKind::Portal
        } else if self.status == 0 {
            ProbeKind::Unreachable
        } else {
            ProbeKind::Blocked
        }
    }
}

/// Esquemas de las cabeceras `Proxy-Authenticate`: primer token de cada una, sin
/// repetidos, ordenados y unidos con `, `.
fn schemes(values: &[String]) -> String {
    let mut found: Vec<&str> = values
        .iter()
        .filter_map(|value| value.split_whitespace().next())
        .collect();
    found.sort_unstable();
    found.dedup();
    found.join(", ")
}

/// Una petición por `proxy` (`""` directo) sin seguir redirecciones (`probe`).
///
/// No aplica las exclusiones del sistema (F7 del original, corregido): el candidato se
/// prueba de verdad.
pub fn probe(url: &str, proxy: &str, timeout: Duration) -> Probe {
    let session = Session::new(&SessionOptions {
        proxies: Proxies::single(proxy),
        timeout,
        cookies: false,
    });
    let outcome = session.and_then(|session| session.get(url));
    match outcome {
        Ok(Reply {
            status,
            location,
            proxy_authenticate,
            ..
        }) => {
            let success = (200..300).contains(&status);
            Probe {
                status,
                location: if success {
                    String::new()
                } else {
                    location.unwrap_or_default()
                },
                authenticate: if success {
                    String::new()
                } else {
                    schemes(&proxy_authenticate)
                },
                error: String::new(),
                url: url.to_owned(),
            }
        }
        Err(failure) => Probe {
            status: failure.tunnel_status.unwrap_or(0),
            error: failure.error,
            url: url.to_owned(),
            ..Probe::default()
        },
    }
}

/// Nombres de campo del portal distintos de los predeterminados (`portal_fields`).
///
/// Solo nombres, nunca valores. Devuelve `[]` si no se puede abrir la página (cualquier
/// estado que no sea 2xx, incluidas las redirecciones) o no tiene formulario con
/// contraseña.
pub fn portal_fields(url: &str, proxy: &str, timeout: Duration) -> Vec<(String, String)> {
    let Ok(session) = Session::new(&SessionOptions {
        proxies: Proxies::single(proxy),
        timeout,
        cookies: false,
    }) else {
        return Vec::new();
    };
    let Ok(reply) = session.get(url) else {
        return Vec::new();
    };
    if !reply.is_success() {
        return Vec::new();
    }
    let html = String::from_utf8_lossy(&reply.body);
    let forms = portal_forms(&html, "password");
    let Some(form) = forms.first() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    if !form.user_field.is_empty() && form.user_field != "username" {
        values.push((
            "proxy_authentication_username_field".to_owned(),
            form.user_field.clone(),
        ));
    }
    if !form.password_field.is_empty() && form.password_field != "password" {
        values.push((
            "proxy_authentication_password_field".to_owned(),
            form.password_field.clone(),
        ));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clasificacion_sigue_el_orden_del_original() {
        let probe = |status: u16, location: &str| Probe {
            status,
            location: location.to_owned(),
            ..Probe::default()
        };
        assert_eq!(probe(204, "").kind(204), ProbeKind::Ok);
        assert_eq!(probe(407, "").kind(204), ProbeKind::Auth);
        assert_eq!(probe(302, "http://p/").kind(204), ProbeKind::Portal);
        assert_eq!(probe(308, "/x").kind(204), ProbeKind::Portal);
        assert_eq!(probe(302, "").kind(204), ProbeKind::Blocked);
        assert_eq!(probe(0, "").kind(204), ProbeKind::Unreachable);
        assert_eq!(probe(403, "").kind(204), ProbeKind::Blocked);
        // El estado esperado gana a todo lo demás.
        assert_eq!(probe(407, "").kind(407), ProbeKind::Ok);
    }

    #[test]
    fn los_esquemas_se_ordenan_y_no_se_repiten() {
        let values = [
            "Negotiate".to_owned(),
            "NTLM".to_owned(),
            "NTLM abc".to_owned(),
        ];
        assert_eq!(schemes(&values), "NTLM, Negotiate");
        assert_eq!(
            schemes(&["  ".to_owned(), "Basic realm=\"x\"".to_owned()]),
            "Basic"
        );
        assert_eq!(schemes(&[]), "");
    }
}
