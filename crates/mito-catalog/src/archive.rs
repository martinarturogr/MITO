//! Lectura de manifiestos de un ZIP de repositorio sin extraerlo (`_archive_manifests`).
//!
//! Contrato: `docs/spec/modules/buckets.md` («`_archive_manifests`»). Se valida todo el
//! archivo, pero solo se leen en memoria los `.json` de los manifiestos: no se escribe
//! nada en disco.
//!
//! Diferencia deliberada (ADR 0008, punto 1): el límite de 12 GiB se aplica también a los
//! bytes realmente leídos, no solo al tamaño declarado en el ZIP.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use mito_core::Message;
use mito_core::storage::{PortablePath, identifier, portable_path};
use serde_json::{Map, Value};
use zip::ZipArchive;

use crate::error::{Error, Result, bail};
use crate::pypath;

/// Límites compartidos con la extracción de `mito-transfer` (150 000 entradas, 12 GiB).
pub use mito_transfer::archives::{MAX_ENTRIES, MAX_EXPANDED};

const S_IFMT: u32 = 0o170_000;
const S_IFLNK: u32 = 0o120_000;

fn invalid_zip(archive: &Path, error: impl std::fmt::Display) -> Error {
    Error::domain(
        Message::new("Archivo ZIP no válido: {p1}: {p2}")
            .arg("p1", archive.display())
            .arg("p2", error),
    )
}

fn too_large() -> Error {
    Error::domain(Message::new("Archivo demasiado grande para extraer"))
}

/// Entrada validada del archivo.
struct Item {
    index: usize,
    name: String,
    path: PortablePath,
    is_dir: bool,
}

impl Item {
    /// `str(path)` de `PureWindowsPath`: los segmentos unidos con `\`.
    fn joined(&self) -> String {
        self.path.segments().join("\\")
    }
}

