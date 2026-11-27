//! Aviso de descarga alterada por la red (`DownloadIntegrityDialog`).

use mito_core::Message;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, TextView, Ui, Variant};

const CLOSE: &str = "integrity-close";

/// «DESCARGA ALTERADA POR LA RED»: explica la causa probable y muestra la URL, el
/// tamaño recibido y el host. Cualquier botón o Esc lo cierran con [`Outcome::Closed`].
#[derive(Debug)]
pub struct DownloadIntegrityDialog {
    prompt: Prompt,
    url: String,
    size: Option<u64>,
    host: String,
    portal: bool,
    details: TextView,
}

impl DownloadIntegrityDialog {
    /// Diálogo con los datos de la descarga alterada. Una URL o un host vacíos se
    /// muestran como `?`, igual que un tamaño desconocido; `portal` indica que la
    /// causa más probable es el portal de autenticación de la red.
    pub fn new(url: String, size: Option<u64>, host: String, portal: bool) -> Self {
        let buttons = vec![PromptButton {
            id: CLOSE,
            label: "Cerrar",
            variant: Variant::Primary,
        }];
        Self {
            prompt: Prompt::new(
                buttons,
                CLOSE,
                Size::PercentMax(95, 90),
                theme::BORDER_DANGER,
            ),
            url,
            size,
            host,
            portal,
            details: TextView::new("integrity-details")
                .wrapped(true)
                .focusable(false),
        }
    }

    fn details_text(&self, tr: &mito_i18n::Translator) -> String {
        let unknown = |text: &str| {
            if text.is_empty() {
                "?".to_owned()
            } else {
                text.to_owned()
            }
        };
        let size = self
            .size
            .map_or_else(|| "?".to_owned(), |size| size.to_string());
        tr.tr(&Message::new("URL: {p1}\nRecibido: {p2} bytes desde {p3}")
            .arg("p1", unknown(&self.url))
            .arg("p2", size)
            .arg("p3", unknown(&self.host)))
    }
}

impl Dialog for DownloadIntegrityDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let cause = if self.portal {
            "Causa más probable: el portal de autenticación de la red. Pulsa INTERNET, conéctate y vuelve a intentarlo."
        } else {
            "Causa más probable: el proxy o el antivirus de la red ha sustituido o modificado la descarga. No es un fallo de MITO ni del catálogo. Prueba desde otra red o pasa estos datos a soporte."
        };
        self.details.set_text(&self.details_text(ui.tr));
        let mut content = Content::new();
        content
            .text(
                ui.tr.text("DESCARGA ALTERADA POR LA RED"),
                Style::new()
                    .fg(theme::BORDER_DANGER)
                    .bg(theme::SIDEBAR)
                    .add_modifier(Modifier::BOLD),
            )
            .gap(1)
            .text(
                ui.tr.text(
                    "Lo que ha llegado no es el archivo publicado, así que MITO lo ha descartado y no ha instalado nada.",
                ),
                theme::panel(),
            )
            .gap(1)
            .text(ui.tr.text(cause), theme::panel())
            .gap(1)
            .slot("details", 4);
        let details = &mut self.details;
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                details.render(frame, slots.get("details"), ui);
            });
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if matches!(input, Input::Scroll { .. }) {
            self.details.handle(input);
            return Step::Stay;
        }
        match self.prompt.handle(input) {
            Some(_) => Step::Close(Outcome::Closed),
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

    fn dialog(portal: bool) -> DownloadIntegrityDialog {
        DownloadIntegrityDialog::new(
            "https://example.com/app.zip".into(),
            Some(1234),
            "example.com".into(),
            portal,
        )
    }

    #[test]
    fn aspecto_con_proxy_o_antivirus() {
        let harness = Harness::new();
        let mut dialog = dialog(false);
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn aspecto_con_portal() {
        let harness = Harness::new();
        let mut dialog = dialog(true);
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn los_datos_desconocidos_se_muestran_como_interrogacion() {
        let harness = Harness::spanish();
        let mut dialog = DownloadIntegrityDialog::new(String::new(), None, String::new(), false);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("URL: ?"), "{text}");
        assert!(text.contains("Recibido: ? bytes desde ?"), "{text}");
    }

    #[test]
    fn cerrar_esc_y_clic_cierran() {
        let mut harness = Harness::new();
        for code in [KeyCode::Enter, KeyCode::Esc] {
            let mut dialog = dialog(false);
            harness.draw(&mut dialog, 100, 30);
            assert_eq!(dialog.focus().focused(), Some(CLOSE));
            assert_eq!(
                harness.press(&mut dialog, code),
                Step::Close(Outcome::Closed)
            );
        }
        let mut dialog = dialog(false);
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Closed)
        );
    }
}
