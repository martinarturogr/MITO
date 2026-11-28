//! Confirmación de enlaces del host (`HostDataConfirm`).

use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Ui, Variant};

const CANCEL: &str = "host-data-cancel";
const ACCEPT: &str = "host-data-accept";

/// Pregunta de enlaces del host con un botón «Cancelar» (foco inicial) y otro,
/// `warning`, con la etiqueta que se indique («Enlazar», «Desenlazar», «Borrar y
/// enlazar», «Seleccionar», «Apartar y enlazar»). Se cierra con [`Outcome::Confirmed`];
/// Esc cancela.
#[derive(Debug)]
pub struct HostDataConfirm {
    prompt: Prompt,
    message: String,
}

impl HostDataConfirm {
    /// Diálogo con el texto ya traducido y la clave de traducción de la etiqueta del
    /// botón que acepta.
    pub fn new(message: impl Into<String>, label: &'static str) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: ACCEPT,
                label,
                variant: Variant::Warning,
            },
        ];
        Self {
            prompt: Prompt::new(buttons, CANCEL, Size::PercentMax(90, 110), theme::BORDER),
            message: message.into(),
        }
    }
}

impl Dialog for HostDataConfirm {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content.text(&self.message, theme::panel());
        self.prompt
            .render(frame, screen, ui, &mut content, |_, _, _| {});
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        match self.prompt.handle(input) {
            Some(event) => Step::Close(Outcome::Confirmed(event.confirms(ACCEPT))),
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

    const MESSAGE: &str = "Host data: ¿crear un enlace?\nHost: C:\\Users\\a\\AppData\\Roaming\\Foo\nMITO: C:\\mito\\home\\AppData\\Roaming\\Foo\nCompartido por todos los profiles.";

    #[test]
    fn aspecto() {
        let harness = Harness::spanish();
        let mut dialog = HostDataConfirm::new(MESSAGE, "Enlazar");
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn la_etiqueta_del_boton_es_la_pedida() {
        let harness = Harness::spanish();
        let mut dialog = HostDataConfirm::new("¿Seguro?", "Desenlazar");
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(text.contains("Desenlazar"), "{text}");
        assert!(!text.contains("Enlazar ]"), "{text}");
    }

    #[test]
    fn foco_en_cancelar_y_solo_el_boton_de_aceptar_confirma() {
        let mut harness = Harness::new();
        let mut dialog = HostDataConfirm::new(MESSAGE, "Enlazar");
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
        let mut dialog = HostDataConfirm::new(MESSAGE, "Enlazar");
        harness.draw(&mut dialog, 100, 24);
        harness.press(&mut dialog, KeyCode::Tab);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = HostDataConfirm::new(MESSAGE, "Enlazar");
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