/// Lee `nombre → manifiesto` de un ZIP de repositorio GitHub (o equivalente).
///
/// Acepta `raíz/app.json` y `raíz/bucket/app.json` (si existe la carpeta `bucket`), y
/// ignora el resto de archivos. Los manifiestos se leen sin BOM.
///
/// # Errors
///
/// `Archivo demasiado grande para extraer`, `Entrada duplicada: …`, `Los enlaces del
/// archivo no están permitidos`, `Ruta no portable`/`Ruta reservada`, `Ruta fuera de su
/// raíz` (más de una raíz), `Nombre no válido` para un manifiesto, y un error de dominio
/// con el archivo y el motivo si el ZIP o un manifiesto no son válidos.
pub fn archive_manifests(archive: &Path) -> Result<Map<String, Value>> {
    let file = File::open(archive).map_err(|e| mito_core::Error::io(archive, e))?;
    let mut zip = ZipArchive::new(file).map_err(|e| invalid_zip(archive, e))?;
    if zip.len() > MAX_ENTRIES {
        return Err(too_large());
    }
    let mut declared: u64 = 0;
    let mut items = Vec::with_capacity(zip.len());
    let mut seen = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|e| invalid_zip(archive, e))?;
        declared = declared.saturating_add(entry.size());
        let name = entry
            .name()
            .map_err(|e| invalid_zip(archive, e))?
            .into_owned();
        let is_dir = entry.is_dir();
        let attributes = entry.external_attributes();
        drop(entry);
        let path = portable_path(&name)?;
        let key = caseless::default_case_fold_str(&path.segments().join("\\"));
        if !seen.insert(key) {
            return bail(Message::new("Entrada duplicada: {p1}").arg("p1", &name));
        }
        if (attributes >> 16) & S_IFMT == S_IFLNK {
            return bail(Message::new("Los enlaces del archivo no están permitidos"));
        }
        let Some(root) = path.segments().first() else {
            return bail(Message::new("Ruta no portable: {p1}").arg("p1", &name));
        };
        roots.insert(root.clone());
        items.push(Item {
            index,
            name,
            path,
            is_dir,
        });
    }
    if declared > MAX_EXPANDED {
        return Err(too_large());
    }
    if roots.len() != 1 {
        return bail(Message::new("Ruta fuera de su raíz: {p1}").arg("p1", archive.display()));
    }
    let is_bucket_folder = |item: &Item| {
        item.path
            .segments()
            .get(1)
            .is_some_and(|part| caseless::default_case_fold_str(part) == "bucket")
    };
    let nested = items
        .iter()
        .any(|item| is_bucket_folder(item) && (item.path.segments().len() > 2 || item.is_dir));
    items.sort_by_cached_key(Item::joined);
    let mut manifests = Map::new();
    let mut read: u64 = 0;
    for item in items {
        let last = item.path.segments().last().map_or("", String::as_str);
        if item.is_dir || !pypath::suffix(last).eq_ignore_ascii_case(".json") {
            continue;
        }
        if item.path.segments().len() != if nested { 3 } else { 2 } {
            continue;
        }
        if nested && !is_bucket_folder(&item) {
            continue;
        }
        let name = identifier(pypath::stem(last))?;
        let mut entry = zip
            .by_index(item.index)
            .map_err(|e| invalid_zip(archive, e))?;
        let mut bytes = Vec::new();
        let limit = MAX_EXPANDED.saturating_sub(read).saturating_add(1);
        entry
            .by_ref()
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(|e| invalid_zip(archive, e))?;
        read = read.saturating_add(bytes.len() as u64);
        if read > MAX_EXPANDED {
            return Err(too_large());
        }
        let content = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        let manifest = serde_json::from_slice(content).map_err(|error| {
            Error::domain(
                Message::new("JSON no válido en {p1}: {p2}")
                    .arg("p1", &item.name)
                    .arg("p2", error),
            )
        })?;
        manifests.insert(name, manifest);
    }
    Ok(manifests)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;

    use serde_json::json;
    use zip::write::SimpleFileOptions;

    use super::*;

    /// Entrada de un ZIP de prueba: nombre, contenido y atributos externos opcionales.
    pub(crate) struct Member<'a> {
        pub name: &'a str,
        pub content: &'a [u8],
        pub attributes: Option<u32>,
    }

    pub(crate) fn member<'a>(name: &'a str, content: &'a [u8]) -> Member<'a> {
        Member {
            name,
            content,
            attributes: None,
        }
    }

    /// Escribe un ZIP con las entradas dadas, tal cual (sin sanear los nombres).
    pub(crate) fn write_zip(path: &Path, members: &[Member<'_>]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for member in members {
            let mut options = SimpleFileOptions::default();
            if let Some(attributes) = member.attributes {
                options = options.external_attributes(attributes);
            }
            if member.name.ends_with('/') {
                zip.add_directory(member.name, options).unwrap();
            } else {
                zip.start_file(member.name, options).unwrap();
                zip.write_all(member.content).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn with_bom(value: &Value) -> Vec<u8> {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend(serde_json::to_vec(value).unwrap());
        bytes
    }

    #[test]
    fn layouts_plano_y_con_carpeta_bucket_sin_extraer_nada() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("source.zip");
        let manifest = with_bom(&json!({"version": "1"}));
        for folder in ["repo/", "repo/bucket/"] {
            let name = format!("{folder}tool.json");
            write_zip(
                &archive,
                &[
                    member(&name, &manifest),
                    member("repo/docs/ignored.json", b"invalid json"),
                ],
            );
            let manifests = archive_manifests(&archive).unwrap();
            assert_eq!(Value::Object(manifests), json!({"tool": {"version": "1"}}));
            let names: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert_eq!(names, std::slice::from_ref(&archive));
        }
    }

    #[test]
    fn con_carpeta_bucket_se_ignoran_los_json_de_la_raiz() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("source.zip");
        write_zip(
            &archive,
            &[
                member("repo/bucket/", b""),
                member("repo/bucket/a.json", b"{\"version\":\"1\"}"),
                member("repo/b.json", b"{\"version\":\"2\"}"),
                member("repo/bucket/sub/c.json", b"{\"version\":\"3\"}"),
                member("repo/bucket/readme.md", b"x"),
            ],
        );
        let manifests = archive_manifests(&archive).unwrap();
        assert_eq!(Value::Object(manifests), json!({"a": {"version": "1"}}));
    }

    #[test]
    fn zip_malos_se_rechazan_con_el_mensaje_del_original() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("bad.zip");
        let symlink = (0o120_777_u32) << 16;
        let cases: [(&str, Option<u32>, &str); 5] = [
            ("repo/../escape", None, "Ruta no portable: repo/../escape"),
            ("repo/CON", None, "Ruta reservada: repo/CON"),
            (
                "repo/bucket/TOOL.json",
                None,
                "Entrada duplicada: repo/bucket/TOOL.json",
            ),
            (
                "repo/link",
                Some(symlink),
                "Los enlaces del archivo no están permitidos",
            ),
            ("other/x.json", None, "Ruta fuera de su raíz: "),
        ];
        for (name, attributes, expected) in cases {
            write_zip(
                &archive,
                &[
                    member("repo/bucket/tool.json", b"{\"version\":\"2\"}"),
                    Member {
                        name,
                        content: b"{}",
                        attributes,
                    },
                ],
            );
            let error = archive_manifests(&archive).unwrap_err().to_string();
            assert!(error.starts_with(expected), "{name}: {error}");
        }
    }

    #[test]
    fn nombre_de_manifiesto_y_json_invalidos() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("x.zip");
        write_zip(&archive, &[member("repo/-x.json", b"{}")]);
        assert_eq!(
            archive_manifests(&archive).unwrap_err().to_string(),
            "Nombre no válido: '-x'"
        );
        write_zip(&archive, &[member("repo/a.json", b"{")]);
        let error = archive_manifests(&archive).unwrap_err().to_string();
        assert!(
            error.starts_with("JSON no válido en repo/a.json: "),
            "{error}"
        );
        std::fs::write(&archive, b"no es un zip").unwrap();
        let error = archive_manifests(&archive).unwrap_err().to_string();
        assert!(error.starts_with("Archivo ZIP no válido: "), "{error}");
    }

    #[test]
    fn los_nombres_se_comparan_sin_distinguir_mayusculas_ni_separadores() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("x.zip");
        write_zip(&archive, &[member("Repo/A.JSON", b"{\"version\":\"1\"}")]);
        let manifests = archive_manifests(&archive).unwrap();
        assert_eq!(Value::Object(manifests), json!({"a": {"version": "1"}}));
    }
}
