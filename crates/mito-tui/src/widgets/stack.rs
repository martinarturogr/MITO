//! Apilado vertical de bloques dentro de un área (el `Vertical` de Textual).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::text;

/// Reparte un área de arriba abajo: cada bloque ocupa las filas que pide.
#[derive(Debug, Clone, Copy)]
pub struct Stack {
    area: Rect,
    next: u16,
}

impl Stack {
    /// Apilado que empieza en la primera fila de `area`.
    pub fn new(area: Rect) -> Self {
        Self { area, next: area.y }
    }

    /// Filas libres que quedan.
    pub fn remaining_height(&self) -> u16 {
        self.area.bottom().saturating_sub(self.next)
    }

    /// Área libre que queda (de la siguiente fila al final).
    pub fn remaining(&self) -> Rect {
        Rect::new(
            self.area.x,
            self.next.min(self.area.bottom()),
            self.area.width,
            self.remaining_height(),
        )
    }

    /// Reserva `height` filas (menos si no quedan) y devuelve su área.
    pub fn take(&mut self, height: u16) -> Rect {
        let height = height.min(self.remaining_height());
        let rect = Rect::new(self.area.x, self.next, self.area.width, height);
        self.next += height;
        rect
    }

    /// Salta `rows` filas.
    pub fn gap(&mut self, rows: u16) {
        self.take(rows);
    }

    /// Dibuja `content` ajustado al ancho con `style` y devuelve el área que ocupa.
    pub fn text(&mut self, frame: &mut Frame, content: &str, style: Style) -> Rect {
        let lines = text::wrap(content, usize::from(self.area.width));
        let rect = self.take(u16::try_from(lines.len()).unwrap_or(u16::MAX));
        let visible: Vec<Line> = lines
            .into_iter()
            .take(usize::from(rect.height))
            .map(Line::from)
            .collect();
        frame.render_widget(Paragraph::new(visible).style(style), rect);
        rect
    }

    /// Como [`text`](Self::text) pero centrado.
    pub fn centered(&mut self, frame: &mut Frame, content: &str, style: Style) -> Rect {
        let lines = text::wrap(content, usize::from(self.area.width));
        let rect = self.take(u16::try_from(lines.len()).unwrap_or(u16::MAX));
        let visible: Vec<Line> = lines
            .into_iter()
            .take(usize::from(rect.height))
            .map(Line::from)
            .collect();
        frame.render_widget(Paragraph::new(visible).style(style).centered(), rect);
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::testing;

    #[test]
    fn reparte_las_filas_y_se_detiene_al_llegar_al_final() {
        let mut stack = Stack::new(Rect::new(2, 1, 10, 5));
        assert_eq!(stack.take(2), Rect::new(2, 1, 10, 2));
        stack.gap(1);
        assert_eq!(stack.remaining(), Rect::new(2, 4, 10, 2));
        assert_eq!(stack.take(9), Rect::new(2, 4, 10, 2));
        assert_eq!(stack.remaining_height(), 0);
        assert!(stack.take(1).is_empty());
    }

    #[test]
    fn el_texto_se_ajusta_y_reserva_sus_filas() {
        let text = testing::render(10, 4, |frame, area| {
            let mut stack = Stack::new(area);
            let used = stack.text(frame, "uno dos tres", Style::new());
            assert_eq!(used.height, 2);
            stack.centered(frame, "fin", Style::new());
        });
        assert_eq!(text, "uno dos\ntres\n    fin");
    }
}
