//! Nombres, rutas confinadas y JSON atómico (`storage.py` del original).
//!
//! Contrato: `docs/spec/modules/storage.md`. El lock de mutación (`mutation`) está
//! en [`crate::lock`].

use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result, bail};
use crate::message::{Message, py_repr};

/// Nombres de dispositivo reservados por Windows. Como el original, incluye `COM0` y `LPT0`.
const RESERVED: [&str; 24] = [
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Longitud máxima de un identificador (`[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}`).
pub const IDENTIFIER_MAX: usize = 80;

/// Longitud máxima, en caracteres, de un nombre de comando sin `.cmd`.
pub const COMMAND_NAME_MAX: usize = 240;

/// ¿Es reservado el nombre? Compara el texto anterior al primer `.`, sin mayúsculas.
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Valida un identificador de profile, bucket o app y lo devuelve en minúsculas.
///
/// # Errors
///
/// `Nombre no válido` si no cumple el patrón, `Nombre reservado` si termina en `.` o
/// usa un nombre de dispositivo de Windows.
pub fn identifier(value: &str) -> Result<String> {
    let mut chars = value.chars();
    let valid = value.len() <= IDENTIFIER_MAX
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !valid {
        return bail(Message::new("Nombre no válido: {p1}").arg("p1", py_repr(value)));
    }
    if value.ends_with('.') || is_reserved(value) {
        return bail(Message::new("Nombre reservado: {p1}").arg("p1", value));
    }
    Ok(value.to_ascii_lowercase())
}

/// Ruta relativa validada con la sintaxis de Windows, sin consultar el disco.
///
/// Se guarda como lista de segmentos: los separadores `\` y `/` son equivalentes y se
/// descartan los segmentos vacíos y `.`, igual que `PureWindowsPath`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PortablePath {
    segments: Vec<String>,
}

impl PortablePath {
    /// Segmentos de la ruta. Vacío para `""` o `"."`.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Indica si la ruta no tiene segmentos (equivale a la raíz).
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Une la ruta a `root` sin comprobar el confinamiento (ver [`inside`]).
    pub fn join_to(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        path.extend(&self.segments);
        path
    }
}

/// Valida una ruta relativa de manifiesto (`portable_path`).
///
/// # Errors
///
/// `Ruta no portable` si es absoluta, tiene unidad, `..` o `:`; `Ruta reservada` si un
/// segmento tiene `<>"|?*` o caracteres de control, termina en espacio o punto, o usa
/// un nombre de dispositivo.
pub fn portable_path(relative: &str) -> Result<PortablePath> {
    let not_portable = || bail(Message::new("Ruta no portable: {p1}").arg("p1", relative));
    if relative.starts_with(['/', '\\']) {
        return not_portable();
    }
    let segments: Vec<String> = relative
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != ".")
        .map(str::to_owned)
        .collect();
    if segments.iter().any(|s| s == ".." || s.contains(':')) {
        return not_portable();
    }
    let reserved = |s: &String| {
        s.chars()
            .any(|c| "<>\"|?*".contains(c) || u32::from(c) < 32)
            || s.ends_with([' ', '.'])
            || is_reserved(s)
    };
    if segments.iter().any(reserved) {
        return bail(Message::new("Ruta reservada: {p1}").arg("p1", relative));
    }
    Ok(PortablePath { segments })
}

/// Resuelve enlaces del prefijo existente de `path` y añade el resto sin tocar el disco,
/// como `Path.resolve(strict=False)` de Python. `path` no puede contener `..`.
fn resolve_lenient(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = dunce::canonicalize(&existing) {
            let mut out = resolved;
            out.extend(rest.iter().rev());
            return out;
        }
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => {
                // Nada existe: normalización léxica.
                return path
                    .components()
                    .filter(|c| !matches!(c, Component::CurDir))
                    .collect();
            }
        }
    }
}

