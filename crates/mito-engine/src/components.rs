//! Ciclo de vida de los componentes instalados (`components.py`): actualizar, retener
//! una versión, liberarla y eliminar conservando los datos persistentes.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use mito_catalog::pyjson::py_str;
use mito_catalog::{Manifest, compatibility};
use mito_core::persistence::definitions;
use mito_core::storage::{inside, read_json, write_json};
use mito_core::{Message, Operation};
use serde_json::Value;

use crate::Log;
use crate::engine::Engine;
use crate::error::{Error, Result, bail};
use crate::fsutil::{
    check_unused, create_dir_all, is_link, relative_posix, remove_tree, staging_name,
};
use crate::install::InstallOptions;
use crate::isolation::{isolated, isolated_manifest};
use crate::profiles::Profile;
use crate::record::{MARKER, Record, effective};

/// Parte de una versión tras los números: un texto ordena antes que un número.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionPart {
    /// Texto en minúsculas.
    Text(String),
    /// Número.
    Number(u128),
}

/// Clave de ordenación de una versión (`version_key`): números, estable antes que
/// prerelease y partes del sufijo.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionKey {
    numbers: Vec<u128>,
    stable: bool,
    parts: Vec<VersionPart>,
}

/// Versión posterior disponible en el catálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Versión del catálogo.
    pub version: String,
    /// `Compatible` o el motivo de rechazo.
    pub status: String,
}

const PRERELEASE: [&str; 8] = [
    "alpha", "beta", "preview", "pre", "rc", "ea", "dev", "snapshot",
];

/// `version_key` del original: `[vV]?\d+([._-]\d+)*` seguido de un sufijo libre.
///
/// Devuelve `None` si no encaja o si un número no cabe en 128 bits (el original usa
/// enteros sin límite; aquí esa versión no es candidata a actualización).
pub fn version_key(value: &str) -> Option<VersionKey> {
    if value.contains('\n') {
        return None;
    }
    let bytes = value.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'v' | b'V')));
    let mut numbers = Vec::new();
    loop {
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if start == index {
            return None;
        }
        numbers.push(value[start..index].parse::<u128>().ok()?);
        if matches!(bytes.get(index), Some(b'.' | b'_' | b'-'))
            && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
        {
            index += 1;
        } else {
            break;
        }
    }
    while numbers.len() > 1 && numbers.last() == Some(&0) {
        numbers.pop();
    }
    let suffix = value[index..]
        .to_lowercase()
        .trim_start_matches(['-', '.', '_', '+'])
        .to_owned();
    let prerelease = PRERELEASE.iter().any(|p| suffix.starts_with(p));
    let mut parts = Vec::new();
    let mut chars = suffix.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        let digits = c.is_ascii_digit();
        if !digits && !c.is_ascii_lowercase() {
            chars.next();
            continue;
        }
        let mut end = start;
        while let Some(&(i, d)) = chars.peek() {
            if (digits && d.is_ascii_digit()) || (!digits && d.is_ascii_lowercase()) {
                end = i + d.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let text = &suffix[start..end];
        parts.push(if digits {
            VersionPart::Number(text.parse().ok()?)
        } else {
            VersionPart::Text(text.to_owned())
        });
    }
    Some(VersionKey {
        numbers,
        stable: !prerelease,
        parts,
    })
}

/// Rechaza una actualización que cambia el aislamiento o las definiciones de `persist`.
///
/// # Errors
///
/// `La actualización cambia persist o el aislamiento por profile; requiere una migración
/// de datos explícita.`
pub fn validate_update_layout(old: &Record, manifest: &Value) -> Result<()> {
    let old_entries = definitions(effective(&old.manifest)?.get("persist"))?;
    let new_entries = definitions(effective(manifest)?.get("persist"))?;
    let same_set = old_entries.iter().all(|e| new_entries.contains(e))
        && new_entries.iter().all(|e| old_entries.contains(e));
    if isolated(old)? != isolated_manifest(manifest)? || !same_set {
        return bail(Message::new(
            "La actualización cambia persist o el aislamiento por profile; requiere una migración de datos explícita.",
        ));
    }
    Ok(())
}

type CatalogMap = BTreeMap<String, Manifest>;

