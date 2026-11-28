//! Elegir una aplicación del host entre las encontradas (`HostDiscoverDialog`).

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::Paragraph;

use super::layout::columns;
use super::{Context, Dialog, Outcome, Step, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::text;
use crate::widgets::{
    Button, Column, Focus, Stack, Table, TableRow, TextInput, Ui, Variant, WidgetEvent, header,
};
use crate::worker::Pick;

const CLOSE: &str = "host-discover-close";
const FILTER: &str = "host-discover-filter";
const LIST: &str = "host-discover-list";
const BROWSE: &str = "host-discover-browse";
const CANCEL: &str = "host-discover-cancel";
const ADD: &str = "host-discover-add";
/// Filas de la lista.
const LIST_ROWS: u16 = 16;

/// «AGREGAR APLICACIÓN DEL HOST»: filtro, lista de aplicaciones encontradas y los botones
/// «Examinar…», «Cancelar» y «Agregar». Se cierra con [`Outcome::HostApplication`]:
/// la ruta elegida, [`Pick::Browse`] o [`Pick::Cancel`] (Esc, × y «Cancelar»).
#[derive(Debug)]
pub struct HostDiscoverDialog {
    focus: Focus,
    found: Vec<(String, PathBuf)>,
    shown: Vec<usize>,
    filter: TextInput,
    list: Table,
}

impl HostDiscoverDialog {
    /// Diálogo con las aplicaciones encontradas (etiqueta y ruta).
    pub fn new(found: Vec<(String, PathBuf)>) -> Self {
        let mut list = Table::new(LIST, vec![Column::new("", Constraint::Fill(1))]);
        list.header = false;
        let mut dialog = Self {
            focus: Focus::starting_at(FILTER),
            found,
            shown: Vec::new(),
            filter: TextInput::new(FILTER),
            list,
        };
        dialog.refresh();
        dialog
    }

    /// Vuelve a filtrar: una opción aparece si todas las palabras (sin mayúsculas) están
    /// en `{etiqueta} {ruta}`.
    fn refresh(&mut self) {
        let words: Vec<String> = self
            .filter
            .value
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        self.shown = self
            .found
            .iter()
            .enumerate()
            .filter(|(_, (label, path))| {
                let haystack = format!("{label} {}", path.display()).to_lowercase();
                words.iter().all(|word| haystack.contains(word))
            })
            .map(|(index, _)| index)
            .collect();
        let rows = self
            .shown
            .iter()
            .map(|index| {
                let (label, path) = &self.found[*index];
                TableRow::new(
                    index.to_string(),
                    vec![format!("{label}  ·  {}", path.display())],
                )
            })
            .collect();
        self.list.set_rows(rows);
    }

    fn selected(&self) -> Option<PathBuf> {
        let key = self.list.highlighted()?;
        let index: usize = key.parse().ok()?;
        self.found.get(index).map(|(_, path)| path.clone())
    }

    fn pick(&self) -> Step {
        match self.selected() {
            Some(path) => Step::Close(Outcome::HostApplication(Pick::Path(path))),
            None => Step::Stay,
        }
    }

    fn act(&self, id: &str) -> Step {
        match id {
            CLOSE | CANCEL => Step::Close(Outcome::HostApplication(Pick::Cancel)),
            BROWSE => Step::Close(Outcome::HostApplication(Pick::Browse)),
            ADD | LIST => self.pick(),
            _ => Step::Stay,
        }
    }
}

impl Dialog for HostDiscoverDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let intro = ui.tr.text(
            "Aplicaciones encontradas en el menú Inicio y el escritorio del host. Si falta alguna, usa Examinar.",
        );
        let area = modal::area(
            screen,
            Size::PercentMax(90, 110),
            Size::Auto(LIST_ROWS + 12),
        );
        let inner = modal::frame(frame, area, theme::BORDER);
        let inner = Rect::new(
            inner.x + 2,
            inner.y + 1,
            inner.width.saturating_sub(4),
            inner.height.saturating_sub(2),
        );
        let mut stack = Stack::new(inner);
        header::render(
            frame,
            stack.take(header::HEIGHT),
            ui,
            "AGREGAR APLICACIÓN DEL HOST",
            CLOSE,
        );
        stack.text(frame, intro, theme::panel());
        stack.gap(1);
        self.filter.placeholder = ui.tr.text("Filtrar…").to_owned();
        self.filter.render(frame, stack.take(1), ui);
        stack.gap(1);
        // Lo que queda, menos los botones y su separación.
        let rows = stack.remaining_height().saturating_sub(2).min(LIST_ROWS);
        let list = stack.take(rows);
        if self.shown.is_empty() {
            let empty = Rect::new(list.x, list.y, list.width, 1);
            frame.render_widget(
                Paragraph::new(text::truncate(
                    ui.tr.text("No se ha encontrado ninguna aplicación."),
                    usize::from(list.width),
                ))
                .style(theme::panel()),
                empty,
            );
        } else {
            self.list.render(frame, list, ui);
        }
        stack.gap(1);
        let cells = columns(stack.take(1), 3, 1);
        Button::new(BROWSE, ui.tr.text("Examinar…")).render(frame, cells[0], ui);
        Button::new(CANCEL, ui.tr.text("Cancelar")).render(frame, cells[1], ui);
        Button::new(ADD, ui.tr.text("Agregar"))
            .variant(Variant::Success)
            .disabled(self.shown.is_empty())
            .render(frame, cells[2], ui);
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::HostApplication(Pick::Cancel));
        }
        let focused = self.focus.focused().map(str::to_owned);
        let on_button = matches!(focused.as_deref(), Some(BROWSE | CANCEL | ADD));
        if navigate(&mut self.focus, input, on_button) {
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.focus.set(&id);
            return match id.as_str() {
                FILTER => {
                    self.filter.click(*column);
                    Step::Stay
                }
                LIST => {
                    // Un clic sobre una opción la elige (`OptionSelected`).
                    let clicked = self
                        .focus
                        .area_of(LIST)
                        .map(|area| self.list.offset + usize::from(row.saturating_sub(area.y)))
                        .filter(|index| *index < self.list.rows.len());
                    match clicked {
                        Some(index) => {
                            self.list.cursor = index;
                            self.pick()
                        }
                        None => Step::Stay,
                    }
                }
                other => self.act(other),
            };
        }
        match focused.as_deref() {
            Some(FILTER) => {
                // ↑↓ mueven la lista sin salir del filtro.
                if input.is(KeyCode::Down) || input.is(KeyCode::Up) {
                    self.list.handle(input);
                    return Step::Stay;
                }
                match self.filter.handle(input) {
                    Some(WidgetEvent::Changed(_)) => {
                        self.refresh();
                        Step::Stay
                    }
                    Some(WidgetEvent::Submitted(_)) => self.pick(),
                    _ => Step::Stay,
                }
            }
            Some(LIST) => {
                if input.is_activation() {
                    return self.pick();
                }
                self.list.handle(input);
                Step::Stay
            }
            _ => match pressed(&mut self.focus, input) {
                Some(id) => self.act(&id),
                None => Step::Stay,
            },
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;

    fn found() -> Vec<(String, PathBuf)> {
        vec![
            ("Editor".to_owned(), PathBuf::from(r"C:\Tools\Editor.exe")),
            ("Viewer".to_owned(), PathBuf::from(r"C:\Tools\Viewer.exe")),
            ("Run".to_owned(), PathBuf::from(r"C:\Tools\Run.cmd")),
        ]
    }

    #[test]
    fn aspecto_con_hallazgos() {
        let harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn aspecto_sin_hallazgos_deshabilita_agregar() {
        let harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(vec![]);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("No se ha encontrado ninguna aplicación."),
            "{text}"
        );
        assert!(!dialog.focus().is_registered(ADD));
        insta::assert_snapshot!(text);
    }

    #[test]
    fn el_filtro_deja_un_resultado_y_agregar_devuelve_la_ruta() {
        let mut harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(dialog.focus().focused(), Some(FILTER));
        harness.type_text(&mut dialog, "view");
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("Viewer  ·  C:\\Tools\\Viewer.exe"), "{text}");
        assert!(!text.contains("Editor"), "{text}");
        assert_eq!(
            harness.click(&mut dialog, ADD),
            Step::Close(Outcome::HostApplication(Pick::Path(PathBuf::from(
                r"C:\Tools\Viewer.exe"
            ))))
        );
    }

    #[test]
    fn el_filtro_busca_en_etiqueta_y_ruta_con_todas_las_palabras() {
        let mut harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        harness.draw(&mut dialog, 100, 30);
        harness.type_text(&mut dialog, "tools .CMD");
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(dialog.shown, [2]);
        harness.type_text(&mut dialog, " nada");
        harness.draw(&mut dialog, 100, 30);
        assert!(dialog.shown.is_empty());
        assert_eq!(dialog.selected(), None);
    }

    #[test]
    fn intro_en_el_filtro_elige_la_resaltada_y_las_flechas_mueven_la_lista() {
        let mut harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        harness.draw(&mut dialog, 100, 30);
        harness.press(&mut dialog, KeyCode::Down);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::HostApplication(Pick::Path(PathBuf::from(
                r"C:\Tools\Viewer.exe"
            ))))
        );
    }

    #[test]
    fn examinar_cancelar_la_cruz_y_esc_devuelven_su_eleccion() {
        let mut harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, BROWSE),
            Step::Close(Outcome::HostApplication(Pick::Browse))
        );
        for id in [CANCEL, CLOSE] {
            assert_eq!(
                harness.click(&mut dialog, id),
                Step::Close(Outcome::HostApplication(Pick::Cancel))
            );
        }
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::HostApplication(Pick::Cancel))
        );
    }

    #[test]
    fn un_clic_en_una_opcion_la_elige() {
        let mut harness = Harness::spanish();
        let mut dialog = HostDiscoverDialog::new(found());
        harness.draw(&mut dialog, 100, 30);
        let area = dialog.focus().area_of(LIST).unwrap();
        let step = harness.send(
            &mut dialog,
            &Input::Click {
                column: area.x + 3,
                row: area.y + 2,
            },
        );
        assert_eq!(
            step,
            Step::Close(Outcome::HostApplication(Pick::Path(PathBuf::from(
                r"C:\Tools\Run.cmd"
            ))))
        );
    }
}
