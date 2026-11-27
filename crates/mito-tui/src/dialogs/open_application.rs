//! Diálogo «Abrir» una aplicación con argumentos (`OpenApplicationDialog`).

use crossterm::event::KeyCode;
use mito_engine::arguments::split;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step, navigate};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Select, TextInput, Ui, Variant, WidgetEvent};

const CANCEL: &str = "application-open-cancel";
const OPEN: &str = "application-open-independent";
const ENTRY: &str = "application-entry";
const ARGUMENTS: &str = "application-arguments";

/// «Abrir»: nombre del componente, entrada de aplicación (si tiene varias) y
/// argumentos. En F6 el proceso es siempre independiente (`detached`), así que no hay
/// selector «Proceso». Se cierra con [`Outcome::Open`] o, al cancelar o con Esc, con
/// [`Outcome::Closed`].
#[derive(Debug)]
pub struct OpenApplicationDialog {
    prompt: Prompt,
    name: String,
    entry: Option<Select<String>>,
    arguments: TextInput,
}

impl OpenApplicationDialog {
    /// Diálogo para abrir `name`; `entries` son las entradas de aplicación del
    /// manifiesto (con una o ninguna no se muestra el selector si está vacío).
    pub fn new(name: impl Into<String>, entries: Vec<String>) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: OPEN,
                label: "Abrir",
                variant: Variant::Primary,
            },
        ];
        let entry = (!entries.is_empty()).then(|| {
            let options = entries.into_iter().map(|e| (e.clone(), e)).collect();
            Select::new(ENTRY, options, Some(0))
        });
        let mut prompt = Prompt::new(buttons, ARGUMENTS, Size::Cells(62), theme::BORDER);
        prompt.focus.set(ARGUMENTS);
        Self {
            prompt,
            name: name.into(),
            entry,
            arguments: TextInput::new(ARGUMENTS),
        }
    }

    fn open(&self) -> Step {
        Step::Close(Outcome::Open {
            arguments: split(&self.arguments.value),
            entry: self
                .entry
                .as_ref()
                .and_then(|select| select.value().cloned()),
        })
    }

    fn entry_open(&self) -> bool {
        self.entry.as_ref().is_some_and(|select| select.open)
    }

    fn on_button(&self, id: &str) -> Option<Step> {
        match id {
            CANCEL => Some(Step::Close(Outcome::Closed)),
            OPEN => Some(self.open()),
            _ => None,
        }
    }
}

impl Dialog for OpenApplicationDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content
            .text(ui.tr.text("Abrir"), theme::title())
            .gap(1)
            .text(&self.name, theme::panel())
            .gap(1);
        if self.entry.is_some() {
            content.slot("entry", 1).gap(1);
        }
        content
            .text(ui.tr.text("Argumentos"), theme::panel())
            .slot("arguments", 1);
        let entry = &self.entry;
        let arguments = &self.arguments;
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                if let Some(select) = entry {
                    select.render(frame, slots.get("entry"), ui);
                }
                arguments.render(frame, slots.get("arguments"), ui);
            });
        if let Some(select) = &self.entry {
            select.render_overlay(frame, ui);
        }
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        // Con la lista del selector abierta, ella recibe las entradas.
        if self.entry_open() {
            let hit = match input {
                Input::Click { column, row } => self.prompt.focus.hit(*column, *row),
                _ => None,
            };
            let closing = input.is_tab()
                || input.is_back_tab()
                || (matches!(input, Input::Click { .. }) && hit.as_deref() != Some(ENTRY));
            if let Some(select) = self.entry.as_mut() {
                if closing {
                    select.close();
                } else {
                    if let Input::Click { column, row } = input {
                        select.click(*column, *row);
                    } else {
                        select.handle(input);
                    }
                    return Step::Stay;
                }
            }
        }
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        let focused = self.prompt.focus.focused().map(str::to_owned);
        let on_button = matches!(focused.as_deref(), Some(CANCEL | OPEN));
        if navigate(&mut self.prompt.focus, input, on_button) {
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.prompt.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.prompt.focus.set(&id);
            return match id.as_str() {
                ENTRY => {
                    if let Some(select) = self.entry.as_mut() {
                        select.click(*column, *row);
                    }
                    Step::Stay
                }
                ARGUMENTS => {
                    self.arguments.click(*column);
                    Step::Stay
                }
                other => self.on_button(other).unwrap_or(Step::Stay),
            };
        }
        match focused.as_deref() {
            Some(ENTRY) => {
                if let Some(select) = self.entry.as_mut() {
                    select.handle(input);
                }
                Step::Stay
            }
            Some(ARGUMENTS) => match self.arguments.handle(input) {
                Some(WidgetEvent::Submitted(_)) => self.open(),
                _ => Step::Stay,
            },
            Some(id) if input.is_activation() => self.on_button(id).unwrap_or(Step::Stay),
            _ => Step::Stay,
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

    fn entries() -> Vec<String> {
        vec!["main".into(), "terminal".into()]
    }

    #[test]
    fn aspecto_sin_entradas() {
        let harness = Harness::new();
        let mut dialog = OpenApplicationDialog::new("main/tool", vec![]);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn aspecto_con_entradas_y_lista_abierta() {
        let mut harness = Harness::new();
        let mut dialog = OpenApplicationDialog::new("main/tool", entries());
        harness.draw(&mut dialog, 80, 24);
        harness.focus_on(&mut dialog, ENTRY);
        harness.press(&mut dialog, KeyCode::Enter);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn el_foco_empieza_en_los_argumentos_y_sin_selector_de_proceso() {
        let harness = Harness::spanish();
        let mut dialog = OpenApplicationDialog::new("main/tool", vec![]);
        let text = harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(ARGUMENTS));
        assert!(
            !text.contains("detached") && !text.contains("Proceso"),
            "{text}"
        );
    }

    #[test]
    fn intro_en_los_argumentos_abre_con_la_linea_partida() {
        let mut harness = Harness::new();
        let mut dialog = OpenApplicationDialog::new("main/tool", vec![]);
        harness.draw(&mut dialog, 80, 24);
        harness.type_text(&mut dialog, "run \"two words\" --x");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Open {
                arguments: vec!["run".into(), "two words".into(), "--x".into()],
                entry: None
            })
        );
    }

    #[test]
    fn el_boton_abrir_devuelve_la_entrada_elegida() {
        let mut harness = Harness::new();
        let mut dialog = OpenApplicationDialog::new("main/tool", entries());
        harness.draw(&mut dialog, 80, 24);
        harness.focus_on(&mut dialog, ENTRY);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, OPEN),
            Step::Close(Outcome::Open {
                arguments: vec![],
                entry: Some("terminal".into())
            })
        );
    }

    #[test]
    fn cancelar_y_esc_cierran_sin_abrir_y_esc_cierra_antes_la_lista() {
        let mut harness = Harness::new();
        let mut dialog = OpenApplicationDialog::new("main/tool", entries());
        harness.draw(&mut dialog, 80, 24);
        harness.focus_on(&mut dialog, ENTRY);
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(harness.press(&mut dialog, KeyCode::Esc), Step::Stay);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
        let mut dialog = OpenApplicationDialog::new("main/tool", entries());
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, CANCEL),
            Step::Close(Outcome::Closed)
        );
    }
}
