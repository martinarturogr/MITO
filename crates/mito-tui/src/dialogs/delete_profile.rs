//! Confirmación de eliminar un profile (`DeleteProfileDialog`).

use mito_core::Message;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Ui, Variant};

const CANCEL: &str = "delete-profile-cancel";
const CONFIRM: &str = "delete-profile-confirm";

/// «¿Eliminar el profile …?». El foco empieza en «Cancelar». Se cierra con
/// [`Outcome::Confirmed`]; Esc cancela.
#[derive(Debug)]
pub struct DeleteProfileDialog {
    prompt: Prompt,
    profile: String,
}

impl DeleteProfileDialog {
    /// Diálogo para eliminar `profile`.
    pub fn new(profile: impl Into<String>) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Eliminar profile",
                variant: Variant::Error,
            },
        ];
        Self {
            prompt: Prompt::new(buttons, CANCEL, Size::Cells(66), theme::BORDER_DANGER),
            profile: profile.into(),
        }
    }
}

impl Dialog for DeleteProfileDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let question = ui
            .tr
            .tr(&Message::new("¿Eliminar el profile {p1}?").arg("p1", &self.profile));
        let mut content = Content::new();
        content
            .text(question, theme::title())
            .gap(1)
            .text(
                ui.tr.text(
                    "Se eliminará su configuración. Se conservan componentes, datos persistentes, home y workspace. Los accesos de este profile dejarán de funcionar.",
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

    #[test]
    fn aspecto() {
        let harness = Harness::new();
        let mut dialog = DeleteProfileDialog::new("trabajo");
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn la_pregunta_lleva_el_nombre_del_profile() {
        let harness = Harness::spanish();
        let mut dialog = DeleteProfileDialog::new("trabajo");
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(text.contains("¿Eliminar el profile trabajo?"), "{text}");
    }

    #[test]
    fn foco_en_cancelar_y_solo_el_boton_rojo_confirma() {
        let mut harness = Harness::new();
        let mut dialog = DeleteProfileDialog::new("trabajo");
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
        let mut dialog = DeleteProfileDialog::new("trabajo");
        harness.draw(&mut dialog, 80, 24);
        harness.press(&mut dialog, KeyCode::Tab);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = DeleteProfileDialog::new("trabajo");
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
