//! Extracción de TAR, `.tar.gz`, `.tar.xz` y `.tar.bz2` (bloque TAR de `archives.py`).

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use mito_core::Message;
use mito_core::storage::inside;
use tar::{Archive, EntryType};

use super::Limits;
use super::sink::Sink;
use crate::error::{Error, Result};

const TOO_LARGE: &str = "TAR demasiado grande";
const UNSAFE: &str = "TAR con enlaces, dispositivos o entradas duplicadas";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    /// Enlace simbólico o duro, dispositivo, FIFO…
    Other,
}

struct Member {
    name: String,
    kind: Kind,
    size: u64,
}

/// Compresión detectada por la firma del contenido, no por la extensión.
fn open(archive: &Path) -> std::io::Result<Archive<Box<dyn Read>>> {
    let mut reader = BufReader::new(File::open(archive)?);
    let mut magic = [0_u8; 6];
    let mut filled = 0;
    while filled < magic.len() {
        let read = std::io::Read::read(&mut reader, &mut magic[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    let magic = &magic[..filled];
    // Se reabre en lugar de rebobinar: el lector con búfer no necesita `Seek`.
    let reader = BufReader::new(File::open(archive)?);
    let source: Box<dyn Read> = if magic.starts_with(&[0x1F, 0x8B]) {
        Box::new(flate2::read::MultiGzDecoder::new(reader))
    } else if magic.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Box::new(liblzma::read::XzDecoder::new_multi_decoder(reader))
    } else if magic.starts_with(b"BZh") {
        Box::new(bzip2::read::MultiBzDecoder::new(reader))
    } else {
        Box::new(reader)
    };
    Ok(Archive::new(source))
}

/// ¿Es un TAR (posiblemente comprimido) el contenido de `archive`? (`tarfile.is_tarfile`)
///
/// Se considera TAR si la primera cabecera es válida (suma de verificación correcta) o
/// si el archivo, no vacío, empieza por un bloque de ceros.
pub fn is_tar(archive: &Path) -> bool {
    if std::fs::metadata(archive).is_ok_and(|m| m.len() == 0) {
        return false;
    }
    let Ok(mut tar) = open(archive) else {
        return false;
    };
    let Ok(mut entries) = tar.entries() else {
        return false;
    };
    !matches!(entries.next(), Some(Err(_)))
}

fn damaged(archive: &Path, error: &std::io::Error) -> Error {
    Error::domain(
        Message::new("Archivo TAR no válido: {p1}")
            .arg("p1", format!("{}: {error}", archive.display())),
    )
}

fn kind_of(kind: EntryType) -> Kind {
    match kind {
        EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => Kind::File,
        EntryType::Directory => Kind::Dir,
        _ => Kind::Other,
    }
}

/// Lista y valida todos los miembros antes de escribir nada.
fn plan(archive: &Path, destination: &Path, limits: &Limits) -> Result<Vec<Member>> {
    let too_large = || Error::domain(Message::new(TOO_LARGE));
    let mut tar = open(archive).map_err(|e| damaged(archive, &e))?;
    let mut members = Vec::new();
    let mut total = 0_u64;
    for entry in tar.entries().map_err(|e| damaged(archive, &e))? {
        let entry = entry.map_err(|e| damaged(archive, &e))?;
        if members.len() >= limits.max_entries {
            return Err(too_large());
        }
        total = total.saturating_add(entry.size());
        members.push(Member {
            name: String::from_utf8_lossy(&entry.path_bytes()).into_owned(),
            kind: kind_of(entry.header().entry_type()),
            size: entry.size(),
        });
    }
    if total > limits.max_expanded {
        return Err(too_large());
    }
    let mut seen = HashSet::new();
    for member in &members {
        let target = inside(destination, &member.name)?;
        if member.kind == Kind::Other || !seen.insert(target.to_string_lossy().to_lowercase()) {
            return Err(Error::domain(Message::new(UNSAFE)));
        }
    }
    Ok(members)
}

/// Extrae un TAR con los límites por defecto. Ver [`extract_tar_with`].
///
/// # Errors
///
/// Los de [`extract_tar_with`].
pub fn extract_tar(archive: &Path, destination: &Path) -> Result<()> {
    extract_tar_with(archive, destination, &Limits::default())
}

/// Extrae un TAR (gzip, xz, bzip2 o sin comprimir) validándolo antes de escribir:
/// más de `limits.max_entries` miembros o más de `limits.max_expanded` bytes declarados
/// (`TAR demasiado grande`); cualquier miembro que no sea archivo ni carpeta, o duplicado
/// sin distinguir mayúsculas (`TAR con enlaces, dispositivos o entradas duplicadas`); y
/// rutas que no pasen `inside`. Los bytes realmente escritos también se limitan.
///
/// # Errors
///
/// Los anteriores y errores de E/S; si falla la escritura se deshace lo escrito.
pub fn extract_tar_with(archive: &Path, destination: &Path, limits: &Limits) -> Result<()> {
    let members = plan(archive, destination, limits)?;
    let mut sink = Sink::new(archive, destination, limits.max_expanded, TOO_LARGE);
    let result = write(archive, destination, &members, &mut sink);
    if result.is_err() {
        sink.rollback();
    }
    result
}

fn write(
    archive: &Path,
    destination: &Path,
    members: &[Member],
    sink: &mut Sink<'_>,
) -> Result<()> {
    let mut tar = open(archive).map_err(|e| damaged(archive, &e))?;
    let mut expected = members.iter();
    for entry in tar.entries().map_err(|e| damaged(archive, &e))? {
        let mut entry = entry.map_err(|e| damaged(archive, &e))?;
        // El archivo no debe haber cambiado entre la validación y la escritura.
        let Some(member) = expected.next() else {
            return Err(Error::domain(Message::new(UNSAFE)));
        };
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if name != member.name
            || kind_of(entry.header().entry_type()) != member.kind
            || entry.size() != member.size
        {
            return Err(Error::domain(Message::new(UNSAFE)));
        }
        let target = inside(destination, &member.name)?;
        match member.kind {
            Kind::Dir => Sink::dir(&target)?,
            Kind::File => sink.file(&target, &mut entry)?,
            Kind::Other => return Err(Error::domain(Message::new(UNSAFE))),
        }
    }
    Ok(())
}