impl Engine {
    fn catalog_map(&self) -> Result<CatalogMap> {
        Ok(self.catalog()?.all()?.into_iter().collect())
    }

    /// Versión posterior del catálogo, si la hay. Una versión retenida no se actualiza.
    ///
    /// # Errors
    ///
    /// Los de lectura del catálogo.
    pub fn available_update(
        &self,
        lock: &Record,
        catalog: Option<&BTreeMap<String, Manifest>>,
    ) -> Result<Option<Candidate>> {
        if lock.pinned.is_some() {
            return Ok(None);
        }
        let owned;
        let catalog = if let Some(catalog) = catalog {
            catalog
        } else {
            owned = self.catalog_map()?;
            &owned
        };
        let Some(manifest) = catalog.get(&lock.key) else {
            return Ok(None);
        };
        let new_version = manifest
            .raw()
            .get("version")
            .map(py_str)
            .unwrap_or_default();
        let (Some(old), Some(new)) = (version_key(&lock.version_text()), version_key(&new_version))
        else {
            return Ok(None);
        };
        if new <= old {
            return Ok(None);
        }
        Ok(Some(Candidate {
            version: new_version,
            status: compatibility(&lock.key, manifest).text(),
        }))
    }

    /// Regenera los shims de los profiles indicados y refresca la terminal.
    ///
    /// # Errors
    ///
    /// Los de [`Engine::write_shims`] y de la terminal.
    pub fn refresh_launchers<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Result<()> {
        for name in names {
            self.fault("refresh-launchers", name)?;
            let data = self.profile(name)?;
            let folder = self.shims_folder(name)?;
            self.write_shims(&data, name, &folder)?;
        }
        self.extensions().terminal.prepare(self)
    }

