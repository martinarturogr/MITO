//! Aislamiento por profile (`isolation.py`).
//!
//! Un componente está aislado si y solo si su manifiesto efectivo declara `persist` y
//! `shortcuts`. Entonces cada profile tiene una copia independiente en
//! `core/profile-apps/<profile>/<bucket>/<app>` y datos en
//! `home/profiles/<profile>/persist/<key>`; si no, todos comparten `core/apps` y
//! `home/persist`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use mito_catalog::pyjson::py_truthy;
use mito_core::persistence::{self, DEFAULTS, JOURNAL};
use mito_core::storage::{identifier, inside, portable_path};
use mito_core::{Layout, Message};
use serde_json::{Map, Value};

use crate::engine::Engine;
use crate::error::{Error, Result, bail, io_error};
use crate::fsutil::{copy_dir, copy_file, create_dir_all, is_link, relative_posix, rename};
use crate::record::{Record, effective};

/// ¿Está aislado un manifiesto? (`persist` y `shortcuts` verdaderos).
///
/// # Errors
///
/// Los de [`effective`].
pub fn isolated_manifest(manifest: &Value) -> Result<bool> {
    let manifest = effective(manifest)?;
    Ok(isolated_effective(&manifest))
}

/// Como [`isolated_manifest`] sobre un manifiesto ya efectivo.
pub fn isolated_effective(manifest: &Map<String, Value>) -> bool {
    manifest.get("persist").is_some_and(py_truthy)
        && manifest.get("shortcuts").is_some_and(py_truthy)
}

/// ¿Está aislado el componente?
///
/// # Errors
///
/// Los de [`effective`].
pub fn isolated(record: &Record) -> Result<bool> {
    isolated_manifest(&record.manifest)
}

fn profile_required(profile: Option<&str>) -> Result<String> {
    match profile {
        Some(profile) => Ok(identifier(profile)?),
        None => bail(Message::new("Nombre no válido: ").with_suffix("None")),
    }
}

/// Carpeta de datos persistentes: `home/profiles/<profile>/persist/<key>` si está
/// aislado, `home/persist/<key>` si no. `profile` solo se usa y valida si está aislado.
///
/// # Errors
///
/// Nombres de profile no válidos y rutas fuera de la raíz.
pub fn persist_root(layout: &Layout, record: &Record, profile: Option<&str>) -> Result<PathBuf> {
    if isolated(record)? {
        let profile = profile_required(profile)?;
        Ok(persistence::persist_root(
            layout,
            &record.key,
            Some(&profile),
        )?)
    } else {
        Ok(persistence::persist_root(layout, &record.key, None)?)
    }
}

/// Carpeta del payload que usa `profile`: la compartida de `core/apps` o su copia.
///
/// # Errors
///
/// Rutas fuera de la raíz o `Ruta de instalación no válida` si no está en `core/apps`.
pub fn payload_path(layout: &Layout, record: &Record, profile: Option<&str>) -> Result<PathBuf> {
    let base = inside(layout.root(), &record.path)?;
    if !isolated(record)? {
        return Ok(base);
    }
    let profile = profile_required(profile)?;
    let Some(relative) = relative_posix(&base, &layout.apps()) else {
        return bail(Message::new("Ruta de instalación no válida para eliminar"));
    };
    Ok(inside(&layout.profile_apps().join(profile), &relative)?)
}

fn lower(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Comprueba que no haya enlaces fuera de las rutas excluidas.
fn check_no_links(base: &Path, excluded: &HashSet<String>) -> Result<()> {
    let mut pending = vec![base.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let entries = fs::read_dir(&folder).map_err(io_error(&folder))?;
        for entry in entries {
            let path = entry.map_err(io_error(&folder))?.path();
            if excluded.contains(&lower(&path)) {
                continue;
            }
            if is_link(&path) {
                return Err(Error::domain(
                    Message::new("Enlace no admitido en la copia del componente: {p1}")
                        .arg("p1", path.display()),
                ));
            }
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn copy_filtered(base: &Path, folder: &Path, to: &Path, excluded: &HashSet<String>) -> Result<()> {
    create_dir_all(to)?;
    for entry in fs::read_dir(folder).map_err(io_error(folder))? {
        let entry = entry.map_err(io_error(folder))?;
        let path = entry.path();
        if excluded.contains(&lower(&path)) || (folder == base && entry.file_name() == JOURNAL) {
            continue;
        }
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_filtered(base, &path, &target, excluded)?;
        } else {
            copy_file(&path, &target).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

/// Prepara el payload de `profile` y devuelve su ruta (`prepare_payload`).
///
/// El llamador tiene el lock de mutación. Si está aislado y la copia no existe, la
/// construye en `core/staging` y la publica con un `rename`; después enlaza siempre la
/// persistencia (también la compartida).
///
/// # Errors
///
/// `Enlace no admitido en la copia del componente: {p1}`, los de persistencia y E/S.
pub fn prepare_payload(layout: &Layout, record: &Record, profile: Option<&str>) -> Result<PathBuf> {
    let base = inside(layout.root(), &record.path)?;
    let target = payload_path(layout, record, profile)?;
    let manifest = record.effective()?;
    let entries = persistence::definitions(manifest.get("persist"))?;
    if isolated_effective(&manifest) && !target.exists() {
        let excluded: HashSet<String> = entries
            .iter()
            .map(|e| Ok(lower(&portable_path(&e.relative)?.join_to(&base))))
            .collect::<Result<_>>()?;
        check_no_links(&base, &excluded)?;
        create_dir_all(&layout.staging())?;
        let tmp = tempfile::tempdir_in(layout.staging()).map_err(io_error(&layout.staging()))?;
        let copy = tmp.path().join("payload");
        copy_filtered(&base, &base, &copy, &excluded)?;
        for entry in &entries {
            let source = portable_path(&entry.relative)?.join_to(&base);
            let defaults = inside(&copy.join(DEFAULTS), &entry.relative)?;
            if !defaults.exists() && source.exists() && !is_link(&source) {
                if let Some(parent) = defaults.parent() {
                    create_dir_all(parent)?;
                }
                if source.is_dir() {
                    copy_dir(&source, &defaults).map_err(io_error(&source))?;
                } else {
                    copy_file(&source, &defaults).map_err(io_error(&source))?;
                }
            }
        }
        if let Some(parent) = target.parent() {
            create_dir_all(parent)?;
        }
        rename(&copy, &target)?;
    }
    let destination = persist_root(layout, record, profile)?;
    persistence::prepare(layout, &target, &record.key, &entries, Some(&destination))?;
    Ok(target)
}

impl Engine {
    /// [`prepare_payload`] con el punto de fallo de pruebas `prepare-payload`.
    ///
    /// # Errors
    ///
    /// Ver [`prepare_payload`].
    pub fn prepare_payload(&self, record: &Record, profile: &str) -> Result<PathBuf> {
        self.fault("prepare-payload", profile)?;
        prepare_payload(self.layout(), record, Some(profile))
    }
}
