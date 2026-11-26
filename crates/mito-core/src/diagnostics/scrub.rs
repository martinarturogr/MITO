//! Saneado de texto antes de escribirlo en el log (`diagnostics.scrub`).
//!
//! El log nunca debe contener credenciales de URL, queries firmadas, tokens, pares
//! `clave=secreto`, la ruta de la instancia ni el perfil de usuario de Windows.

use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{NoExpand, Regex};

use crate::layout::Layout;

/// Longitud máxima, en caracteres, de un texto saneado.
pub const MAX_CHARS: usize = 12_000;

/// Construye una expresión regular fija; un patrón inválido es un fallo de programación
/// que los tests detectan.
fn regex(pattern: &str) -> Regex {
    // Los patrones son constantes y los tests los compilan todos.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("patrón de saneado válido")
}

static URL: LazyLock<Regex> = LazyLock::new(|| regex(r#"(?i)https?://[^\s<>"']+"#));
static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\b(?:ph[cp]_|gh[pousr]_)[A-Za-z0-9_]+"));
static BEARER: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\bBearer\s+\S+"));
static USER_PROFILE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+"));
static SECRET_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r#"(?i)(["']?(?:password|passwd|pwd|token|api[_-]?key|secret|authorization|cookie)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;]+)"#,
    )
});

/// Ruta de la instancia tal como la ve el saneado (`str(Path(root).resolve())`).
fn resolved(path: &Path) -> PathBuf {
    dunce::canonicalize(path)
        .or_else(|_| std::path::absolute(path).map(|p| dunce::simplified(&p).to_path_buf()))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Ruta de la instancia resuelta, como texto (la usa también `locations`).
pub(crate) fn resolved_text(path: &Path) -> String {
    resolved(path).to_string_lossy().into_owned()
}

/// Sanea `value` para el log local.
///
/// En este orden: URLs (se conservan esquema, host y ruta; se quitan credenciales y
/// fragmento y la query pasa a `[redacted]`), tokens de GitHub/PostHog, `Bearer`,
/// `X:\Users\<nombre>`, pares `password=…`/`token:…`, la raíz de la instancia
/// (`<MITO>`) y `USERPROFILE` (`<USER>`) con `\` y con `/`, y recorte a 12 000
/// caracteres.
pub fn scrub(layout: &Layout, value: &str) -> String {
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    scrub_with(&resolved_text(layout.root()), &profile, value)
}

fn scrub_with(root: &str, profile: &str, value: &str) -> String {
    let mut text = URL
        .replace_all(value, |found: &regex::Captures<'_>| sanitize_url(&found[0]))
        .into_owned();
    text = TOKEN
        .replace_all(&text, NoExpand("[redacted]"))
        .into_owned();
    text = BEARER
        .replace_all(&text, NoExpand("Bearer [redacted]"))
        .into_owned();
    text = USER_PROFILE
        .replace_all(&text, NoExpand("<USER>"))
        .into_owned();
    text = SECRET_PAIR
        .replace_all(&text, "${1}[redacted]")
        .into_owned();
    for (path, label) in [(root, "<MITO>"), (profile, "<USER>")] {
        if path.is_empty() {
            continue;
        }
        for variant in [path.to_owned(), path.replace('\\', "/")] {
            let pattern = format!("(?i){}", regex::escape(&variant));
            if let Ok(re) = Regex::new(&pattern) {
                text = re.replace_all(&text, NoExpand(label)).into_owned();
            }
        }
    }
    if let Some((end, _)) = text.char_indices().nth(MAX_CHARS) {
        text.truncate(end);
    }
    text
}

/// Quita credenciales, query y fragmento de una URL (`urlsplit`/`urlunsplit`).
/// `[url]` si la URL no se puede interpretar, como cuando `urlsplit` lanza `ValueError`.
fn sanitize_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "[url]".to_owned();
    };
    let netloc_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (netloc, after) = rest.split_at(netloc_end);
    if !bracketed_host_is_valid(netloc) {
        return "[url]".to_owned();
    }
    let host = netloc.rsplit('@').next().unwrap_or_default();
    let without_fragment = after.split('#').next().unwrap_or_default();
    let (path, query) = without_fragment
        .split_once('?')
        .unwrap_or((without_fragment, ""));
    let mut out = format!("{}://{host}{path}", scheme.to_ascii_lowercase());
    if !query.is_empty() {
        out.push_str("?[redacted]");
    }
    out
}

