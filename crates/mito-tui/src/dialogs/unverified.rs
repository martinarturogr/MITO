//! Confirmación de descargas sin hash (`UnverifiedDownloadsDialog`).

use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, TextView, Ui, Variant};

const CANCEL: &str = "unverified-cancel";
const CONFIRM: &str = "unverified-confirm";
const LIST: &str = "unverified-list";

/// Filas visibles de la lista de descargas antes de pasar a desplazarla.
const MAX_LIST_ROWS: usize = 8;

/// «DESCARGAS SIN HASH»: avisa de que no se podrá comprobar la integridad. El foco
/// empieza en «Cancelar». Se cierra con [`Outcome::Confirmed`]; Esc cancela.
#[derive(Debug)]
pub struct UnverifiedDownloadsDialog {
    prompt: Prompt,
    downloads: Vec<String>,
    list: TextView,
    rows: u16,
}

impl UnverifiedDownloadsDialog {
    /// Diálogo con las URL de las descargas, una por línea.
    pub fn new(downloads: Vec<String>) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Descargar e instalar",
                variant: Variant::Warning,
            },
        ];
        let rows = u16::try_from(downloads.len().clamp(1, MAX_LIST_ROWS)).unwrap_or(1);
        Self {
            prompt: Prompt::new(
                buttons,
                CANCEL,
                Size::PercentMax(85, 78),
                theme::BORDER_WARNING,
            ),
            list: TextView::from_text(LIST, &downloads.join("\n")),
            downloads,
            rows,
        }
    }

    /// Descargas que se muestran.
    pub fn downloads(&self) -> &[String] {
        &self.downloads
    }
}

impl Dialog for UnverifiedDownloadsDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content
            .text(ui.tr.text("DESCARGAS SIN HASH"), theme::title())
            .gap(1)
            .text(
                ui.tr.text(
                    "No se podrá comprobar la integridad de estas descargas. ¿Quieres continuar?",
                ),
                theme::panel(),
            )
            .gap(1)
            .slot("list", self.rows);
        let list = &mut self.list;
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                list.render(frame, slots.get("list"), ui);
            });
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if matches!(input, Input::Scroll { .. }) {
            self.list.handle(input);
            return Step::Stay;
        }
        if self.prompt.focus.focused() == Some(LIST)
            && !input.is_tab()
            && !input.is_back_tab()
            && !input.is(crossterm::event::KeyCode::Esc)
        {
            self.list.handle(input);
            return Step::Stay;
        }
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

    fn dialog() -> UnverifiedDownloadsDialog {
        UnverifiedDownloadsDialog::new(vec![
            "https://example.com/a.zip".into(),
            "https://example.com/b.zip".into(),
        ])
    }

    #[test]
    fn aspecto() {
        let harness = Harness::new();
        let mut dialog = dialog();
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn el_foco_empieza_en_cancelar() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(false))
        );
    }

    #[test]
    fn descargar_e_instalar_confirma() {
        let mut harness = Harness::new();
        let mut dialog = dialog();
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, CONFIRM),
            Step::Close(Outcome::Confirmed(true))
        );
    }

    #[test]
    fn esc_cancela_y_la_lista_larga_se_desplaza() {
        let mut harness = Harness::new();
        let downloads: Vec<String> = (0..12)
            .map(|i| format!("https://example.com/{i}.zip"))
            .collect();
        let mut dialog = UnverifiedDownloadsDialog::new(downloads);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, LIST);
        harness.press(&mut dialog, KeyCode::End);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("11.zip") && !text.contains("/0.zip"),
            "{text}"
        );
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
