//! URL y cabeceras: utilidades de `transfer.py` que no tocan la red.
//!
//! Se parsea a mano, como `urllib.parse.urlsplit`, para que cualquier texto tenga un
//! resultado (el original nunca falla al partir una URL).

use mito_core::Message;

use crate::error::Error;

/// Prefijos de marcado HTML (en minúsculas) que delatan una página interceptada.
const MARKUP: [&[u8]; 7] = [
    b"<!doctype html",
    b"<html",
    b"<head",
    b"<body",
    b"<script",
    b"<meta",
    b"<title",
];

/// Partes de una URL como las separa `urlsplit`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Split<'a> {
    /// Esquema en minúsculas (vacío si no hay).
    pub scheme: String,
    /// Autoridad (`usuario@host:puerto`), vacía si no hay.
    pub netloc: &'a str,
    /// Ruta, sin query ni fragmento.
    pub path: &'a str,
}

pub(crate) fn split(url: &str) -> Split<'_> {
    let url = url.trim();
    let url = url.split_once('#').map_or(url, |(before, _)| before);
    let url = url.split_once('?').map_or(url, |(before, _)| before);
    let (scheme, rest) = match url.split_once(':') {
        Some((scheme, rest))
            if scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            (scheme.to_ascii_lowercase(), rest)
        }
        _ => (String::new(), url),
    };
    let (netloc, path) = match rest.strip_prefix("//") {
        Some(after) => match after.find('/') {
            Some(slash) => (&after[..slash], &after[slash..]),
            None => (after, ""),
        },
        None => ("", rest),
    };
    Split {
        scheme,
        netloc,
        path,
    }
}

/// ¿Es `http` o `https` el esquema de la URL?
pub(crate) fn is_http(url: &str) -> bool {
    matches!(split(url).scheme.as_str(), "http" | "https")
}

/// ¿Tiene la URL un esquema que no es `http` ni `https`? Una ruta relativa no lo tiene.
pub(crate) fn has_foreign_scheme(url: &str) -> bool {
    let scheme = split(url).scheme;
    !scheme.is_empty() && !matches!(scheme.as_str(), "http" | "https")
}

