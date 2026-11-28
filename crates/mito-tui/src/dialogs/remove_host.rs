//! Confirmación de quitar una aplicación del host (`RemoveHostDialog`).

use std::path::Path;

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

const CANCEL: &str = "host-remove-cancel";
const CONFIRM: &str = "host-remove-confirm";

/// «¿Quitar {nombre} de MITO?». El foco empieza en «Cancelar». Se cierra con
/// [`Outcome::Confirmed`]; Esc cancela.
#[derive(Debug)]
pub struct RemoveHostDialog {
    prompt: Prompt,
    name: String,
}

impl RemoveHostDialog {
    /// Diálogo para quitar la aplicación del host `path`.
    pub fn new(path: &Path) -> Self {
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
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            prompt: Prompt::new(buttons, CANCEL, Size::Cells(70), theme::BORDER_DANGER),
            name,
        }
    }
}

impl Dialog for RemoveHostDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let question = ui
            .tr
            .tr(&Message::new("¿Quitar {p1} de MITO?").arg("p1", &self.name));
        let mut content = Content::new();
        content.text(question, theme::title()).gap(1).text(
            ui.tr.text(
                "Se eliminan la referencia y sus enlaces de caché/datos. El ejecutable del host y sus datos se conservan.",
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
    use crossterm::event::KeyCode;

    use super::*;
    use crate::dialogs::testing::Harness;

    fn dialog() -> RemoveHostDialog {
        RemoveHostDialog::new(Path::new(r"C:\Apps\Foo.exe"))
    }

    #[test]
    fn aspecto() {
        let harness = Harness::new();
        insta::assert_snapshot!(harness.draw(&mut dialog(), 80, 24));
    }

    #[test]
    fn la_pregunta_lleva_el_nombre_del_archivo() {
        let harness = Harness::spanish();
        let text = harness.draw(&mut dialog(), 80, 24);
        assert!(text.contains("¿Quitar Foo.exe de MITO?"), "{text}");
    }

    #[test]
    fn foco_en_cancelar_y_solo_eliminar_confirma() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
        let mut dialog = self::dialog();
        harness.draw(&mut dialog, 80, 24);
        harness.press(&mut dialog, KeyCode::Tab);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = self::dialog();
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