/// Reproduce las comprobaciones de `urlsplit` sobre hosts entre corchetes.
fn bracketed_host_is_valid(netloc: &str) -> bool {
    let Some(open) = netloc.find('[') else {
        // Un corchete de cierre suelto también es un error.
        return !netloc.contains(']');
    };
    if !netloc.contains(']') {
        return false;
    }
    let host = netloc[open + 1..].split(']').next().unwrap_or_default();
    if let Some(future) = host.strip_prefix('v') {
        let Some((hex, tail)) = future.split_once('.') else {
            return false;
        };
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && !tail.is_empty();
    }
    host.split('%')
        .next()
        .unwrap_or_default()
        .parse::<Ipv6Addr>()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn clean(text: &str) -> String {
        scrub_with("C:\\mito\\inst", "C:\\Users\\Persona", text)
    }

    #[test]
    fn todos_los_patrones_compilan() {
        for re in [&*URL, &*TOKEN, &*BEARER, &*USER_PROFILE, &*SECRET_PAIR] {
            assert_ne!(re.as_str(), "");
        }
    }

    #[test]
    fn las_urls_pierden_credenciales_query_y_fragmento() {
        assert_eq!(
            clean("Download failed https://user:password@example.com/path?token=SECRET#frag fin"),
            "Download failed https://example.com/path?[redacted] fin"
        );
        assert_eq!(
            clean("HTTP://Example.com:8080/a"),
            "http://Example.com:8080/a"
        );
        assert_eq!(clean("https://example.com/a?"), "https://example.com/a");
        assert_eq!(clean("https://example.com"), "https://example.com");
        assert_eq!(clean("https://u@h/x#a?b"), "https://h/x");
        assert_eq!(clean("(https://example.com/x)"), "(https://example.com/x)");
    }

    #[test]
    fn urls_no_interpretables_se_sustituyen() {
        assert_eq!(clean("ver https://[::1/x ya"), "ver [url] ya");
        assert_eq!(clean("https://[nada]/x"), "[url]");
        assert_eq!(clean("https://[::1]:80/x"), "https://[::1]:80/x");
    }

    #[test]
    fn tokens_bearer_y_pares_secretos() {
        assert_eq!(clean("x phc_EXAMPLE_1 y"), "x [redacted] y");
        assert_eq!(clean("GHP_abc ghs_def"), "[redacted] [redacted]");
        assert_eq!(clean("Bearer SECRET"), "Bearer [redacted]");
        assert_eq!(
            clean(r#"password="two words" fin"#),
            "password=[redacted] fin"
        );
        assert_eq!(clean("pwd: 'a b', ok"), "pwd: [redacted], ok");
        assert_eq!(clean("API-KEY=abc;"), "API-KEY=[redacted];");
        assert_eq!(clean(r#""token": "abc""#), r#""token": [redacted]"#);
        assert_eq!(
            clean("Authorization: Bearer SECRET"),
            "Authorization: [redacted] [redacted]"
        );
        assert_eq!(clean("sin secretos"), "sin secretos");
    }

    #[test]
    fn rutas_de_usuario_y_de_la_instancia() {
        assert_eq!(clean("c:/Users/Someone/file"), "<USER>/file");
        assert_eq!(clean("D:\\USERS\\x\\y"), "<USER>\\y");
        assert_eq!(
            clean("en C:\\MITO\\Inst\\core y c:/mito/inst/core"),
            "en <MITO>\\core y <MITO>/core"
        );
        assert_eq!(clean("C:\\Users\\Persona\\a"), "<USER>\\a");
        assert_eq!(scrub_with("", "", "igual"), "igual");
    }

    #[test]
    fn scrub_quita_la_raiz_real_de_la_instancia() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let text = scrub(
            &layout,
            &format!(
                "Download failed https://user:password@example.com/path?token=SECRET \
                 password=\"two words\" Authorization: Bearer SECRET phc_EXAMPLE {} C:/Users/Someone/file",
                dir.path().join("file").display()
            ),
        );
        for secret in [
            "SECRET",
            "two words",
            "user:password",
            "phc_EXAMPLE",
            "Someone",
        ] {
            assert!(!text.contains(secret), "{secret}: {text}");
        }
        assert!(!text.contains(&*dunce::canonicalize(dir.path()).unwrap().to_string_lossy()));
        assert!(text.contains("Download failed"));
        assert!(text.contains("example.com/path"));
    }

    #[test]
    fn el_resultado_se_recorta_a_12000_caracteres() {
        let long = "ñ".repeat(13_000);
        assert_eq!(clean(&long).chars().count(), MAX_CHARS);
        let exact = "a".repeat(MAX_CHARS);
        assert_eq!(clean(&exact), exact);
    }

    proptest! {
        #[test]
        fn nunca_sobrevive_un_secreto_tras_password(secret in "[A-Za-z0-9]{6,12}") {
            let text = clean(&format!("fallo password={secret} y token: {secret}"));
            prop_assert!(!text.contains(&secret));
        }

        #[test]
        fn scrub_nunca_supera_el_limite(text in ".{0,200}") {
            prop_assert!(clean(&text).chars().count() <= MAX_CHARS);
        }
    }
}
