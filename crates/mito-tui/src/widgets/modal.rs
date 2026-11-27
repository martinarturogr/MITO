//! Panel modal centrado sobre un fondo oscurecido.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear};

use crate::theme;

/// Dimensión de un modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// Celdas exactas (se recorta al tamaño de la pantalla).
    Cells(u16),
    /// Porcentaje de la pantalla.
    Percent(u16),
    /// Porcentaje de la pantalla con un máximo en celdas (`95 %, máx. 136`).
    PercentMax(u16, u16),
    /// Lo que pide el contenido (celdas), con un máximo del 90 % de la pantalla.
    Auto(u16),
}

impl Size {
    /// Celdas que ocupa en una pantalla con `available` celdas en esa dimensión.
    pub fn resolve(self, available: u16) -> u16 {
        let percent = |p: u16| {
            u16::try_from(u32::from(available) * u32::from(p.min(100)) / 100).unwrap_or(available)
        };
        let size = match self {
            Self::Cells(n) => n,
            Self::Percent(p) => percent(p),
            Self::PercentMax(p, max) => percent(p).min(max),
            Self::Auto(n) => n.min(percent(90)),
        };
        size.min(available)
    }
}

/// Área de un modal de `width` por `height` centrado en `screen`.
pub fn area(screen: Rect, width: Size, height: Size) -> Rect {
    let width = width.resolve(screen.width);
    let height = height.resolve(screen.height);
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}

/// Oscurece todo lo dibujado hasta ahora (la superposición `#00000088` del original).
pub fn dim_background(frame: &mut Frame) {
    let buffer = frame.buffer_mut();
    let screen = buffer.area;
    for y in screen.top()..screen.bottom() {
        for x in screen.left()..screen.right() {
            let cell = &mut buffer[(x, y)];
            if let Color::Rgb(..) = cell.fg {
                cell.fg = theme::darken(cell.fg, 55);
            } else if cell.fg == Color::Reset {
                cell.fg = theme::darken(theme::TEXT, 55);
            }
            if let Color::Rgb(..) = cell.bg {
                cell.bg = theme::darken(cell.bg, 55);
            } else if cell.bg == Color::Reset {
                cell.bg = theme::darken(theme::SCREEN, 55);
            }
        }
    }
}

/// Oscurece el fondo, limpia `area` y dibuja el panel `#153429` con borde redondeado
/// del color `border`. Devuelve el área interior para el contenido: sin el borde y con
/// una columna de relleno a cada lado.
pub fn frame(frame: &mut Frame, area: Rect, border: Color) -> Rect {
    dim_background(frame);
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border).bg(theme::SIDEBAR))
        .style(theme::panel());
    frame.render_widget(block, area);
    Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::testing;

    #[test]
    fn el_modal_se_centra_y_se_recorta_a_la_pantalla() {
        let screen = Rect::new(0, 0, 100, 30);
        assert_eq!(
            area(screen, Size::Cells(60), Size::Cells(10)),
            Rect::new(20, 10, 60, 10)
        );
        assert_eq!(
            area(screen, Size::Percent(90), Size::Percent(90)),
            Rect::new(5, 1, 90, 27)
        );
        assert_eq!(
            area(screen, Size::PercentMax(95, 80), Size::Auto(40)),
            Rect::new(10, 1, 80, 27)
        );
        assert_eq!(
            area(Rect::new(0, 0, 40, 10), Size::Cells(78), Size::Auto(5)),
            Rect::new(0, 2, 40, 5)
        );
    }

    #[test]
    fn el_panel_oscurece_el_fondo_y_dibuja_el_borde() {
        let buffer = testing::render_buffer(12, 5, |frame, area| {
            frame.buffer_mut().set_style(area, theme::screen());
            let inner = super::frame(frame, Rect::new(2, 1, 8, 3), theme::BORDER);
            assert_eq!(inner, Rect::new(4, 2, 4, 1));
        });
        assert_eq!(
            testing::buffer_text(&buffer),
            "\n  ╭──────╮\n  │      │\n  ╰──────╯"
        );
        assert_eq!(buffer[(0, 0)].bg, theme::darken(theme::SCREEN, 55));
        assert_eq!(buffer[(3, 2)].bg, theme::SIDEBAR);
        assert_eq!(buffer[(2, 1)].fg, theme::BORDER);
    }
}
