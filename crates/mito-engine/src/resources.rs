//! Recursos de bucket para los hooks (`bucket_resources.prepare`, F7).
//!
//! Los hooks pueden leer la carpeta `scripts/` de su bucket (y de otros) con
//! `$bucketsdir`. Cada instalación trabaja con una copia privada en
//! `<staging>/hook-buckets/<bucket>/scripts`, compartida por `pre_install` e `installer`.
//! Contrato: `docs/spec/modules/bucket_resources.md` («Parte `prepare`»).

use std::fs;
use std::path::{Path, PathBuf};

use mito_catalog::github_repository;
use mito_core::storage::{identifier, inside};
use mito_core::{Message, Operation};
use mito_transfer::archives::{MAX_ENTRIES, MAX_EXPANDED, extract_zip};

use crate::Log;
use crate::error::{Error, Result, bail, io_error};
use crate::extensions::Downloader;
use crate::fsutil::{copy_file, create_dir_all, is_link};

/// Copia los `scripts/` de los buckets de `origins` (`(nombre, origen)`) en
/// `destination` (`<staging>/hook-buckets`).
///
/// Un origen remoto es un repositorio de GitHub y se baja como snapshot sin hash; la
/// aprobación del usuario se pidió al planificar (`Plan::unverified`). Un origen local es
/// la raíz del repositorio o su carpeta `bucket/`.
///
/// # Errors
///
/// `La fuente debe ser un repositorio GitHub o una carpeta local`, `El snapshot del
/// catálogo debe tener una sola raíz`, `No hay recursos scripts válidos en el catálogo:
/// {name}`, `Los enlaces del catálogo no están permitidos`, `Recursos del catálogo
/// demasiado grandes` y los de descarga, extracción y E/S.
pub fn prepare_resources(
    origins: &[(String, String)],
    destination: &Path,
    downloader: &dyn Downloader,
    operation: &Operation,
    log: Log<'_>,
) -> Result<()> {
    create_dir_all(destination)?;
    let parent = destination.parent().unwrap_or(destination);
    for (name, origin) in origins {
        log(crate::raw_line(format!(
            "{name}: preparando recursos del catálogo"
        )));
        let scratch = tempfile::tempdir_in(parent).map_err(io_error(parent))?;
        let source = if origin.starts_with("http://") || origin.starts_with("https://") {
            snapshot(origin, scratch.path(), downloader, operation)?
        } else {
            local_source(origin)
        };
        let scripts = source.join("scripts");
        if !scripts.is_dir() || is_link(&scripts) {
            return bail(
                Message::new("No hay recursos scripts válidos en el catálogo: {p1}")
                    .arg("p1", name),
            );
        }
        let folder = destination.join(identifier(name)?).join("scripts");
        for (path, target) in plan_copy(&scripts, &folder)? {
            if let Some(parent) = target.parent() {
                create_dir_all(parent)?;
            }
            copy_file(&path, &target).map_err(io_error(&target))?;
        }
    }
    Ok(())
}

/// Origen local: la carpeta indicada o, si es `bucket/` sin `scripts/`, su padre.
fn local_source(origin: &str) -> PathBuf {
    let source = PathBuf::from(origin);
    let is_bucket = source
        .file_name()
        .is_some_and(|name| name.to_string_lossy().to_lowercase() == "bucket");
    if is_bucket
        && !source.join("scripts").is_dir()
        && let Some(parent) = source.parent()
    {
        return parent.to_path_buf();
    }
    source
}

/// Descarga y extrae el snapshot de GitHub; devuelve su única carpeta raíz.
fn snapshot(
    origin: &str,
    scratch: &Path,
    downloader: &dyn Downloader,
    operation: &Operation,
) -> Result<PathBuf> {
    let Some((owner, repository)) = github_repository(origin) else {
        return bail(Message::new(
            "La fuente debe ser un repositorio GitHub o una carpeta local",
        ));
    };
    let archive = scratch.join("repository.zip");
    downloader.download(
        &format!("https://api.github.com/repos/{owner}/{repository}/zipball"),
        &archive,
        None,
        operation,
    )?;
    let extracted = scratch.join("repository");
    extract_zip(&archive, &extracted)?;
    let mut entries = fs::read_dir(&extracted)
        .map_err(io_error(&extracted))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(io_error(&extracted))?;
    match (entries.pop(), entries.is_empty()) {
        (Some(root), true) if root.path().is_dir() => Ok(root.path()),
        _ => bail(Message::new(
            "El snapshot del catálogo debe tener una sola raíz",
        )),
    }
}

/// Recorre `scripts` sin seguir enlaces y devuelve `(origen, destino)` de cada archivo.
fn plan_copy(scripts: &Path, folder: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let too_large = || Error::domain(Message::new("Recursos del catálogo demasiado grandes"));
    let mut files = Vec::new();
    let mut count = 0usize;
    let mut total = 0u64;
    let mut pending = vec![(scripts.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(io_error(&directory))? {
            let entry = entry.map_err(io_error(&directory))?;
            let path = entry.path();
            if is_link(&path) {
                return bail(Message::new("Los enlaces del catálogo no están permitidos"));
            }
            let relative = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let target = inside(folder, &relative)?;
            count += 1;
            if path.is_dir() {
                pending.push((path, format!("{relative}/")));
            } else {
                total += entry.metadata().map_err(io_error(&path))?.len();
                files.push((path, target));
            }
            if count > MAX_ENTRIES || total > MAX_EXPANDED {
                return Err(too_large());
            }
        }
    }
    files.sort();
    Ok(files)
}
