//! Identidad de archivos NTFS.

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

/// Identidad de un archivo en disco: volumen e índice de archivo.
fn identity(path: &Path) -> io::Result<(u32, u64)> {
    let file = File::open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: el handle pertenece a `file`, que sigue abierto durante la llamada, e
    // `info` es una estructura válida que la función solo escribe.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut info) }
        .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xffff))?;
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, index))
}

/// ¿Son `a` y `b` el mismo archivo (el mismo volumen e índice)? Es el caso de dos
/// enlaces duros del mismo contenido (`os.path.samefile`).
///
/// # Errors
///
/// El error de E/S del sistema si alguno no se puede abrir.
pub fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(identity(a)? == identity(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn un_enlace_duro_es_el_mismo_archivo_y_una_copia_no() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("a.txt");
        fs::write(&original, "x").unwrap();
        let hard = dir.path().join("b.txt");
        fs::hard_link(&original, &hard).unwrap();
        let copy = dir.path().join("c.txt");
        fs::copy(&original, &copy).unwrap();
        assert!(same_file(&original, &hard).unwrap());
        assert!(same_file(&original, &original).unwrap());
        assert!(!same_file(&original, &copy).unwrap());
        assert!(same_file(&original, &dir.path().join("falta")).is_err());
    }
}