    /// Elimina un componente conservando sus datos persistentes y quitándolo de todos
    /// los profiles que lo tenían.
    ///
    /// # Errors
    ///
    /// `El componente ya no está instalado`, `Ruta de instalación no válida para
    /// eliminar`, `Cierra la aplicación antes de eliminar; …` y errores de E/S. Si falla
    /// antes de borrar, todo vuelve a su sitio.
    pub fn remove(&self, installation: &str, log: Log<'_>) -> Result<()> {
        let _lock = self.mutation()?;
        let layout = self.layout();
        let lock = self
            .installed()?
            .get(installation)
            .cloned()
            .ok_or_else(|| Error::domain(Message::new("El componente ya no está instalado")))?;
        let payload = inside(layout.root(), &lock.path)?;
        let apps = dunce::canonicalize(layout.apps()).unwrap_or_else(|_| layout.apps());
        let resolved = dunce::canonicalize(&payload).unwrap_or_else(|_| payload.clone());
        let depth = relative_posix(&resolved, &apps).map(|r| r.split('/').count());
        if depth != Some(2) || is_link(&payload) {
            return bail(Message::new("Ruta de instalación no válida para eliminar"));
        }
        let backup: BTreeMap<String, Profile> = self
            .users(installation)
            .into_iter()
            .map(|name| self.profile(&name).map(|p| (name, p)))
            .collect::<Result<_>>()?;
        let stage = inside(
            layout.root(),
            &format!("core/staging/{}", staging_name("remove")),
        )?;
        // Todas las copias preparadas, también las de profiles inactivos o borrados.
        let mut targets: Vec<PathBuf> = vec![payload.clone()];
        if isolated(&lock)? {
            let relative = relative_posix(&resolved, &apps).unwrap_or_default();
            let base = dunce::canonicalize(layout.profile_apps())
                .unwrap_or_else(|_| layout.profile_apps());
            if let Ok(folders) = fs::read_dir(layout.profile_apps()) {
                let mut folders: Vec<PathBuf> = folders.flatten().map(|f| f.path()).collect();
                folders.sort();
                for folder in folders {
                    let copy = inside(&folder, &relative)?;
                    if copy.exists() {
                        let inside_base =
                            dunce::canonicalize(&copy).is_ok_and(|c| c.starts_with(&base));
                        if !inside_base || is_link(&copy) {
                            return bail(Message::new("Copia de profile fuera de su directorio"));
                        }
                        targets.push(copy);
                    }
                }
            }
        }
        // Como al actualizar: una app abierta bloquea sus archivos.
        for target in &targets {
            check_unused(target, true)?;
        }
        create_dir_all(&stage)?;
        let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
        for (index, target) in targets.iter().enumerate() {
            let stored = stage.join(index.to_string());
            if let Err(error) = fs::rename(target, &stored) {
                for (from, to) in moved.iter().rev() {
                    let _ = fs::rename(to, from);
                }
                let _ = fs::remove_dir(&stage);
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    return bail(
                        Message::new("Cierra la aplicación antes de eliminar; archivo en uso o sin permiso de eliminación: {p1}")
                            .arg("p1", payload.display()),
                    );
                }
                return Err(Error::io(target, error));
            }
            moved.push((target.clone(), stored));
        }
        let mut changed: Vec<&String> = Vec::new();
        for (name, data) in &backup {
            let mut updated = data.clone();
            updated.apps.retain(|app| app != installation);
            if let Err(error) = self.save_profile(name, &updated) {
                for (from, to) in moved.iter().rev() {
                    let _ = fs::rename(to, from);
                }
                let _ = fs::remove_dir(&stage);
                for done in changed {
                    let _ = crate::profiles::save(layout, done, &backup[done]);
                }
                return Err(error);
            }
            changed.push(name);
        }
        if remove_tree(&stage).is_err() {
            log(&Message::new("Componente retirado; quedan archivos en uso en {p1}. Cierra la aplicación para poder limpiarlos.")
                .arg("p1", stage.display())
                .to_string());
        }
        self.refresh_launchers(backup.keys().map(String::as_str))?;
        if let Err(error) = self
            .extensions()
            .host
            .unregister_installation(self, installation, log)
        {
            log(
                &Message::new("No se pudieron retirar los enlaces del host: {p1}")
                    .arg("p1", error)
                    .to_string(),
            );
        }
        log(
            &Message::new("Eliminado {p1} {p3}. Datos persistentes conservados.")
                .arg("p1", &lock.key)
                .arg("p3", lock.version_text())
                .to_string(),
        );
        Ok(())
    }

    /// Instala una versión concreta de `key` y la retiene hasta liberarla.
    ///
    /// # Errors
    ///
    /// Los de [`Engine::install`] y de cambio de diseño de datos.
    pub fn install_version(
        &self,
        key: &str,
        manifest: Value,
        pin: Value,
        confirm_unverified: Option<crate::Confirm<'_>>,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<String> {
        if let Some(current) = self.installed()?.get(key) {
            validate_update_layout(current, &manifest)?;
        }
        let options = InstallOptions {
            manifests: BTreeMap::from([(key.to_owned(), manifest)]),
            pins: BTreeMap::from([(key.to_owned(), pin)]),
            confirm_unverified,
            ..InstallOptions::default()
        };
        self.install(&[key], &options, log, operation)?;
        let version = self
            .installed()?
            .get(key)
            .map(Record::version_text)
            .unwrap_or_default();
        log(&Message::new(
            "{p1} {p3} instalado y retenido: Actualizar no lo sustituirá hasta liberarlo en Info.",
        )
        .arg("p1", key)
        .arg("p3", version)
        .to_string());
        Ok(key.to_owned())
    }

    /// Quita la retención sin tocar el payload. `false` si no estaba retenido.
    ///
    /// # Errors
    ///
    /// `El componente ya no está instalado` y errores de escritura.
    pub fn release_version(&self, installation: &str, log: Log<'_>) -> Result<bool> {
        let lock = {
            let _lock = self.mutation()?;
            let lock = self
                .installed()?
                .get(installation)
                .cloned()
                .ok_or_else(|| Error::domain(Message::new("El componente ya no está instalado")))?;
            let marker = inside(self.root(), &lock.path)?.join(MARKER);
            let mut metadata: Value = read_json(&marker)?;
            let removed = metadata
                .get_mut("record")
                .and_then(Value::as_object_mut)
                .and_then(|record| record.remove("pinned"));
            if removed.is_none() {
                return Ok(false);
            }
            write_json(&marker, &metadata)?;
            lock
        };
        log(&Message::new(
            "Versión liberada: {p1} {p3}. Actualizar volverá a proponer la versión del catálogo.",
        )
        .arg("p1", &lock.key)
        .arg("p3", lock.version_text())
        .to_string());
        Ok(true)
    }

    /// Actualiza un componente a la versión del catálogo en su ruta estable.
    ///
    /// # Errors
    ///
    /// `El componente ya no está instalado`, `No hay una versión posterior en el
    /// catálogo local`, el estado de incompatibilidad y los de [`Engine::install`].
    pub fn update(
        &self,
        installation: &str,
        confirm_unverified: Option<crate::Confirm<'_>>,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<String> {
        let old = self
            .installed()?
            .get(installation)
            .cloned()
            .ok_or_else(|| Error::domain(Message::new("El componente ya no está instalado")))?;
        let Some(candidate) = self.available_update(&old, None)? else {
            return bail(Message::new(
                "No hay una versión posterior en el catálogo local",
            ));
        };
        if candidate.status != mito_catalog::COMPATIBLE {
            return Err(Error::untranslated(candidate.status));
        }
        let (_, manifest) = self.catalog()?.resolve(&old.key)?;
        validate_update_layout(&old, manifest.raw())?;
        let options = InstallOptions {
            confirm_unverified,
            ..InstallOptions::default()
        };
        self.install(&[old.key.as_str()], &options, log, operation)?;
        let replacement = self
            .installed()?
            .get(&old.key)
            .map(Record::version_text)
            .unwrap_or_default();
        log(&Message::new(
            "Actualizado {p1}: {p3} → {p5}. Ruta, persistencia y activación conservadas.",
        )
        .arg("p1", &old.key)
        .arg("p3", old.version_text())
        .arg("p5", replacement)
        .to_string());
        Ok(old.key)
    }

    /// Instalaciones con una versión posterior compatible en el catálogo local.
    ///
    /// # Errors
    ///
    /// Los de lectura del inventario y del catálogo.
    pub fn pending_updates(&self) -> Result<Vec<String>> {
        let catalog = self.catalog_map()?;
        let mut result = Vec::new();
        for (id, lock) in &self.installed()? {
            if self
                .available_update(lock, Some(&catalog))?
                .is_some_and(|c| c.status == mito_catalog::COMPATIBLE)
            {
                result.push(id.to_owned());
            }
        }
        Ok(result)
    }

    /// Actualiza en orden, cada componente con su propio plan atómico. Se detiene en el
    /// primer fallo; omite los que ya no tienen actualización.
    ///
    /// # Errors
    ///
    /// El primer error de [`Engine::update`].
    pub fn update_all(
        &self,
        installations: &[String],
        confirm_unverified: Option<crate::Confirm<'_>>,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<Vec<String>> {
        let mut updated = Vec::new();
        for (position, installation) in installations.iter().enumerate() {
            let lock = self.installed()?.get(installation).cloned();
            let candidate = match &lock {
                Some(lock) => self.available_update(lock, None)?,
                None => None,
            };
            if candidate.is_none_or(|c| c.status != mito_catalog::COMPATIBLE) {
                continue;
            }
            log(&Message::new("Actualizando {p1} de {p3}: {p5}")
                .arg("p1", position + 1)
                .arg("p3", installations.len())
                .arg("p5", installation)
                .to_string());
            updated.push(self.update(installation, confirm_unverified, log, operation)?);
        }
        log(&Message::new("Componentes actualizados: {p1}")
            .arg("p1", updated.len())
            .to_string());
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orden_de_versiones() {
        let key = |v: &str| version_key(v).unwrap();
        assert!(key("1.10") > key("1.9"));
        assert!(key("2.0") > key("2.0-rc1"));
        assert_eq!(key("1.0.0"), key("1"));
        assert_eq!(key("v2.0"), key("2"));
        assert!(version_key("abc").is_none());
        assert!(!key("2.0-ea").stable);
        assert!(key("2.0-earth") < key("2.0"));
        assert!(key("1.0-rc2") > key("1.0-rc1"));
        assert!(key("1.0-beta") < key("1.0-beta1"));
        assert!(version_key("99999999999999999999999999999999999999999").is_none());
    }
}
