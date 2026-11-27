//! Aviso de nueva versión antes de instalar (`InstallVersionDialog`).

use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Ui, Variant};

const CANCEL: &str = "install-version-cancel";
const CONFIRM: &str = "install-version-confirm";

/// «Nueva versión disponible»: confirma instalar una versión mayor que la mostrada.
/// Se cierra con [`Outcome::Confirmed`]; Esc cancela.
#[derive(Debug)]
pub struct InstallVersionDialog {
    prompt: Prompt,
    key: String,
    previous: String,
    version: String,
}

impl InstallVersionDialog {
    /// Diálogo para `key`, que pasa de `previous` a `version`.
    pub fn new(
        key: impl Into<String>,
        previous: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Instalar",
                variant: Variant::Success,
            },
        ];
        Self {
            prompt: Prompt::new(buttons, CANCEL, Size::Cells(68), theme::BORDER),
            key: key.into(),
            previous: previous.into(),
            version: version.into(),
        }
    }
}

impl Dialog for InstallVersionDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content
            .text(ui.tr.text("Nueva versión disponible"), theme::title())
            .gap(1)
            .text(
                format!("{}\n{} → {}", self.key, self.previous, self.version),
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

    fn dialog() -> InstallVersionDialog {
        InstallVersionDialog::new("main/tool", "1.0", "2.0")
    }

    #[test]
    fn aspecto() {
        let harness = Harness::new();
        let mut dialog = dialog();
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn el_foco_inicial_esta_en_cancelar_y_intro_cancela() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
    }

    #[test]
    fn instalar_con_tab_o_con_clic_confirma() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 80, 24);
        harness.press(&mut dialog, KeyCode::Tab);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = self::dialog();
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, CONFIRM),
            Step::Close(Outcome::Confirmed(true))
        );
    }

    #[test]
    fn esc_cancela() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
