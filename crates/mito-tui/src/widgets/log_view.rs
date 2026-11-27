//! Registro de la sesión con ajuste de línea y desplazamiento automático.

use std::cell::Cell;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::text;
use crate::input::Input;
use crate::theme;

/// Registro (el `RichLog` del original): entradas de texto plano, las de error en rojo
/// y negrita. Sigue el final mientras no se desplace hacia arriba.
#[derive(Debug, Clone)]
pub struct LogView {
    /// Id lógico.
    pub id: WidgetId,
    /// Entradas `(texto, es_error)`.
    pub entries: Vec<(String, bool)>,
    /// Primera línea visual visible cuando no se sigue el final.
    pub scroll: usize,
    /// ¿Sigue el final del registro?
    pub follow: bool,
    /// Estilo base.
    pub style: Style,
    view: Cell<(usize, usize)>,
    area: Cell<Rect>,
}

impl LogView {
    /// Registro vacío que sigue el final.
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id: id.into(),
            entries: Vec::new(),
            scroll: 0,
            follow: true,
            style: theme::screen(),
            view: Cell::new((0, 0)),
            area: Cell::new(Rect::ZERO),
        }
    }

    /// Añade una entrada de texto plano (sin markup).
    pub fn push(&mut self, text: impl Into<String>, error: bool) {
        self.entries.push((text.into(), error));
    }

    /// Texto de todas las entradas, una por línea (lo que copia «Copiar log»).
    pub fn plain(&self) -> String {
        self.entries
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn visual(&self, width: usize) -> Vec<(String, bool)> {
        self.entries
            .iter()
            .flat_map(|(entry, error)| {
                text::wrap(entry, width)
                    .into_iter()
                    .map(move |line| (line, *error))
            })
            .collect()
    }

    fn scroll_by(&mut self, delta: isize) {
        let (total, height) = self.view.get();
        let max = total.saturating_sub(height);
        let current = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };
        let next = current.saturating_add_signed(delta).min(max);
        self.scroll = next;
        self.follow = next >= max;
    }

    /// Procesa una entrada cuando el registro tiene el foco (la rueda, con el puntero
    /// encima): ↑↓, RePág/AvPág, Inicio/Fin.
    pub fn handle(&mut self, input: &Input) {
        let page = isize::try_from(self.view.get().1.max(1)).unwrap_or(isize::MAX);
        match input {
            Input::Scroll { column, row, down } => {
                if self.area.get().contains((*column, *row).into()) {
                    self.scroll_by(if *down { 3 } else { -3 });
                }
            }
            _ if input.is(KeyCode::Up) => self.scroll_by(-1),
            _ if input.is(KeyCode::Down) => self.scroll_by(1),
            _ if input.is(KeyCode::PageUp) => self.scroll_by(-page),
            _ if input.is(KeyCode::PageDown) => self.scroll_by(page),
            _ if input.is(KeyCode::Home) => {
                self.scroll = 0;
                self.follow = false;
            }
            _ if input.is(KeyCode::End) => self.follow = true,
            _ => {}
        }
    }

    /// Dibuja las líneas visibles en `area` y registra la zona.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        self.area.set(area);
        let height = usize::from(area.height);
        let lines = self.visual(usize::from(area.width));
        let max = lines.len().saturating_sub(height);
        self.view.set((lines.len(), height));
        let first = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };
        frame.buffer_mut().set_style(area, self.style);
        let visible: Vec<Line> = lines
            .into_iter()
            .skip(first)
            .take(height)
            .map(|(line, error)| {
                if error {
                    Line::styled(
                        line,
                        Style::new()
                            .fg(theme::ERROR_TEXT)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Line::from(line)
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(visible).style(self.style), area);
        ui.focusable(&self.id, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    fn draw(log: &LogView, height: u16) -> ratatui::buffer::Buffer {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        testing::render_buffer(10, height, |frame, area| {
            let mut ui = focus.frame(&tr);
            log.render(frame, area, &mut ui);
        })
    }

    #[test]
    fn sigue_el_final_y_ajusta_las_lineas() {
        let mut log = LogView::new("log");
        for i in 0..4 {
            log.push(format!("mensaje {i}"), false);
        }
        log.push("uno dos tres cuatro", false);
        let text = testing::buffer_text(&draw(&log, 3));
        assert_eq!(text, "uno dos\ntres\ncuatro");
    }

    #[test]
    fn subir_deja_de_seguir_y_fin_vuelve() {
        let mut log = LogView::new("log");
        for i in 0..6 {
            log.push(format!("m{i}"), false);
        }
        draw(&log, 3);
        log.handle(&Input::key(KeyCode::Up));
        assert!(!log.follow);
        assert_eq!(testing::buffer_text(&draw(&log, 3)), "m2\nm3\nm4");
        log.handle(&Input::key(KeyCode::Down));
        assert!(log.follow, "al llegar abajo vuelve a seguir");
        log.handle(&Input::key(KeyCode::Home));
        assert_eq!(testing::buffer_text(&draw(&log, 3)), "m0\nm1\nm2");
        log.handle(&Input::key(KeyCode::End));
        assert_eq!(testing::buffer_text(&draw(&log, 3)), "m3\nm4\nm5");
    }

    #[test]
    fn los_errores_van_en_rojo_y_negrita() {
        let mut log = LogView::new("log");
        log.push("bien", false);
        log.push("mal", true);
        let buffer = draw(&log, 2);
        assert_ne!(buffer[(0, 0)].fg, theme::ERROR_TEXT);
        assert_eq!(buffer[(0, 1)].fg, theme::ERROR_TEXT);
        assert!(buffer[(0, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(log.plain(), "bien\nmal");
    }
}
