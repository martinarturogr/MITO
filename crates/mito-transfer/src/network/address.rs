//! Validación, origen y redirecciones de URL (`validate_url`, `origin` y
//! `redirect_location` de `network.py`).

use crate::web;

/// Estados de redirección (`REDIRECTS`).
pub const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];

/// Valida una URL HTTP/HTTPS con host y sin credenciales (`validate_url`).
///
/// # Errors
///
/// `Configura una URL HTTP/HTTPS sin credenciales` si el esquema no es `http` o `https`,
/// no hay host o hay `usuario@`; también si la URL no se puede interpretar (por ejemplo,
/// un puerto que no es un número).
pub fn validate_url(url: &str) -> mito_core::Result<reqwest::Url> {
    mito_core::config::validate_url(url)?;
    reqwest::Url::parse(url).map_err(|_| {
        mito_core::Error::domain(mito_core::Message::new(
            "Configura una URL HTTP/HTTPS sin credenciales",
        ))
    })
}

/// `(esquema, netloc)` en minúsculas, con la `netloc` tal cual (`urlsplit`).
pub fn origin(url: &str) -> (String, String) {
    let parts = web::split(url);
    (parts.scheme, parts.netloc.to_lowercase())
}

/// ¿Es el mismo origen? Como [`origin`], pero el puerto por defecto (`:80` en `http`,
/// `:443` en `https`) cuenta igual que un puerto implícito (F6 del original, corregido).
pub fn same_origin(left: &str, right: &str) -> bool {
    normalized(left) == normalized(right)
}

fn normalized(url: &str) -> (String, String) {
    let (scheme, netloc) = origin(url);
    let default = match scheme.as_str() {
        "http" => ":80",
        "https" => ":443",
        _ => "",
    };
    let netloc = match netloc.strip_suffix(default) {
        Some(host) if !default.is_empty() => host.to_owned(),
        _ => netloc,
    };
    (scheme, netloc)
}

/// Destino absoluto de una redirección, o `""` (`redirect_location`; `urljoin`).
///
/// Solo cuentan los estados de [`REDIRECTS`] con cabecera `Location` no vacía.
pub fn redirect_location(url: &str, status: u16, location: Option<&str>) -> String {
    let Some(location) = location.filter(|l| !l.is_empty() && REDIRECTS.contains(&status)) else {
        return String::new();
    };
    urljoin(url, location)
}

/// `urllib.parse.urljoin(base, reference)`; si no se puede interpretar, devuelve `reference`.
pub fn urljoin(base: &str, reference: &str) -> String {
    reqwest::Url::parse(base)
        .and_then(|base| base.join(reference))
        .map_or_else(|_| reference.to_owned(), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_las_urls_del_contrato() {
        assert!(validate_url("http://proxy.example:8080").is_ok());
        assert!(validate_url("HTTPS://Host/a?b=1").is_ok());
        for bad in [
            "",
            "ftp://host",
            "http://",
            "http://@host",
            "http://user:secret@proxy.example",
            "proxy.example:8080",
            "http://host:abc",
        ] {
            let error = validate_url(bad).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Configura una URL HTTP/HTTPS sin credenciales",
                "{bad}"
            );
        }
    }

    #[test]
    fn el_origen_no_normaliza_el_puerto_pero_la_comparacion_si() {
        assert_eq!(
            origin("HTTP://Portal.Empresa.local/login?x=1"),
            ("http".to_owned(), "portal.empresa.local".to_owned())
        );
        assert_ne!(origin("http://h"), origin("http://h:80"));
        assert!(same_origin("http://h/a", "http://H:80/b"));
        assert!(same_origin("https://h:443/a", "https://h/b"));
        assert!(!same_origin("http://h", "https://h"));
        assert!(!same_origin("http://h:8080", "http://h"));
        assert!(!same_origin("https://h:80", "https://h"));
    }

    #[test]
    fn la_redireccion_solo_cuenta_con_estado_de_redireccion_y_location() {
        let base = "http://h:1/a/b?x=1";
        assert_eq!(
            redirect_location(base, 302, Some("/login")),
            "http://h:1/login"
        );
        assert_eq!(
            redirect_location(base, 308, Some("http://o/p")),
            "http://o/p"
        );
        assert_eq!(redirect_location(base, 400, Some("/login")), "");
        assert_eq!(redirect_location(base, 407, Some("/login")), "");
        assert_eq!(redirect_location(base, 302, None), "");
        assert_eq!(redirect_location(base, 302, Some("")), "");
    }
}
