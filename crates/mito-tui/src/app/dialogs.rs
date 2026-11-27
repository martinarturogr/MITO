//! Pila de diálogos modales y el uso de su resultado.
//!
//! Cada diálogo se abre con un [`Purpose`] que dice para qué: al cerrarse, su
//! [`Outcome`] se combina con él (la continuación que en Textual era el `await`).

use std::sync::mpsc::Sender;

use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::state::App;
use crate::dialogs::{
    ComponentInfoDialog, ConfigurationDialog, Context, DeleteProfileDialog, Dialog,
    DownloadIntegrityDialog, InstallVersionDialog, LongStepDialog, ManageProfileDialog,
    ManifestDialog, OpenApplicationDialog, OperationDialog, Outcome, ProfileChoice,
    RemoveComponentDialog, Step, UnverifiedDownloadsDialog,
};
use crate::input::Input;
use crate::model::describe;
use crate::widgets::notice::LANGUAGE_TIMEOUT;
use crate::widgets::{Notice, Severity};
use crate::worker::{Action, Request};

/// El diálogo en sí. Los de progreso y paso largo se guardan con su tipo para poder
/// cerrarlos desde la aplicación (`finish`).
pub(super) enum Slot {
    /// Cualquier otro diálogo.
    Plain(Box<dyn Dialog>),
    /// Progreso de la operación.
    Progress(OperationDialog),
    /// Aviso de paso largo.
    LongStep(LongStepDialog),
}

impl Slot {
    pub(super) fn dialog(&mut self) -> &mut dyn Dialog {
        match self {
            Self::Plain(dialog) => dialog.as_mut(),
            Self::Progress(dialog) => dialog,
            Self::LongStep(dialog) => dialog,
        }
    }
}

/// Para qué se abrió un diálogo.
pub(super) enum Purpose {
    /// Manifiesto del componente del catálogo.
    Manifest,
    /// Información del componente instalado.
    Info(String),
    /// Confirmación de eliminar el componente instalado.
    Remove(String),
    /// Abrir la aplicación del componente instalado.
    OpenApplication(String),
    /// Configuración global (`None`) o del profile.
    Configuration(Option<String>),
    /// «Gestionar profile».
    ManageProfile,
    /// Confirmación de borrar el profile seleccionado.
    DeleteProfile,
    /// Pregunta del hilo: la respuesta vuelve por el canal.
    Confirm(Sender<bool>),
    /// Descarga alterada por la red.
    Integrity,
    /// Progreso de la operación.
    Progress,
    /// Aviso de paso largo.
    LongStep,
}

/// Un diálogo abierto.
pub(super) struct Open {
    pub slot: Slot,
    pub purpose: Purpose,
}

impl App {
    /// Abre un diálogo encima de los demás.
    pub(super) fn push_dialog(&mut self, dialog: impl Dialog + 'static, purpose: Purpose) {
        self.dialogs.push(Open {
            slot: Slot::Plain(Box::new(dialog)),
            purpose,
        });
    }

    /// ¿Hay un diálogo de progreso abierto?
    pub(super) fn has_progress(&self) -> bool {
        self.dialogs
            .iter()
            .any(|open| matches!(open.slot, Slot::Progress(_)))
    }

    /// ¿Hay un aviso de paso largo abierto?
    pub(super) fn has_long_step(&self) -> bool {
        self.dialogs
            .iter()
            .any(|open| matches!(open.slot, Slot::LongStep(_)))
    }

    /// Cierra de inmediato los diálogos de progreso y de paso largo (el final de la
    /// operación).
    pub(super) fn close_operation_dialogs(&mut self) {
        self.dialogs
            .retain(|open| !matches!(open.slot, Slot::Progress(_) | Slot::LongStep(_)));
    }

    /// Marca como terminado el aviso de paso largo abierto (`finish`).
    pub(super) fn finish_long_step(&mut self) {
        for open in &mut self.dialogs {
            if let Slot::LongStep(dialog) = &mut open.slot {
                dialog.finish();
            }
        }
    }

    pub(super) fn open_progress(&mut self, dialog: OperationDialog) {
        self.dialogs.push(Open {
            slot: Slot::Progress(dialog),
            purpose: Purpose::Progress,
        });
    }

