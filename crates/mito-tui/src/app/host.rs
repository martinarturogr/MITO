//! Aplicaciones del host (F8): botones de su fila, elección al agregar y accesos de MITO.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use mito_desktop::host_apps;
use mito_desktop::host_data::Kind;

use super::dialogs::Purpose;
use super::state::App;
use crate::dialogs::{
    HostDiscoverDialog, HostInfoDialog, OpenApplicationDialog, RemoveHostDialog, ShortcutDialog,
};
use crate::model::describe;
use crate::widgets::Severity;
use crate::worker::{Action, Pick};

impl App {
    /// El profile seleccionado o, si no hay, el aviso «Selecciona un profile».
    pub(super) fn require_profile(&mut self) -> Option<String> {
        let profile = self.current_profile();
        if profile.is_none() {
            let text = self.text("Selecciona un profile");
            self.notify(text, Severity::Warning);
        }
        profile
    }

    /// Ruta registrada de la aplicación del host o, si falla, el aviso con el error.
    fn host_path(&mut self, identity: &str) -> Option<PathBuf> {
        match host_apps::target(self.engine.root(), identity) {
            Ok(path) => Some(path),
            Err(error) => {
                let text = describe(&error, &self.tr);
                self.notify(text, Severity::Error);
                None
            }
        }
    }

    /// Un botón de la fila de la aplicación del host `identity`.
    pub(super) fn host_button(&mut self, action: &str, identity: &str) {
        match action {
            "cache" | "data" => {
                let kind = if action == "cache" {
                    Kind::Cache
                } else {
                    Kind::Data
                };
                self.request(Action::HostLink {
                    identity: identity.to_owned(),
                    kind,
                });
            }
            "open" => {
                if self.require_profile().is_none() {
                    return;
                }
                if let Some(path) = self.host_path(identity) {
                    let name = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let dialog = OpenApplicationDialog::new(name, Vec::new());
                    self.push_dialog(dialog, Purpose::HostOpen(identity.to_owned()));
                }
            }
            "link" => {
                if self.require_profile().is_none() {
                    return;
                }
                if self.host_path(identity).is_some() {
                    self.open_shortcut(
                        true,
                        Vec::new(),
                        Purpose::HostShortcut(identity.to_owned()),
                    );
                }
            }
            "info" => {
                if let Some(path) = self.host_path(identity) {
                    self.push_dialog(HostInfoDialog::new(path), Purpose::HostInfo);
                }
            }
            "remove" => {
                if let Some(path) = self.host_path(identity) {
                    let dialog = RemoveHostDialog::new(&path);
                    self.push_dialog(dialog, Purpose::HostRemove(identity.to_owned()));
                }
            }
            _ => {}
        }
    }

    /// `ShortcutDialog`: sus valores por defecto salen de `config.properties` y un valor
    /// no válido allí se avisa en lugar de romper la interfaz.
    pub(super) fn open_shortcut(
        &mut self,
        application: bool,
        entries: Vec<String>,
        purpose: Purpose,
    ) {
        match ShortcutDialog::new(&self.engine, application, entries) {
            Ok(dialog) => self.push_dialog(dialog, purpose),
            Err(error) => {
                self.capture(&error, "shortcut.open");
                let text = describe(&error, &self.tr);
                self.notify(text, Severity::Error);
            }
        }
    }

    /// «Acceso directo» de «Gestionar profile».
    pub(super) fn open_mito_shortcut(&mut self) {
        if self.require_profile().is_some() {
            self.open_shortcut(false, Vec::new(), Purpose::MitoShortcut);
        }
    }

    /// `HostDiscoverDialog`: el hilo espera la elección en `answer`.
    pub(super) fn open_host_discover(
        &mut self,
        found: Vec<(String, PathBuf)>,
        answer: Sender<Pick>,
    ) {
        self.push_dialog(HostDiscoverDialog::new(found), Purpose::Pick(answer));
    }
}
