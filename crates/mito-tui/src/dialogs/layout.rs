//! Composición vertical de los diálogos: bloques de texto, huecos y casillas con nombre.
//!
//! El mismo contenido sirve para medir (la altura que pide un diálogo con altura
//! automática) y para dibujar.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::widgets::text;

/// Un bloque de la columna.
#[derive(Debug, Clone)]
enum Item {
    Text(String, Style),
    Gap(u16),
    Slot(&'static str, u16),
}

/// Columna de bloques de un diálogo.
#[derive(Debug, Clone, Default)]
pub struct Content {
    items: Vec<Item>,
}

/// Áreas con nombre que dejó [`Content::render`] para colocar widgets.
#[derive(Debug, Clone, Default)]
pub struct Slots {
    entries: Vec<(&'static str, Rect)>,
}

impl Slots {
    /// Área de la casilla `name`; vacía si no existe.
    pub fn get(&self, name: &str) -> Rect {
        self.entries
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(Rect::ZERO, |(_, rect)| *rect)
    }
}

impl Content {
    /// Columna vacía.
    pub fn new() -> Self {
        Self::default()
    }

    /// Texto ajustado al ancho.
    pub fn text(&mut self, content: impl Into<String>, style: Style) -> &mut Self {
        self.items.push(Item::Text(content.into(), style));
        self
    }

    /// Filas en blanco.
    pub fn gap(&mut self, rows: u16) -> &mut Self {
        self.items.push(Item::Gap(rows));
        self
    }

    /// Reserva `height` filas con el nombre `name`.
    pub fn slot(&mut self, name: &'static str, height: u16) -> &mut Self {
        self.items.push(Item::Slot(name, height));
        self
    }

    /// Filas que ocupa la columna con `width` celdas de ancho.
    pub fn height(&self, width: u16) -> u16 {
        self.items
            .iter()
            .map(|item| match item {
                Item::Text(content, _) => text::height(content, usize::from(width)),
                Item::Gap(rows) | Item::Slot(_, rows) => *rows,
            })
            .fold(0u16, u16::saturating_add)
    }

    /// Dibuja los textos en `area` y devuelve las áreas de las casillas.
    pub fn render(&self, frame: &mut Frame, area: Rect) -> Slots {
        let mut slots = Slots::default();
        let mut y = area.y;
        for item in &self.items {
            let remaining = area.bottom().saturating_sub(y);
            match item {
                Item::Text(content, style) => {
                    let lines = text::wrap(content, usize::from(area.width));
                    let height = u16::try_from(lines.len())
                        .unwrap_or(u16::MAX)
                        .min(remaining);
                    let rect = Rect::new(area.x, y, area.width, height);
                    let shown: Vec<Line> = lines
                        .into_iter()
                        .take(usize::from(height))
                        .map(Line::from)
                        .collect();
                    frame.render_widget(Paragraph::new(shown).style(*style), rect);
                    y += height;
                }
                Item::Gap(rows) => y += (*rows).min(remaining),
                Item::Slot(name, rows) => {
                    let height = (*rows).min(remaining);
                    slots
                        .entries
                        .push((name, Rect::new(area.x, y, area.width, height)));
                    y += height;
                }
            }
        }
        slots
    }
}

/// Reparte `area` en `count` columnas iguales separadas por `gap` celdas.
pub fn columns(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    Layout::horizontal(vec![Constraint::Fill(1); count])
        .spacing(gap)
        .split(area)
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::testing;

    #[test]
    fn mide_y_dibuja_lo_mismo() {
        let mut content = Content::new();
        content
            .text("uno dos tres", Style::new())
            .gap(1)
            .text("fin", Style::new())
            .slot("botones", 1);
        assert_eq!(content.height(8), 2 + 1 + 1 + 1);
        let mut found = Rect::ZERO;
        let text = testing::render(8, 6, |frame, area| {
            let slots = content.render(frame, area);
            found = slots.get("botones");
            assert_eq!(slots.get("nada"), Rect::ZERO);
        });
        assert_eq!(text, "uno dos\ntres\n\nfin");
        assert_eq!(found, Rect::new(0, 4, 8, 1));
    }

    #[test]
    fn las_columnas_se_reparten_por_igual() {
        let cols = columns(Rect::new(0, 0, 21, 1), 2, 1);
        assert_eq!(cols, vec![Rect::new(0, 0, 10, 1), Rect::new(11, 0, 10, 1)]);
        assert_eq!(columns(Rect::ZERO, 0, 1), Vec::<Rect>::new());
    }
}