    pub(super) fn open_long_step(&mut self, dialog: LongStepDialog) {
        self.dialogs.push(Open {
            slot: Slot::LongStep(dialog),
            purpose: Purpose::LongStep,
        });
    }

    /// Entrega una entrada al diálogo de arriba.
    pub(super) fn dialog_input(&mut self, input: &Input) {
        let index = self.dialogs.len().saturating_sub(1);
        let Some(open) = self.dialogs.get_mut(index) else {
            return;
        };
        let mut context = Context {
            tr: &self.tr,
            engine: &self.engine,
            notices: &mut self.notices,
        };
        if let Step::Close(outcome) = open.slot.dialog().handle(input, &mut context) {
            self.close_dialog(index, outcome);
        }
    }

    /// Tic de todos los diálogos (relojes y cierres diferidos).
    pub(super) fn tick_dialogs(&mut self) {
        let mut index = 0;
        while index < self.dialogs.len() {
            let mut context = Context {
                tr: &self.tr,
                engine: &self.engine,
                notices: &mut self.notices,
            };
            let step = self.dialogs[index].slot.dialog().tick(&mut context);
            match step {
                Step::Stay => index += 1,
                Step::Close(outcome) => self.close_dialog(index, outcome),
            }
        }
    }

    /// Dibuja la pila de diálogos, de abajo arriba.
    pub(super) fn render_dialogs(&mut self, frame: &mut Frame, screen: Rect) {
        for open in &mut self.dialogs {
            let dialog = open.slot.dialog();
            let mut ui = dialog.focus().frame(&self.tr);
            dialog.render(frame, screen, &mut ui);
        }
    }

    /// Retira el diálogo `index` y usa su resultado.
    pub(super) fn close_dialog(&mut self, index: usize, outcome: Outcome) {
        let open = self.dialogs.remove(index);
        match (open.purpose, outcome) {
            (Purpose::Info(installation), Outcome::Release) => {
                self.request(Action::ReleaseVersion(installation));
            }
            (Purpose::Remove(installation), Outcome::Confirmed(true)) => {
                self.request(Action::RemoveComponent(installation));
            }
            (Purpose::OpenApplication(installation), Outcome::Open { arguments, entry }) => {
                self.request(Action::Launch {
                    installation,
                    arguments,
                    shortcut: entry,
                });
            }
            (Purpose::Configuration(profile), Outcome::Saved(true)) => {
                self.configuration_saved(profile.as_deref());
            }
            (Purpose::ManageProfile, Outcome::Profile(choice)) => {
                self.manage_profile_result(choice);
            }
            (Purpose::DeleteProfile, Outcome::Confirmed(true)) => {
                self.request(Action::DeleteProfile);
            }
            (Purpose::Confirm(answer), outcome) => {
                let _ = answer.send(matches!(outcome, Outcome::Confirmed(true)));
            }
            _ => {}
        }
    }

    /// «Manifiesto» del componente seleccionado del catálogo.
    pub(super) fn open_manifest(&mut self) {
        let Some(key) = self.catalog.selected.clone() else {
            return;
        };
        let manifest = self
            .catalog
            .data
            .as_ref()
            .and_then(|catalog| catalog.manifest(&key));
        if let Some(manifest) = manifest {
            let dialog = ManifestDialog::new(key, manifest);
            self.push_dialog(dialog, Purpose::Manifest);
        }
    }

    /// «Gestionar profile».
    pub(super) fn open_manage_profile(&mut self) {
        let profile = self.current_profile();
        let can_delete = profile.is_some() && self.engine.profiles().len() > 1;
        let dialog = ManageProfileDialog::new(profile.as_deref(), can_delete);
        self.push_dialog(dialog, Purpose::ManageProfile);
    }

    fn manage_profile_result(&mut self, choice: ProfileChoice) {
        match choice {
            ProfileChoice::Cancel => {}
            ProfileChoice::Create(name) => self.request(Action::Create(name)),
            ProfileChoice::Configuration => {
                let profile = self.current_profile();
                self.open_configuration(profile);
            }
            ProfileChoice::Delete => {
                if let Some(profile) = self.current_profile() {
                    self.push_dialog(DeleteProfileDialog::new(profile), Purpose::DeleteProfile);
                }
            }
        }
    }

