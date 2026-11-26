//! Nombre local estable de una descarga (`download_filename`).
//!
//! Contrato: `docs/spec/modules/transfer.md` («Nombre local derivado de la URL»).

use mito_core::Message;
use mito_core::error::{Result, bail};
use mito_core::message::py_repr;
use mito_core::storage::portable_path;

/// Partes de una URL como las separa `urllib.parse.urlsplit` (solo las que se usan).
struct SplitUrl<'a> {
    path: &'a str,
    fragment: &'a str,
}

fn split_url(url: &str) -> SplitUrl<'_> {
    let (rest, fragment) = url.split_once('#').unwrap_or((url, ""));
    let rest = rest.split_once('?').map_or(rest, |(before, _)| before);
    // Esquema: letra seguida de letras, dígitos, `+`, `-` o `.` hasta `:`.
    let rest = match rest.split_once(':') {
        Some((scheme, after))
            if scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            after
        }
        _ => rest,
    };
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => authority_and_path
            .find('/')
            .map_or("", |slash| &authority_and_path[slash..]),
        None => rest,
    };
    SplitUrl { path, fragment }
}

/// Decodifica `%XX` como `urllib.parse.unquote`: bytes UTF-8 y sustitución de las
/// secuencias inválidas por U+FFFD.
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            // Dos dígitos hexadecimales siempre caben en un byte.
            out.push(u8::try_from(high * 16 + low).unwrap_or_default());
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Nombre local de la descarga: alias `#/nombre` o último segmento de la ruta,
/// decodificado y validado como nombre de archivo de Windows. Se ignoran la query, las
/// redirecciones y `Content-Disposition`.
///
/// # Errors
///
/// Los de `portable_path` y `Nombre no válido` si queda vacío, es `.`/`..` o contiene
/// separadores.
pub fn download_filename(url: &str) -> Result<String> {
    let parts = split_url(url);
    let raw = match parts.fragment.strip_prefix('/') {
        Some(alias) => alias,
        None => parts.path.rsplit('/').next().unwrap_or_default(),
    };
    let name = unquote(raw);
    portable_path(&name)?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return bail(Message::new("Nombre no válido: {p1}").arg("p1", py_repr(&name)));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_validos() {
        assert_eq!(
            download_filename("https://example.org/Tool%2EEXE?x=1").unwrap(),
            "Tool.EXE"
        );
        assert_eq!(
            download_filename("https://e.org/a/b/app.zip").unwrap(),
            "app.zip"
        );
        assert_eq!(
            download_filename("https://e.org/dl?id=3#/setup.exe").unwrap(),
            "setup.exe"
        );
        assert_eq!(
            download_filename("https://e.org/x.7z#frag").unwrap(),
            "x.7z"
        );
        assert_eq!(
            download_filename("https://e.org/Caf%C3%A9.zip").unwrap(),
            "Café.zip"
        );
        assert_eq!(
            download_filename("https://e.org/bad%ZZ.zip").unwrap(),
            "bad%ZZ.zip"
        );
    }

    #[test]
    fn nombres_rechazados_como_el_original() {
        let cases = [
            ("https://example.org/", "Nombre no válido: ''"),
            ("https://example.org", "Nombre no válido: ''"),
            ("https://e.org/x#/../escape", "Ruta no portable: ../escape"),
            ("https://e.org/x#/dir/file", "Nombre no válido: 'dir/file'"),
            ("https://e.org/x#/C%3Aevil", "Ruta no portable: C:evil"),
            ("https://e.org/x#/NUL", "Ruta reservada: NUL"),
            ("https://e.org/x#/a%5Cb", "Nombre no válido: 'a\\\\b'"),
            ("https://e.org/x#/bad%3Fname", "Ruta reservada: bad?name"),
            ("https://e.org/x#/..", "Ruta no portable: .."),
            ("https://e.org/x#/file.", "Ruta reservada: file."),
        ];
        for (url, expected) in cases {
            assert_eq!(
                download_filename(url).unwrap_err().to_string(),
                expected,
                "{url}"
            );
        }
    }
}