/// Une `relative` a `root` y garantiza que el resultado, resolviendo enlaces, queda
/// dentro de `root` (`inside`).
///
/// # Errors
///
/// Los de [`portable_path`], o `Ruta fuera de su raíz` si un enlace (junction, symlink)
/// apunta fuera.
pub fn inside(root: &Path, relative: &str) -> Result<PathBuf> {
    let portable = portable_path(relative)?;
    let path = portable.join_to(root);
    if !resolve_lenient(&path).starts_with(resolve_lenient(root)) {
        return bail(Message::new("Ruta fuera de su raíz: {p1}").arg("p1", relative));
    }
    Ok(path)
}

/// Valida un nombre de comando (un único nombre de archivo Windows) y lo devuelve en
/// minúsculas.
///
/// # Errors
///
/// `Nombre de comando no válido` si está vacío, supera 240 caracteres, contiene
/// separadores o es `.`; además los errores de [`portable_path`].
pub fn command_name(value: &str) -> Result<String> {
    let invalid =
        || bail(Message::new("Nombre de comando no válido: {p1}").arg("p1", py_repr(value)));
    if value.trim().is_empty()
        || value.chars().count() > COMMAND_NAME_MAX
        || value.contains(['/', '\\'])
    {
        return invalid();
    }
    portable_path(&format!("{value}.cmd"))?;
    portable_path(value)?;
    if value == "." || value == ".." {
        return invalid();
    }
    Ok(value.to_lowercase())
}

/// Lee un JSON UTF-8, con o sin BOM.
///
/// # Errors
///
/// [`Error::Io`] si no se puede leer y [`Error::Json`] si el contenido no es válido.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    let content = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    serde_json::from_slice(content).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Escribe `value` como JSON de forma atómica, con el formato del original: UTF-8 sin
/// BOM, sangría de 2 espacios, orden de inserción, sin escapar no-ASCII y `\n` final.
///
/// Escribe un temporal `*.tmp` en la misma carpeta, hace `fsync` y lo renombra sobre
/// el destino: el archivo contiene siempre la versión anterior o la nueva.
///
/// # Errors
///
/// [`Error::Json`] si no se puede serializar y [`Error::Io`] si falla el disco.
pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let mut content = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })?;
    content.push(b'\n');
    write_atomic(path, &content)
}

