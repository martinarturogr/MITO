//! Preparación de una descarga: extraer contenedores o copiar el archivo suelto
//! (`archives.py` y `extract_zip` de `transfer.py`). Nunca ejecuta lo descargado.
//!
//! Contrato: `docs/spec/modules/archives.md`.
//!
//! - ZIP y `.nupkg`, TAR (`.tar.gz`, `.tar.xz`, `.tar.bz2`): nativos y validados por
//!   completo antes de escribir.
//! - 7z y RAR: delegados en el `7z.exe` privado, cuya ruta recibe el llamador.
//! - MSI, Burn e Inno Setup pertenecen a la fase F7: aquí solo hay puntos de enganche
//!   ([`ExtractOptions::msi`], [`ExtractOptions::inno`]).

mod sevenzip;
mod sink;
mod tarball;
mod zipfile;

use std::fs;
use std::io::Read;
use std::path::Path;

use mito_core::Message;
use mito_core::storage::inside;

pub use sevenzip::{extract_7zip, extract_7zip_with};
pub use tarball::{extract_tar, extract_tar_with, is_tar};
pub use zipfile::{extract_zip, extract_zip_with, is_zip};

use crate::error::{Error, Result};

/// Máximo de bytes expandidos de un contenedor: 12 GiB (`MAX_EXPANDED`).
pub const MAX_EXPANDED: u64 = 12 * 1024 * 1024 * 1024;
/// Máximo de entradas de un contenedor (ZIP, TAR y 7z).
pub const MAX_ENTRIES: usize = 150_000;

/// Límites de una extracción. Se pueden bajar en tests; los valores por defecto son los
/// del original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Entradas máximas.
    pub max_entries: usize,
    /// Bytes máximos expandidos, tanto declarados como realmente escritos.
    pub max_expanded: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
            max_expanded: MAX_EXPANDED,
        }
    }
}

/// Resultado de [`extract`] (el `bool` del original).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extraction {
    /// Se extrajo un contenedor (`True`).
    Extracted,
    /// Se copió un archivo suelto a `destination/<filename>` (`False`).
    Loose,
}

/// Extractor delegado (MSI o Inno Setup, F7): recibe el archivo y el destino.
pub type Delegate<'a> = &'a dyn Fn(&Path, &Path) -> Result<()>;

/// Opciones de [`extract`].
#[derive(Default)]
pub struct ExtractOptions<'a> {
    /// Ruta de `7z.exe` (`core/tools/7zip/7z.exe`). Sin ella, 7z y RAR fallan con
    /// `Falta el extractor privado 7-Zip; ejecuta bootstrap`.
    pub sevenzip: Option<&'a Path>,
    /// El manifiesto declara `innosetup`: un `.exe` se extrae con `innounp` en lugar de
    /// copiarse.
    pub innosetup: bool,
    /// Extractor de Inno Setup (F7). Sin él, un `.exe` con `innosetup` es un formato no
    /// soportado.
    pub inno: Option<Delegate<'a>>,
    /// Extractor de MSI (F7). Sin él, un `.msi` es un formato no soportado.
    pub msi: Option<Delegate<'a>>,
    /// Recibe los mensajes de progreso (`Extrayendo {p1}`).
    pub log: Option<&'a dyn Fn(Message)>,
    /// Límites de extracción; `None` usa los del original.
    pub limits: Option<Limits>,
}

/// Sufijo en minúsculas del nombre de archivo, como `Path(filename).suffix.lower()`.
fn suffix(filename: &str) -> String {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or_default();
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => name[dot..].to_lowercase(),
        _ => String::new(),
    }
}

fn unsupported(filename: &str) -> Error {
    Error::domain(Message::new("Formato de archivo todavía no soportado: {p1}").arg("p1", filename))
}

fn copy_loose(archive: &Path, destination: &Path, filename: &str) -> Result<Extraction> {
    fs::create_dir_all(destination).map_err(|e| mito_core::Error::io(destination, e))?;
    let target = inside(destination, filename)?;
    fs::copy(archive, &target).map_err(|e| mito_core::Error::io(&target, e))?;
    // `shutil.copy2` conserva la fecha de modificación.
    if let Ok(modified) = fs::metadata(archive).and_then(|m| m.modified())
        && let Ok(file) = fs::File::options().write(true).open(&target)
    {
        let _ = file.set_modified(modified);
    }
    Ok(Extraction::Loose)
}

