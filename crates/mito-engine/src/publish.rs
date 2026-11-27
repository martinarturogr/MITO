//! Publicación de un plan de instalación con rollback (`components.publish_plan`).
//!
//! El llamador tiene el lock de mutación. Todo el plan se prepara en
//! `core/staging/install-<uuid>` antes de reemplazar nada; el último punto cancelable es
//! justo antes de la fase `PUBLICANDO`. A partir de ahí cualquier fallo restaura, en
//! este orden: accesos directos, lo publicado, lo apartado, los profiles y el staging.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use mito_catalog::array;
use mito_catalog::pyjson::py_str;
use mito_core::storage::inside;
use mito_core::{Message, Operation};

use crate::Log;
use crate::engine::Engine;
use crate::error::{Error, Result, bail};
use crate::fsutil::{check_unused, create_dir_all, is_link, remove_tree, rename, staging_name};
use crate::isolation::{isolated, payload_path};
use crate::profiles::Profile;
use crate::record::{Record, Records};

/// Lo que hay que deshacer si la publicación falla.
#[derive(Default)]
struct Journal {
    moved: Vec<(PathBuf, PathBuf)>,
    published: Vec<PathBuf>,
    written: Vec<String>,
    links: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl Engine {
    /// Dependencias (recursivas) de `key` según su manifiesto.
    fn dependencies(
        catalog: &mito_catalog::Catalog,
        records: &Records,
        key: &str,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        let Some(record) = records.get(key) else {
            return Ok(());
        };
        let manifest = record.effective()?;
        for name in array(manifest.get("depends")) {
            let (dependency, _) = catalog.resolve(&py_str(name))?;
            if seen.insert(dependency.clone()) {
                Self::dependencies(catalog, records, &dependency, seen)?;
            }
        }
        Ok(())
    }

    /// Publica `records` reemplazando los componentes que cambian.
    pub(crate) fn publish_plan(
        &self,
        records: &Records,
        current: &Records,
        profile: Option<&str>,
        data: Option<&Profile>,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<()> {
        let layout = self.layout();
        let changed: Vec<(&str, &Record)> = records
            .iter()
            .filter(|(key, record)| {
                current.get(key).is_none_or(|old| {
                    old.manifest_sha256 != record.manifest_sha256 || old.pinned != record.pinned
                })
            })
            .collect();
        let profiles: BTreeMap<String, Profile> = self
            .profiles()
            .into_iter()
            .map(|name| self.profile(&name).map(|p| (name, p)))
            .collect::<Result<_>>()?;
        let mut affected: BTreeSet<String> = profiles
            .iter()
            .filter(|(_, value)| {
                value
                    .apps
                    .iter()
                    .any(|app| current.contains(app) && changed.iter().any(|(k, _)| k == app))
            })
            .map(|(name, _)| name.clone())
            .collect();
        if let Some(profile) = profile {
            affected.insert(profile.to_owned());
        }
        let mut targets: Vec<PathBuf> = Vec::new();
        let mut copies: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (key, record) in &changed {
            let base = inside(layout.root(), &record.path)?;
            if is_link(&base) {
                return bail(Message::new("Ruta de instalación enlazada no válida"));
            }
            if base.exists() {
                if !current.contains(key) {
                    return bail(
                        Message::new("Instalación incompleta existente: {p1}")
                            .arg("p1", base.display()),
                    );
                }
                targets.push(base);
            }
            if isolated(record)? {
                let mut names: BTreeSet<String> = profiles
                    .iter()
                    .filter(|(_, value)| value.apps.iter().any(|a| a == key))
                    .map(|(name, _)| name.clone())
                    .collect();
                if let Ok(folders) = std::fs::read_dir(layout.profile_apps()) {
                    for folder in folders.flatten() {
                        let name = folder.file_name().to_string_lossy().into_owned();
                        let target = payload_path(layout, record, Some(&name))?;
                        if target.exists() {
                            if is_link(&target) {
                                return bail(Message::new("Copia de profile no válida"));
                            }
                            targets.push(target);
                            names.insert(name);
                        }
                    }
                }
                copies.insert(key, names);
            }
        }
        for target in &targets {
            check_unused(target, false)?;
        }
        let stage = inside(
            layout.root(),
            &format!("core/staging/{}", staging_name("install")),
        )?;
        create_dir_all(&stage)?;
        let mut journal = Journal::default();
        let result = self.publish_steps(
            &changed,
            &copies,
            &targets,
            &stage,
            &mut journal,
            (&profiles, &affected),
            (records, profile, data),
            log,
            operation,
        );
        if let Err(error) = result {
            self.rollback(&journal, &profiles);
            // El staging está confinado en esta instancia; no se recorren junctions.
            let _ = remove_tree(&stage);
            let _ = self.refresh_launchers(affected.iter().map(String::as_str));
            return Err(error);
        }
        remove_tree(&stage).map_err(|error| {
            Error::domain(
                Message::new("Componente publicado; limpieza pendiente en {p1}: {p3}")
                    .arg("p1", stage.display())
                    .arg("p3", error),
            )
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "reproduce el cuerpo del original"
    )]
    fn publish_steps(
        &self,
        changed: &[(&str, &Record)],
        copies: &BTreeMap<&str, BTreeSet<String>>,
        targets: &[PathBuf],
        stage: &std::path::Path,
        journal: &mut Journal,
        (profiles, affected): (&BTreeMap<String, Profile>, &BTreeSet<String>),
        (records, profile, data): (&Records, Option<&str>, Option<&Profile>),
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<()> {
        let layout = self.layout();
        for (index, (_, record)) in changed.iter().enumerate() {
            let staged = stage.join("new").join(index.to_string());
            self.install_payload(record, &staged, log, operation)?;
        }
        // Último punto en el que cancelar solo descarta el staging.
        operation.check()?;
        operation.phase(
            "PUBLICANDO",
            Message::new("Publicando {p1}; ya no se puede cancelar.")
                .arg(
                    "p1",
                    changed
                        .iter()
                        .map(|(k, _)| *k)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .to_string(),
            false,
        );
        // Otro proceso pudo abrir un binario mientras se descargaba.
        for target in targets {
            check_unused(target, false)?;
        }
        for (index, target) in targets.iter().enumerate() {
            let stored = stage.join("old").join(index.to_string());
            if let Some(parent) = stored.parent() {
                create_dir_all(parent)?;
            }
            rename(target, &stored)?;
            journal.moved.push((target.clone(), stored));
        }
        for (index, (key, record)) in changed.iter().enumerate() {
            let target = inside(layout.root(), &record.path)?;
            if let Some(parent) = target.parent() {
                create_dir_all(parent)?;
            }
            rename(&stage.join("new").join(index.to_string()), &target)?;
            journal.published.push(target);
            if isolated(record)? {
                for name in copies.get(key).into_iter().flatten() {
                    journal
                        .published
                        .push(payload_path(layout, record, Some(name))?);
                    self.prepare_payload(record, name)?;
                }
            } else {
                self.prepare_payload(record, profile.unwrap_or("default"))?;
            }
        }
        let catalog = self.catalog()?;
        let changed_keys: BTreeSet<&str> = changed.iter().map(|(k, _)| *k).collect();
        for name in affected {
            let mut value = match (Some(name.as_str()) == profile, data) {
                (true, Some(data)) => data.clone(),
                _ => profiles.get(name).cloned().unwrap_or_default(),
            };
            if Some(name.as_str()) != profile {
                // Solo las dependencias de lo reemplazado pasan a estar activas.
                let mut needed = BTreeSet::new();
                for app in &profiles.get(name).cloned().unwrap_or_default().apps {
                    if changed_keys.contains(app.as_str()) {
                        Self::dependencies(&catalog, records, app, &mut needed)?;
                    }
                }
                for key in records.keys() {
                    if needed.contains(key) && !value.apps.iter().any(|a| a == key) {
                        value.apps.push(key.to_owned());
                    }
                }
            }
            self.save_profile(name, &value)?;
            journal.written.push(name.clone());
        }
        for (key, record) in changed {
            self.extensions().host.migrate_application_links(
                self,
                key,
                record,
                &mut journal.links,
                log,
            )?;
        }
        self.refresh_launchers(affected.iter().map(String::as_str))
    }

    /// Deshace una publicación fallida, sin interrumpirse por errores secundarios.
    fn rollback(&self, journal: &Journal, profiles: &BTreeMap<String, Profile>) {
        for (path, content) in &journal.links {
            match content {
                None => {
                    let _ = std::fs::remove_file(path);
                }
                Some(bytes) => {
                    let _ = std::fs::write(path, bytes);
                }
            }
        }
        for target in journal.published.iter().rev() {
            if target.exists() {
                let _ = remove_tree(target);
            }
        }
        for (target, stored) in journal.moved.iter().rev() {
            let _ = std::fs::rename(stored, target);
        }
        for name in &journal.written {
            if let Some(original) = profiles.get(name) {
                let _ = crate::profiles::save(self.layout(), name, original);
            }
        }
    }
}