/// Escribe `content` de forma atómica (temporal en la misma carpeta, `fsync` y rename).
///
/// # Errors
///
/// [`Error::Io`] si falla el disco.
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    let mut temporary = tempfile::Builder::new()
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|e| Error::io(parent, e))?;
    temporary
        .write_all(content)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|e| Error::io(temporary.path(), e))?;
    temporary
        .persist(path)
        .map_err(|e| Error::io(path, e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn message(result: Result<impl std::fmt::Debug>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn identificadores() {
        assert_eq!(identifier("Java-21_x.y").unwrap(), "java-21_x.y");
        assert_eq!(
            message(identifier("bad name")),
            "Nombre no válido: 'bad name'"
        );
        assert_eq!(message(identifier("")), "Nombre no válido: ''");
        assert_eq!(message(identifier("-x")), "Nombre no válido: '-x'");
        assert_eq!(message(identifier("ñ")), "Nombre no válido: 'ñ'");
        assert!(identifier(&"a".repeat(80)).is_ok());
        assert!(identifier(&"a".repeat(81)).is_err());
        assert_eq!(message(identifier("con")), "Nombre reservado: con");
        assert_eq!(
            message(identifier("Com0.txt")),
            "Nombre reservado: Com0.txt"
        );
        assert_eq!(message(identifier("lpt9")), "Nombre reservado: lpt9");
        assert_eq!(message(identifier("app.")), "Nombre reservado: app.");
        assert!(identifier("console").is_ok());
    }

    #[test]
    fn rutas_portables() {
        assert_eq!(portable_path("a/b\\c").unwrap().segments(), ["a", "b", "c"]);
        assert_eq!(portable_path("./a//b/").unwrap().segments(), ["a", "b"]);
        assert!(portable_path("").unwrap().is_empty());
        assert!(portable_path(".").unwrap().is_empty());
        for bad in [
            "../x",
            "a/../b",
            "/rooted",
            "\\rooted",
            "C:/absolute",
            "C:x",
            "file:stream",
            "\\\\server\\share",
        ] {
            assert_eq!(
                message(portable_path(bad)),
                format!("Ruta no portable: {bad}"),
                "{bad}"
            );
        }
        for bad in [
            "a<b",
            "a|b",
            "q?",
            "star*",
            "x\u{1f}",
            "trailing.",
            "space ",
            "CON",
            "dir/nul.txt",
            "lpt1",
        ] {
            assert_eq!(
                message(portable_path(bad)),
                format!("Ruta reservada: {bad}"),
                "{bad}"
            );
        }
        assert!(portable_path("CONSOLE/x.txt").is_ok());
    }

    #[test]
    fn nombres_de_comando() {
        assert_eq!(command_name("MyTool").unwrap(), "mytool");
        assert_eq!(
            message(command_name("a/b")),
            "Nombre de comando no válido: 'a/b'"
        );
        assert_eq!(
            message(command_name("   ")),
            "Nombre de comando no válido: '   '"
        );
        assert_eq!(
            message(command_name(".")),
            "Nombre de comando no válido: '.'"
        );
        assert_eq!(message(command_name("..")), "Ruta no portable: ..");
        assert_eq!(message(command_name("nul")), "Ruta reservada: nul.cmd");
        assert!(command_name(&"a".repeat(240)).is_ok());
        assert!(command_name(&"a".repeat(241)).is_err());
    }

    #[test]
    fn inside_confina_y_admite_rutas_inexistentes() {
        let root = tempfile::tempdir().unwrap();
        let path = inside(root.path(), "a/b/c.txt").unwrap();
        assert_eq!(path, root.path().join("a").join("b").join("c.txt"));
        assert_eq!(inside(root.path(), "").unwrap(), root.path());
        assert_eq!(
            message(inside(root.path(), "../x")),
            "Ruta no portable: ../x"
        );
    }

    #[cfg(windows)]
    #[test]
    fn inside_rechaza_enlaces_que_salen_de_la_raiz() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join("link");
        // Un symlink de directorio puede requerir privilegios: se omite si no se puede crear.
        if std::os::windows::fs::symlink_dir(outside.path(), &link).is_err() {
            return;
        }
        assert_eq!(
            message(inside(root.path(), "link/x")),
            "Ruta fuera de su raíz: link/x"
        );
    }

    #[test]
    fn json_con_el_formato_del_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("value.json");
        let value = serde_json::json!({"z": "ñ", "a": [1, 2], "e": {}, "l": []});
        write_json(&path, &value).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "{\n  \"z\": \"ñ\",\n  \"a\": [\n    1,\n    2\n  ],\n  \"e\": {},\n  \"l\": []\n}\n"
        );
        let back: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(back, value);
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn read_json_tolera_bom_y_distingue_errores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.json");
        fs::write(&path, b"\xEF\xBB\xBF{\"a\": 1}").unwrap();
        let value: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(value["a"], 1);
        fs::write(&path, b"{").unwrap();
        assert!(matches!(
            read_json::<serde_json::Value>(&path),
            Err(Error::Json { .. })
        ));
        assert!(matches!(
            read_json::<serde_json::Value>(&dir.path().join("missing.json")),
            Err(Error::Io { .. })
        ));
    }

    proptest! {
        #[test]
        fn una_ruta_aceptada_nunca_escapa_ni_tiene_unidad(input in "[a-zA-Z0-9./\\\\:<>|?* ]{0,24}") {
            if let Ok(path) = portable_path(&input) {
                for segment in path.segments() {
                    prop_assert!(segment != ".." && segment != ".");
                    prop_assert!(!segment.contains(':'));
                    prop_assert!(!segment.ends_with(['.', ' ']));
                    prop_assert!(!segment.chars().any(|c| "<>\"|?*".contains(c)));
                }
            }
        }

        #[test]
        fn inside_siempre_queda_bajo_la_raiz(input in "[a-z./\\\\]{0,16}") {
            let root = tempfile::tempdir().unwrap();
            if let Ok(path) = inside(root.path(), &input) {
                prop_assert!(path.starts_with(root.path()));
            }
        }
    }
}
