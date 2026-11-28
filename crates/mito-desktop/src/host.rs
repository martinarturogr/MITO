//! Punto de extensión [`mito_engine::Host`] con la integración de Windows de F8: el motor
//! lo llama al publicar una versión, al borrar un profile y al eliminar un componente.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mito_core::Message;
use mito_engine::{Engine, Host, Log, Record, Result};

use crate::host_data::HostRoots;
use crate::open_with::{self, Classes};
use crate::{host_exposure, shortcuts};

/// Integración real con Windows: accesos de escritorio, «Abrir con» y exposición de
/// `AppData` al host.
#[derive(Debug, Clone)]
pub struct DesktopHost {
    classes: Classes,
}

impl Default for DesktopHost {
    fn default() -> Self {
        Self {
            classes: Classes::system(),
        }
    }
}

impl DesktopHost {
    /// Integración sobre `classes` (en pruebas, una rama de registro desechable).
    pub fn with_classes(classes: Classes) -> Self {
        Self { classes }
    }
}

impl Host for DesktopHost {
    fn migrate_application_links(
        &self,
        engine: &Engine,
        key: &str,
        record: &Record,
        links: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
        log: Log<'_>,
    ) -> Result<()> {
        shortcuts::migrate_application_links(engine, key, record, links, log)
    }

    fn unregister_profile(&self, engine: &Engine, profile: &str) -> Result<()> {
        open_with::profile_deleted(&self.classes, engine, profile)
    }

    /// Como `components.remove` del original: retira la exposición al host (un fallo se
    /// registra y no impide lo demás) y las entradas de «Abrir con» del componente.
    fn unregister_installation(
        &self,
        engine: &Engine,
        installation: &str,
        log: Log<'_>,
    ) -> Result<()> {
        let exposed = HostRoots::system().and_then(|roots| {
            host_exposure::unregister_installation(engine.root(), &roots, installation)
        });
        if let Err(error) = exposed {
            log(
                Message::new("No se pudieron retirar los enlaces del host: {p1}")
                    .arg("p1", error)
                    .into(),
            );
        }
        open_with::installation_removed(&self.classes, engine, installation, log)
    }
}
