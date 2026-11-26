//! Utilidades de sistema de archivos del motor.

use std::fs;
use std::io;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use mito_core::Message;
use mito_win::links::{is_junction, is_symlink};

use crate::error::{Result, bail, io_error};

/// `DELETE` de Win32: derecho de borrado sobre un archivo.
const DELETE: u32 = 0x0001_0000;
/// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
const SHARE_ALL: u32 = 7;

/// ¿Es un enlace simbólico o un junction? (`is_symlink() or is_junction()`).
pub(crate) fn is_link(path: &Path) -> bool {
    is_junction(path) || is_symlink(path)
}

/// Comprueba que ningún archivo bajo `folder` esté en uso (`check_unused`).
///
/// Abre cada archivo con derecho de borrado y compartiendo todo, como el original con
/// `CreateFileW(path, DELETE, 7, …, OPEN_EXISTING)`. No entra en junctions ni en
/// enlaces simbólicos.
///
/// # Errors
///
/// `Cierra la aplicación antes de eliminar|actualizar; archivo en uso o sin permiso de
/// eliminación: {p1}` con el primer archivo bloqueado.
pub(crate) fn check_unused(folder: &Path, removing: bool) -> Result<()> {
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        // Como `os.walk`, un directorio que no se puede listar se ignora.
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if is_link(&path) {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            let opened = fs::OpenOptions::new()
                .access_mode(DELETE)
                .share_mode(SHARE_ALL)
                .open(&path);
            if opened.is_err() {
                let template = if removing {
                    "Cierra la aplicación antes de eliminar; archivo en uso o sin permiso de eliminación: {p1}"
                } else {
                    "Cierra la aplicación antes de actualizar; archivo en uso o sin permiso de eliminación: {p1}"
                };
                return bail(Message::new(template).arg("p1", path.display()));
            }
        }
    }
    Ok(())
}

/// Borra un árbol sin recorrer junctions ni enlaces (`shutil.rmtree`). Si `path` es un
/// enlace, solo se quita el enlace.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    if mito_win::links::is_junction(path) {
        return mito_win::links::remove_junction(path);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Renombra un directorio o archivo (`Path.rename`).
pub(crate) fn rename(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(io_error(to))
}

/// Crea `path` y sus padres.
pub(crate) fn create_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(io_error(path))
}

/// Copia un archivo conservando la fecha de modificación (`shutil.copy2`).
pub(crate) fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to)?;
    if let Ok(modified) = fs::metadata(from).and_then(|m| m.modified())
        && let Ok(file) = fs::File::options().write(true).open(to)
    {
        // La fecha es un detalle: no debe hacer fallar la copia.
        let _ = file.set_modified(modified);
    }
    Ok(())
}

/// Copia un árbol completo (`shutil.copytree`) conservando fechas.
pub(crate) fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Ruta de `path` relativa a `base`, con `/` como separador (`as_posix`).
pub(crate) fn relative_posix(path: &Path, base: &Path) -> Option<String> {
    let relative = path.strip_prefix(base).ok()?;
    Some(
        relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Nombre de un staging único dentro de `core/staging`.
pub(crate) fn staging_name(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}