/// Prepara una descarga: extrae el contenedor en `destination` o copia el archivo suelto.
///
/// `filename` es el nombre local de la descarga ([`crate::download_filename`]); su sufijo
/// y el contenido de `archive` deciden qué hacer, en este orden (el primero que aplica
/// gana):
///
/// 1. `innosetup` y `.exe`: extractor de Inno ([`ExtractOptions::inno`]).
/// 2. `.jar` o `.ico`: copia íntegra, aunque el contenido sea un ZIP. Un alias
///    `#/archivo.zip` cambia el sufijo y sí extrae.
/// 3. `.msi`: extractor de MSI ([`ExtractOptions::msi`]).
/// 4. Contenido ZIP: [`extract_zip`]; si usa un método no soportado, 7-Zip.
/// 5. Contenido TAR (con o sin gzip/xz/bzip2): [`extract_tar`].
/// 6. `.7z` o `.rar`, o sin sufijo y con firma de 7z o RAR: [`extract_7zip`].
/// 7. Sin sufijo, `.exe`, `.dll`, `.jar`, `.ps1`, `.cmd`, `.bat` o `.com`: copia suelta.
/// 8. Cualquier otro: `Formato de archivo todavía no soportado: {p1}`.
///
/// # Errors
///
/// Los de cada extractor y, en el último caso, el error de formato no soportado.
pub fn extract(
    archive: &Path,
    destination: &Path,
    filename: &str,
    options: &ExtractOptions<'_>,
) -> Result<Extraction> {
    if let Some(log) = options.log {
        log(Message::new("Extrayendo {p1}").arg("p1", filename));
    }
    let limits = options.limits.unwrap_or_default();
    let suffix = suffix(filename);
    if options.innosetup && suffix == ".exe" {
        let inno = options.inno.ok_or_else(|| unsupported(filename))?;
        inno(archive, destination)?;
        return Ok(Extraction::Extracted);
    }
    // Los JAR son ZIP, pero deben seguir siendo artefactos Java ejecutables.
    if suffix == ".jar" || suffix == ".ico" {
        return copy_loose(archive, destination, filename);
    }
    if suffix == ".msi" {
        let msi = options.msi.ok_or_else(|| unsupported(filename))?;
        msi(archive, destination)?;
        return Ok(Extraction::Extracted);
    }
    if is_zip(archive) {
        match extract_zip_with(archive, destination, &limits) {
            // Rutas, tamaños y enlaces ya están validados: solo falla el método.
            Err(Error::UnsupportedZipMethod) => {
                extract_7zip_with(archive, destination, options.sevenzip, &limits)?;
            }
            other => other?,
        }
        return Ok(Extraction::Extracted);
    }
    if is_tar(archive) {
        extract_tar_with(archive, destination, &limits)?;
        return Ok(Extraction::Extracted);
    }
    if suffix == ".7z" || suffix == ".rar" || (suffix.is_empty() && has_archive_signature(archive))
    {
        extract_7zip_with(archive, destination, options.sevenzip, &limits)?;
        return Ok(Extraction::Extracted);
    }
    if matches!(
        suffix.as_str(),
        "" | ".exe" | ".dll" | ".jar" | ".ps1" | ".cmd" | ".bat" | ".com"
    ) {
        return copy_loose(archive, destination, filename);
    }
    Err(unsupported(filename))
}

/// Firma de 7z (`7z\xBC\xAF\x27\x1C`) o RAR (`Rar!\x1A\x07`).
fn has_archive_signature(archive: &Path) -> bool {
    let mut signature = [0_u8; 8];
    let read = fs::File::open(archive)
        .and_then(|mut file| file.read(&mut signature))
        .unwrap_or(0);
    let signature = &signature[..read];
    signature.starts_with(b"7z\xBC\xAF\x27\x1C") || signature.starts_with(b"Rar!\x1A\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sufijo_como_pathlib() {
        assert_eq!(suffix("Tool.JAR"), ".jar");
        assert_eq!(suffix("a.tar.gz"), ".gz");
        assert_eq!(suffix("download"), "");
        assert_eq!(suffix(".bashrc"), "");
        assert_eq!(suffix("file."), "");
        assert_eq!(suffix("dir\\x.7Z"), ".7z");
    }
}
