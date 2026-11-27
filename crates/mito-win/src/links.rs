//! Junctions NTFS: crear, detectar, leer y quitar.
//!
//! Un junction enlaza un directorio con otro del mismo equipo sin necesitar
//! privilegios de administrador. Es lo que usa la persistencia de MITO para los
//! directorios (los archivos usan hardlinks, que ofrece la biblioteca estándar).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Crea un junction en `link` que apunta al directorio existente `target`.
///
/// `link` no debe existir; su carpeta padre sí.
///
/// # Errors
///
/// El error de E/S del sistema si `link` ya existe, `target` no es un directorio o
/// el volumen no admite junctions.
pub fn create_junction(target: &Path, link: &Path) -> io::Result<()> {
    junction::create(target, link)
}

/// Indica si `path` es un junction (un punto de reanálisis de tipo `MOUNT_POINT`).
///
/// Devuelve `false` si la ruta no existe. Los enlaces simbólicos no cuentan. Un junction
/// cuyo destino ya no existe (p. ej. tras mover la raíz de la instancia) sigue siéndolo.
pub fn is_junction(path: &Path) -> bool {
    junction::exists(path).unwrap_or(false) || junction::get_target(path).is_ok()
}

/// Indica si `path` es un enlace simbólico (de archivo o de directorio).
///
/// Un junction no cuenta, aunque la biblioteca estándar lo clasifique como enlace.
pub fn is_symlink(path: &Path) -> bool {
    !is_junction(path) && fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Destino de un junction, sin el prefijo `\?\`.
///
/// # Errors
///
/// El error de E/S del sistema si `link` no existe o no es un junction.
pub fn junction_target(link: &Path) -> io::Result<PathBuf> {
    let target = junction::get_target(link)?;
    Ok(dunce::simplified(&target).to_path_buf())
}

/// Quita un junction sin tocar el contenido de su destino.
///
/// # Errors
///
/// El error de E/S del sistema; falla con `InvalidInput` si `link` no es un junction,
/// para no borrar nunca un directorio real por error.
pub fn remove_junction(link: &Path) -> io::Result<()> {
    if !is_junction(link) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta no es un junction",
        ));
    }
    // `RemoveDirectory` sobre un junction elimina solo el punto de reanálisis.
    fs::remove_dir(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crea_detecta_lee_y_quita_un_junction_sin_borrar_el_destino() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("destino");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("dato.txt"), "x").unwrap();
        let link = dir.path().join("enlace");
        create_junction(&target, &link).unwrap();
        assert!(is_junction(&link));
        assert!(!is_junction(&target));
        assert!(!is_symlink(&link));
        assert_eq!(
            junction_target(&link).unwrap().canonicalize().unwrap(),
            dunce::canonicalize(&target)
                .unwrap()
                .canonicalize()
                .unwrap()
        );
        assert!(link.join("dato.txt").exists());
        remove_junction(&link).unwrap();
        assert!(!link.exists());
        assert!(target.join("dato.txt").exists());
    }

    #[test]
    fn un_junction_con_destino_inexistente_sigue_siendo_un_junction() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("destino");
        fs::create_dir(&target).unwrap();
        let link = dir.path().join("enlace");
        create_junction(&target, &link).unwrap();
        fs::remove_dir(&target).unwrap();
        assert!(is_junction(&link));
        assert!(!is_symlink(&link));
        remove_junction(&link).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
    }

    #[test]
    fn no_quita_un_directorio_real() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        assert!(remove_junction(&real).is_err());
        assert!(real.exists());
        assert!(!is_junction(&dir.path().join("inexistente")));
    }
}
