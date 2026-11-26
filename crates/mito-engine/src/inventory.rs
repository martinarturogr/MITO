//! Inventario de componentes instalados (`core/apps/<bucket>/<app>/.mito.json`).
//!
//! El inventario es la única fuente de verdad de lo instalado, independiente de los
//! profiles y del catálogo. Activar y desactivar solo cambia `apps` de un profile.

use std::fs;
use std::path::{Path, PathBuf};

use mito_core::Message;
use mito_core::storage::read_json;

use crate::engine::Engine;
use crate::error::{Error, Result, bail};
use crate::profiles::Profile;
use crate::record::{MARKER, Record, Records};

/// Subcarpetas de `dir` ordenadas por nombre.
fn folders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    paths.sort();
    paths
}

/// ¿Hay algún `core/apps/*/*/*/.mito.json` (almacenamiento de tres niveles antiguo)?
fn legacy_layout(apps: &Path) -> bool {
    folders(apps)
        .iter()
        .flat_map(|bucket| folders(bucket))
        .flat_map(|app| folders(&app))
        .any(|version| version.join(MARKER).is_file())
}

impl Engine {
    /// Componentes instalados por id de instalación (`bucket/app`), en el orden de sus
    /// marcadores. El registro lleva `path` relativo a la raíz, con `/`.
    ///
    /// # Errors
    ///
    /// `Almacenamiento antiguo: …` si queda el layout de tres niveles e `Identidad o ruta
    /// de componente incoherente` si un marcador no está en `core/apps/<key>` o la clave
    /// se repite; errores de lectura de los marcadores.
    pub fn installed(&self) -> Result<Records> {
        let apps = self.layout().apps();
        if legacy_layout(&apps) {
            return bail(Message::new(
                "Almacenamiento antiguo: cierra las aplicaciones y ejecuta mito.cmd migrate-storage",
            ));
        }
        let mut result = Records::new();
        for bucket in folders(&apps) {
            for app in folders(&bucket) {
                let marker = app.join(MARKER);
                if !marker.is_file() {
                    continue;
                }
                let metadata: serde_json::Value = read_json(&marker)?;
                let mut record = Record::from_value(metadata.get("record").unwrap_or(&metadata))?;
                let relative = format!(
                    "core/apps/{}/{}",
                    bucket.file_name().unwrap_or_default().to_string_lossy(),
                    app.file_name().unwrap_or_default().to_string_lossy()
                );
                if relative != format!("core/apps/{}", record.key) || result.contains(&record.key) {
                    return bail(Message::new("Identidad o ruta de componente incoherente"));
                }
                record.path = relative;
                result.insert(record.key.clone(), record);
            }
        }
        Ok(result)
    }

    /// Componentes de `data.apps` en su orden (el de activación y del `PATH`).
    ///
    /// # Errors
    ///
    /// `Falta la instalación: <ids>` si alguno no está instalado.
    pub fn selected(&self, data: &Profile) -> Result<Records> {
        let installed = self.installed()?;
        select(&installed, data)
    }

    /// Activa o desactiva un componente en un profile. No instala ni borra nada.
    ///
    /// Activar añade el id al final de `apps` si no estaba. Desactivar lo quita y no
    /// exige que esté instalado, para poder retirar entradas rotas.
    ///
    /// # Errors
    ///
    /// `El componente no está instalado` al activar uno que no lo está, y los de
    /// [`Self::profile`].
    pub fn set_active(&self, profile: &str, install_id: &str, active: bool) -> Result<()> {
        let _lock = self.mutation()?;
        let mut data = self.profile(profile)?;
        let installed = self.installed()?;
        if active {
            if !installed.contains(install_id) {
                return bail(Message::new("El componente no está instalado"));
            }
            if !data.apps.iter().any(|app| app == install_id) {
                data.apps.push(install_id.to_owned());
            }
        } else {
            data.apps.retain(|app| app != install_id);
        }
        self.save_profile(profile, &data)?;
        self.extensions().terminal.prepare(self)?;
        Ok(())
    }

    /// Nombres de los profiles que tienen activo `installation`.
    pub fn users(&self, installation: &str) -> Vec<String> {
        self.profiles()
            .into_iter()
            .filter(|name| {
                self.profile(name)
                    .is_ok_and(|p| p.apps.iter().any(|a| a == installation))
            })
            .collect()
    }
}

/// Selección de `data.apps` sobre un inventario ya leído.
pub(crate) fn select(installed: &Records, data: &Profile) -> Result<Records> {
    let missing: Vec<&str> = data
        .apps
        .iter()
        .map(String::as_str)
        .filter(|app| !installed.contains(app))
        .collect();
    if !missing.is_empty() {
        return Err(Error::domain(
            Message::new("Falta la instalación: ").with_suffix(missing.join(", ")),
        ));
    }
    Ok(data
        .apps
        .iter()
        .filter_map(|app| installed.get(app).map(|r| (app.clone(), r.clone())))
        .collect())
}