    /// `open_configuration`: configuración global (`None`) o de un profile.
    pub(super) fn open_configuration(&mut self, profile: Option<String>) {
        match ConfigurationDialog::new(&self.engine, profile.as_deref()) {
            Ok(dialog) => self.push_dialog(dialog, Purpose::Configuration(profile)),
            Err(error) => {
                self.capture(&error, "configuration.open");
                let text = crate::dialogs::describe_core(&error, &self.tr);
                self.notify(text, Severity::Error);
            }
        }
    }

    fn configuration_saved(&mut self, profile: Option<&str>) {
        let text = self.text(
            "Configuración guardada. Abre nuevas pestañas o aplicaciones para usar el nuevo entorno.",
        );
        self.notify(text, Severity::Info);
        if profile.is_some() {
            return;
        }
        self.refresh_terminal_default();
        let changed = Translator::from_layout(self.engine.layout())
            .is_ok_and(|saved| saved.language() != self.tr.language());
        if changed {
            let text =
                self.text("Vuelve a abrir el gestor MITO para aplicar el idioma seleccionado.");
            self.notices
                .push(Notice::with_timeout(text, Severity::Info, LANGUAGE_TIMEOUT));
        }
    }

    /// Diálogo de una pregunta del hilo.
    pub(super) fn open_confirmation(&mut self, request: Request, answer: Sender<bool>) {
        let purpose = Purpose::Confirm(answer);
        match request {
            Request::InstallVersion {
                key,
                previous,
                version,
            } => self.push_dialog(InstallVersionDialog::new(key, previous, version), purpose),
            Request::Unverified(downloads) => {
                self.push_dialog(UnverifiedDownloadsDialog::new(downloads), purpose);
            }
        }
    }

    /// Diálogo «DESCARGA ALTERADA POR LA RED».
    pub(super) fn open_integrity(&mut self, info: crate::model::IntegrityInfo) {
        let dialog = DownloadIntegrityDialog::new(info.url, info.size, info.host, info.portal);
        self.push_dialog(dialog, Purpose::Integrity);
    }

    /// Información, confirmación de eliminar y apertura de un componente instalado.
    pub(super) fn open_component_dialog(&mut self, kind: ComponentDialog, installation: &str) {
        let record = match self.engine.installed() {
            Ok(records) => records.get(installation).cloned(),
            Err(error) => {
                self.component_error(&error, "component.info");
                return;
            }
        };
        let Some(record) = record else {
            return;
        };
        match kind {
            ComponentDialog::Info => {
                let profile = self.current_profile();
                match ComponentInfoDialog::new(&self.engine, &record, profile.as_deref()) {
                    Ok(dialog) => {
                        self.push_dialog(dialog, Purpose::Info(installation.to_owned()));
                    }
                    Err(error) => self.component_error(&error, "component.info"),
                }
            }
            ComponentDialog::Remove => {
                let dialog = RemoveComponentDialog::new(&record, self.engine.users(installation));
                self.push_dialog(dialog, Purpose::Remove(installation.to_owned()));
            }
            ComponentDialog::Open => {
                if self.current_profile().is_none() {
                    let text = self.text("Selecciona un profile");
                    self.notify(text, Severity::Warning);
                    return;
                }
                match self.engine.application_entries(&record) {
                    Ok(entries) => {
                        let labels = entries.into_iter().map(|entry| entry.label).collect();
                        let dialog = OpenApplicationDialog::new(record.key.clone(), labels);
                        self.push_dialog(dialog, Purpose::OpenApplication(installation.to_owned()));
                    }
                    Err(error) => self.component_error(&error, "component.open"),
                }
            }
        }
    }

    fn component_error(&mut self, error: &mito_engine::Error, operation: &str) {
        self.capture(error, operation);
        let text = describe(error, &self.tr);
        self.notify(text, Severity::Error);
    }
}

/// Qué diálogo de componente instalado se abre.
#[derive(Debug, Clone, Copy)]
pub(super) enum ComponentDialog {
    /// «Info».
    Info,
    /// «Eliminar».
    Remove,
    /// «Abrir».
    Open,
}
