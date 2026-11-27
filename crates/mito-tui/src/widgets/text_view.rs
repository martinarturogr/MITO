//! Texto de solo lectura con desplazamiento.

use std::cell::Cell;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::text;
use crate::input::Input;
use crate::theme;

/// Texto de solo lectura (manifiesto, detalle, descargas) con desplazamiento vertical
/// y, sin ajuste de línea, horizontal.
///
/// Con `wrap` las líneas se ajustan al ancho usando su texto plano y el estilo de la
/// línea; sin `wrap` se conservan los estilos de cada tramo.
#[derive(Debug, Clone)]
pub struct TextView {
    /// Id lógico.
    pub id: WidgetId,
    /// Líneas del texto.
    pub lines: Vec<Line<'static>>,
    /// Primera línea visible.
    pub scroll: usize,
    /// Primera columna visible (solo sin ajuste de línea).
    pub hscroll: usize,
    /// ¿Se ajustan las líneas largas al ancho?
    pub wrap: bool,
    /// ¿Entra en el orden de Tab?
    pub focusable: bool,
    /// Estilo base.
    pub style: Style,
    view: Cell<(usize, usize, usize, usize)>,
    area: Cell<Rect>,
}

impl TextView {
    /// Vista vacía, sin ajuste de línea y enfocable.
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id: id.into(),
            lines: Vec::new(),
            scroll: 0,
            hscroll: 0,
            wrap: false,
            focusable: true,
            style: theme::details(),
            view: Cell::new((0, 0, 0, 0)),
            area: Cell::new(Rect::ZERO),
        }
    }

    /// Vista con el texto de `content`, una línea por salto de línea.
    pub fn from_text(id: impl Into<WidgetId>, content: &str) -> Self {
        let mut view = Self::new(id);
        view.set_text(content);
        view
    }

    /// Activa o desactiva el ajuste de línea.
    #[must_use]
    pub fn wrapped(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Define si la vista entra en el orden de Tab.
    #[must_use]
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Sustituye el texto y vuelve al principio.
    pub fn set_text(&mut self, content: &str) {
        self.lines = content
            .split('\n')
            .map(|line| Line::from(line.trim_end_matches('\r').to_owned()))
            .collect();
        self.scroll_home();
    }

    /// Vuelve al principio (arriba a la izquierda).
    pub fn scroll_home(&mut self) {
        self.scroll = 0;
        self.hscroll = 0;
    }

    /// Texto plano de las líneas, unidas por salto de línea.
    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn visual_lines(&self, width: usize) -> Vec<Line<'static>> {
        if !self.wrap {
            return self.lines.clone();
        }
        let mut out = Vec::new();
        for line in &self.lines {
            let style = line
                .spans
                .first()
                .map_or(line.style, |span| line.style.patch(span.style));
            for part in text::wrap(&line.to_string(), width) {
                out.push(Line::styled(part, style));
            }
        }
        out
    }

    fn max_scroll(&self) -> usize {
        let (_, total, height, _) = self.view.get();
        total.saturating_sub(height)
    }

    fn max_hscroll(&self) -> usize {
        let (_, _, _, widest_minus_width) = self.view.get();
        widest_minus_width
    }

    /// Desplaza `delta` líneas (negativo, hacia arriba).
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.max_scroll();
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    /// Procesa una entrada cuando la vista tiene el foco (la rueda, con el puntero
    /// encima aunque no tenga el foco): ↑↓, RePág/AvPág, Inicio/Fin y, sin ajuste de
    /// línea, ←→.
    pub fn handle(&mut self, input: &Input) {
        let page = isize::try_from(self.view.get().2.max(1)).unwrap_or(isize::MAX);
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
            _ if input.is(KeyCode::Home) => self.scroll = 0,
            _ if input.is(KeyCode::End) => self.scroll = self.max_scroll(),
            _ if input.is(KeyCode::Left) && !self.wrap => {
                self.hscroll = self.hscroll.saturating_sub(4);
            }
            _ if input.is(KeyCode::Right) && !self.wrap => {
                self.hscroll = (self.hscroll + 4).min(self.max_hscroll());
            }
            _ => {}
        }
    }

    /// Dibuja el texto visible en `area` y, si es enfocable, registra su zona.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        self.area.set(area);
        let lines = self.visual_lines(usize::from(area.width));
        let widest = lines.iter().map(Line::width).max().unwrap_or(0);
        let height = usize::from(area.height);
        let max_scroll = lines.len().saturating_sub(height);
        self.view.set((
            self.scroll.min(max_scroll),
            lines.len(),
            height,
            if self.wrap {
                0
            } else {
                widest.saturating_sub(usize::from(area.width))
            },
        ));
        let scroll = self.scroll.min(max_scroll);
        let hscroll = if self.wrap {
            0
        } else {
            self.hscroll.min(self.max_hscroll())
        };
        frame.buffer_mut().set_style(area, self.style);
        let visible: Vec<Line> = lines.into_iter().skip(scroll).take(height).collect();
        frame.render_widget(
            Paragraph::new(visible)
                .style(self.style)
                .scroll((0, u16::try_from(hscroll).unwrap_or(u16::MAX))),
            area,
        );
        if self.focusable {
            ui.focusable(&self.id, area);
        } else {
            ui.zone(&self.id, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    fn draw(view: &TextView, width: u16, height: u16) -> String {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        testing::render(width, height, |frame, area| {
            let mut ui = focus.frame(&tr);
            view.render(frame, area, &mut ui);
        })
    }

    #[test]
    fn dibuja_y_desplaza_verticalmente() {
        let content = (0..10)
            .map(|i| format!("línea {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut view = TextView::from_text("t", &content);
        assert_eq!(draw(&view, 12, 3), "línea 0\nlínea 1\nlínea 2");
        view.handle(&Input::key(KeyCode::Down));
        assert_eq!(draw(&view, 12, 3), "línea 1\nlínea 2\nlínea 3");
        view.handle(&Input::key(KeyCode::End));
        assert_eq!(draw(&view, 12, 3), "línea 7\nlínea 8\nlínea 9");
        view.handle(&Input::key(KeyCode::PageUp));
        assert_eq!(view.scroll, 4);
        view.handle(&Input::key(KeyCode::Home));
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn sin_ajuste_se_desplaza_en_horizontal() {
        let mut view = TextView::from_text("t", "0123456789ABCDEF");
        assert_eq!(draw(&view, 8, 1), "01234567");
        view.handle(&Input::key(KeyCode::Right));
        assert_eq!(draw(&view, 8, 1), "456789AB");
        for _ in 0..5 {
            view.handle(&Input::key(KeyCode::Right));
        }
        assert_eq!(draw(&view, 8, 1), "89ABCDEF");
        view.scroll_home();
        assert_eq!(draw(&view, 8, 1), "01234567");
    }

    #[test]
    fn con_ajuste_parte_las_lineas_largas() {
        let view = TextView::from_text("t", "uno dos tres cuatro").wrapped(true);
        assert_eq!(draw(&view, 9, 4), "uno dos\ntres\ncuatro");
    }

    #[test]
    fn la_rueda_solo_actua_con_el_puntero_encima() {
        let content = (0..10)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut view = TextView::from_text("t", &content);
        draw(&view, 5, 3);
        view.handle(&Input::Scroll {
            column: 40,
            row: 40,
            down: true,
        });
        assert_eq!(view.scroll, 0);
        view.handle(&Input::Scroll {
            column: 1,
            row: 1,
            down: true,
        });
        assert_eq!(view.scroll, 3);
    }

    #[test]
    fn no_enfocable_no_entra_en_el_orden_de_tab() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let view = TextView::from_text("t", "x").focusable(false);
        testing::render(5, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            view.render(frame, area, &mut ui);
        });
        assert!(!focus.is_registered("t"));
        assert_eq!(focus.hit(0, 0).as_deref(), Some("t"));
    }
}