/// Host de la URL en minúsculas, sin credenciales, puerto ni corchetes
/// (`urlsplit(url).hostname`).
pub(crate) fn hostname(url: &str) -> Option<String> {
    let netloc = split(url).netloc;
    let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// URL pedida sin consulta ni fragmento: los tokens firmados nunca llegan a los
/// mensajes (`public_url`).
///
/// Diferencia deliberada: tampoco conserva las credenciales (`usuario:clave@`) de la
/// autoridad, que el original sí dejaba en el mensaje.
pub fn public_url(url: &str) -> String {
    let parts = split(url);
    let host = parts
        .netloc
        .rsplit_once('@')
        .map_or(parts.netloc, |(_, h)| h);
    if parts.scheme.is_empty() {
        return parts.path.to_owned();
    }
    if parts.netloc.is_empty() && !url.contains("//") {
        return format!("{}:{}", parts.scheme, parts.path);
    }
    format!("{}://{host}{}", parts.scheme, parts.path)
}

/// ¿Sustituyó un portal o un bloqueo de proxy un recurso que no es HTML? (`web_page`)
///
/// `url` es la URL pedida, `content_type` el tipo MIME sin parámetros y `head` los
/// primeros bytes del contenido.
pub fn web_page(url: &str, content_type: &str, head: &[u8]) -> bool {
    let path = split(url).path.to_lowercase();
    if [".htm", ".html", ".xhtml"]
        .iter()
        .any(|e| path.ends_with(e))
    {
        return false;
    }
    let skip = head
        .iter()
        .take_while(|b| matches!(b, 0xEF | 0xBB | 0xBF | b' ' | b'\t' | b'\r' | b'\n'))
        .count();
    let text = head[skip..].to_ascii_lowercase();
    MARKUP.iter().any(|m| text.starts_with(m))
        || (content_type.eq_ignore_ascii_case("text/html") && text.starts_with(b"<"))
}

/// Error de integridad por una página web en lugar del archivo (`web_page_error`).
pub fn web_page_error(final_url: &str, url: &str, size: Option<u64>) -> Error {
    let host = hostname(final_url).unwrap_or_else(|| "?".to_owned());
    let shown = if url.is_empty() { final_url } else { url };
    Error::Integrity {
        message: Message::new(
            "La red devolvió una página web en lugar del archivo ({p1}); puede ser el portal de autenticación o un bloqueo del proxy. Pulsa INTERNET y reintenta.",
        )
        .arg("p1", &host),
        url: public_url(shown),
        size,
        host,
        portal: true,
    }
}

/// `(inicio, total)` de una cabecera `Content-Range: bytes a-b/total` (`total` es
/// `None` con `*`). `None` si está mal formada.
pub(crate) fn content_range(value: Option<&str>) -> Option<(u64, Option<u64>)> {
    let value = value?.trim();
    let rest = value.strip_prefix("bytes")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let (range, total) = rest.trim().split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(start) || !digits(end) {
        return None;
    }
    let total = match total {
        "*" => None,
        t if digits(t) => Some(t.parse().ok()?),
        _ => return None,
    };
    Some((start.parse().ok()?, total))
}

/// Valor de `If-Range`: un `ETag` fuerte o, si no, `Last-Modified` (`_validator`).
pub(crate) fn validator(etag: Option<&str>, last_modified: Option<&str>) -> Option<String> {
    let etag = etag.map_or("", str::trim);
    if !etag.is_empty() && !etag.starts_with("W/") {
        return Some(etag.to_owned());
    }
    let modified = last_modified.map_or("", str::trim);
    (!modified.is_empty()).then(|| modified.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_paginas_interceptadas() {
        // Casos de test_web_page_detection del original.
        assert!(web_page(
            "https://x/app.zip",
            "application/octet-stream",
            b"\xef\xbb\xbf\n<!doctype html><p>"
        ));
        assert!(web_page(
            "https://x/app.json",
            "text/html",
            b"<div>blocked</div>"
        ));
        assert!(!web_page(
            "https://x/app.json",
            "text/html",
            b"{\"version\": \"1\"}"
        ));
        assert!(!web_page("https://x/app.zip", "text/html", b"PK\x03\x04"));
        assert!(!web_page(
            "https://x/app.xml",
            "application/xml",
            b"<?xml version=\"1.0\"?>"
        ));
    }

    #[test]
    fn una_url_html_no_es_una_pagina_interceptada() {
        for url in [
            "https://x/a.htm",
            "https://x/A.HTML?x=1",
            "https://x/a.xhtml#f",
        ] {
            assert!(!web_page(url, "text/html", b"<html>"), "{url}");
        }
        for prefix in [
            "<HTML>", "<head>", "<body>", "<script>", "<meta>", "<title>",
        ] {
            assert!(
                web_page("https://x/a.zip", "", prefix.as_bytes()),
                "{prefix}"
            );
        }
        assert!(!web_page("https://x/a.zip", "", b""));
    }

    #[test]
    fn url_publica_sin_query_ni_fragmento() {
        assert_eq!(
            public_url("http://127.0.0.1:80/release/jdk.zip?token=abc#frag"),
            "http://127.0.0.1:80/release/jdk.zip"
        );
        assert_eq!(public_url("https://h"), "https://h");
        assert_eq!(public_url("https://user:pw@h.org/a?x=1"), "https://h.org/a");
        assert_eq!(public_url("file:///app.zip"), "file:///app.zip");
    }

    #[test]
    fn partes_de_la_url() {
        assert_eq!(
            hostname("https://User@Example.ORG:8080/a"),
            Some("example.org".into())
        );
        assert_eq!(hostname("http://[::1]:80/x"), Some("::1".into()));
        assert_eq!(hostname("file:///app.zip"), None);
        assert!(is_http("HTTP://x"));
        assert!(is_http("https://x"));
        assert!(!is_http("file:///app.zip"));
        assert!(!is_http("ftp://x"));
        assert!(!is_http("//x/y"));
    }

    #[test]
    fn content_range_como_el_original() {
        assert_eq!(
            content_range(Some("bytes 8192-16383/16384")),
            Some((8192, Some(16384)))
        );
        assert_eq!(content_range(Some(" bytes  5-9/* ")), Some((5, None)));
        for bad in [
            "",
            "bytes 1-2",
            "bytes a-2/3",
            "items 1-2/3",
            "bytes 1-2/x",
            "bytes1-2/3",
        ] {
            assert_eq!(content_range(Some(bad)), None, "{bad}");
        }
        assert_eq!(content_range(None), None);
    }

    #[test]
    fn validador_prefiere_etag_fuerte() {
        assert_eq!(
            validator(Some("\"v1\""), Some("Tue")),
            Some("\"v1\"".into())
        );
        assert_eq!(
            validator(Some("W/\"v1\""), Some(" Tue ")),
            Some("Tue".into())
        );
        assert_eq!(validator(None, None), None);
        assert_eq!(validator(Some(""), Some("")), None);
    }

    #[test]
    fn error_de_portal_con_el_texto_del_contrato() {
        let error = web_page_error(
            "http://127.0.0.1:9/fgtauth?magic=1",
            "http://127.0.0.1:9/app.zip?t=1",
            Some(7),
        );
        assert_eq!(
            error.to_string(),
            "La red devolvió una página web en lugar del archivo (127.0.0.1); puede ser el portal de autenticación o un bloqueo del proxy. Pulsa INTERNET y reintenta."
        );
        let Error::Integrity {
            url,
            size,
            host,
            portal,
            ..
        } = error
        else {
            panic!("se esperaba Integrity");
        };
        assert_eq!(url, "http://127.0.0.1:9/app.zip");
        assert_eq!(size, Some(7));
        assert_eq!(host, "127.0.0.1");
        assert!(portal);
    }
}
