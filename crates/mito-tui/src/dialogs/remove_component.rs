//! Confirmación de eliminar un componente (`RemoveComponentDialog`).

use mito_engine::Record;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Ui, Variant};

const CANCEL: &str = "remove-cancel";
const CONFIRM: &str = "remove-confirm";

/// «Eliminar componente»: pide confirmación y lista los profiles de los que se retira.
/// El foco empieza en «Cancelar». Se cierra con [`Outcome::Confirmed`]; Esc cancela.
#[derive(Debug)]
pub struct RemoveComponentDialog {
    prompt: Prompt,
    key: String,
    version: String,
    profiles: Vec<String>,
}

impl RemoveComponentDialog {
    /// Diálogo para `record`, que está activo en `profiles`.
    pub fn new(record: &Record, profiles: Vec<String>) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Eliminar",
                variant: Variant::Error,
            },
        ];
        Self {
            prompt: Prompt::new(buttons, CANCEL, Size::Cells(68), theme::BORDER_DANGER),
            key: record.key.clone(),
            version: record.version_text(),
            profiles,
        }
    }
}

impl Dialog for RemoveComponentDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let profiles = if self.profiles.is_empty() {
            ui.tr.text("ninguno").to_owned()
        } else {
            self.profiles.join(", ")
        };
        let mut content = Content::new();
        content
            .text(ui.tr.text("Eliminar componente"), theme::title())
            .gap(1)
            .text(format!("{} · {}", self.key, self.version), theme::panel())
            .gap(1)
            .text(
                format!("{}{profiles}", ui.tr.text("Se retirará de estos profiles: ")),
                theme::panel(),
            )
            .gap(1)
            .text(
                ui.tr.text(
                    "Se eliminarán los archivos de este componente. Se conservan el home, los datos persistentes y los workspaces. Las aplicaciones dependientes y sus accesos pueden dejar de funcionar. Cierra el componente antes de eliminarlo.",
                ),
                theme::panel(),
            );
        self.prompt
            .render(frame, screen, ui, &mut content, |_, _, _| {});
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        match self.prompt.handle(input) {
            Some(event) => Step::Close(Outcome::Confirmed(event.confirms(CONFIRM))),
            None => Step::Stay,
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.prompt.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;
    use crossterm::event::KeyCode;
    use serde_json::json;

    fn record() -> Record {
        Record {
            key: "main/tool".into(),
            version: json!("1.2.3"),
            manifest_sha256: String::new(),
            path: "core/apps/main/tool".into(),
            manifest: json!({}),
            pinned: None,
        }
    }

    #[test]
    fn aspecto_con_profiles() {
        let harness = Harness::new();
        let mut dialog = RemoveComponentDialog::new(&record(), vec!["uno".into(), "dos".into()]);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn aspecto_sin_profiles_en_espanol() {
        let harness = Harness::spanish();
        let mut dialog = RemoveComponentDialog::new(&record(), vec![]);
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(
            text.contains("Se retirará de estos profiles: ninguno"),
            "{text}"
        );
        assert!(text.contains("Eliminar componente"), "{text}");
    }

    #[test]
    fn el_foco_empieza_en_cancelar() {
        let mut harness = Harness::new();
        let mut dialog = RemoveComponentDialog::new(&record(), vec![]);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
    }

    #[test]
    fn eliminar_exige_moverse_al_boton_o_hacer_clic() {
        let mut harness = Harness::new();
        let mut dialog = RemoveComponentDialog::new(&record(), vec![]);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(harness.press(&mut dialog, KeyCode::Right), Step::Stay);
        assert_eq!(dialog.focus().focused(), Some(CONFIRM));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Char(' ')),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = RemoveComponentDialog::new(&record(), vec![]);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, CONFIRM),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = RemoveComponentDialog::new(&record(), vec![]);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
