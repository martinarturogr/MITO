//! Extracción de ZIP y `.nupkg` (`extract_zip` de `transfer.py`).

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use mito_core::Message;
use mito_core::storage::inside;
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::result::ZipError;

use super::Limits;
use super::sink::Sink;
use crate::error::{Error, Result};

const TOO_LARGE: &str = "Archivo demasiado grande para extraer";

/// Entrada validada, lista para escribir.
struct Planned {
    index: usize,
    name: String,
    is_dir: bool,
}

pub(crate) fn zip_error(archive: &Path, error: ZipError) -> Error {
    match error {
        ZipError::Io(source) => mito_core::Error::io(archive, source).into(),
        other => {
            Error::domain(Message::new("Archivo ZIP no válido: {p1}").arg("p1", other.to_string()))
        }
    }
}

/// Abre `archive` como ZIP; `None` si no lo es (`zipfile.is_zipfile`).
pub(crate) fn open(archive: &Path) -> Option<ZipArchive<BufReader<File>>> {
    let file = File::open(archive).ok()?;
    ZipArchive::new(BufReader::new(file)).ok()
}

/// ¿Es un ZIP el contenido de `archive`?
pub fn is_zip(archive: &Path) -> bool {
    open(archive).is_some()
}

fn method_is_supported(method: CompressionMethod) -> bool {
    matches!(
        method,
        CompressionMethod::Stored
            | CompressionMethod::Deflated
            | CompressionMethod::Deflate64
            | CompressionMethod::Bzip2
            | CompressionMethod::Lzma
    )
}

/// Extrae un ZIP con los límites por defecto. Ver [`extract_zip_with`].
///
/// # Errors
///
/// Los de [`extract_zip_with`].
pub fn extract_zip(archive: &Path, destination: &Path) -> Result<()> {
    extract_zip_with(archive, destination, &Limits::default())
}

/// Extrae un ZIP validándolo **por completo antes de escribir nada**: número de
/// entradas, tamaño declarado, rutas (`inside`), duplicados sin distinguir mayúsculas,
/// enlaces simbólicos y archivos cifrados. Durante la escritura, además, limita los
/// bytes realmente descomprimidos a `limits.max_expanded`; si se supera, o si falla
/// cualquier escritura, se deshace lo escrito.
///
/// # Errors
///
/// Errores de dominio con los textos del original (`Archivo demasiado grande para
/// extraer`, `Entrada duplicada: {p1}`, `Los enlaces del archivo no están permitidos`,
/// `Ruta no portable`…), [`Error::UnsupportedZipMethod`] si alguna entrada usa un
/// método de compresión no soportado (se devuelve **después** de validar las rutas y
/// antes de escribir), y errores de E/S.
pub fn extract_zip_with(archive: &Path, destination: &Path, limits: &Limits) -> Result<()> {
    let file = File::open(archive).map_err(|e| mito_core::Error::io(archive, e))?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|e| zip_error(archive, e))?;
    let plan = plan(&mut zip, archive, destination, limits)?;
    let mut sink = Sink::new(archive, destination, limits.max_expanded, TOO_LARGE);
    let result = write(&mut zip, archive, destination, &plan, &mut sink);
    if result.is_err() {
        sink.rollback();
    }
    result
}

fn plan(
    zip: &mut ZipArchive<BufReader<File>>,
    archive: &Path,
    destination: &Path,
    limits: &Limits,
) -> Result<Vec<Planned>> {
    let too_large = || Error::domain(Message::new(TOO_LARGE));
    if zip.len() > limits.max_entries {
        return Err(too_large());
    }
    let mut total = 0_u64;
    let mut plan = Vec::with_capacity(zip.len());
    let mut seen = HashSet::new();
    let mut unsupported = false;
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|e| zip_error(archive, e))?;
        total = total.saturating_add(entry.size());
        if total > limits.max_expanded {
            return Err(too_large());
        }
        let name = entry
            .name()
            .map_err(|e| zip_error(archive, e))?
            .into_owned();
        let target = inside(destination, &name)?;
        if !seen.insert(target.to_string_lossy().to_lowercase()) {
            return Err(Error::domain(
                Message::new("Entrada duplicada: {p1}").arg("p1", &name),
            ));
        }
        // `stat.S_ISLNK(external_attr >> 16)`.
        if (entry.external_attributes() >> 16) & 0o170_000 == 0o120_000 {
            return Err(Error::domain(Message::new(
                "Los enlaces del archivo no están permitidos",
            )));
        }
        let is_dir = entry.is_dir();
        if !is_dir {
            if entry.encrypted() {
                return Err(Error::domain(Message::new(
                    "No se admiten archivos cifrados",
                )));
            }
            unsupported |= !method_is_supported(entry.compression());
        }
        plan.push(Planned {
            index,
            name,
            is_dir,
        });
    }
    if unsupported {
        return Err(Error::UnsupportedZipMethod);
    }
    Ok(plan)
}

fn write(
    zip: &mut ZipArchive<BufReader<File>>,
    archive: &Path,
    destination: &Path,
    plan: &[Planned],
    sink: &mut Sink<'_>,
) -> Result<()> {
    for planned in plan {
        let target: PathBuf = inside(destination, &planned.name)?;
        if planned.is_dir {
            Sink::dir(&target)?;
        } else {
            let mut entry = zip
                .by_index(planned.index)
                .map_err(|e| zip_error(archive, e))?;
            sink.file(&target, &mut entry)?;
        }
    }
    Ok(())
}
