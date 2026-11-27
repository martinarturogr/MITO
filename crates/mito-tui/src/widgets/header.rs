//! Cabecera de los diálogos: título centrado y botón de cierre `×`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use super::focus::Ui;
use super::text;
use crate::theme;

/// Filas que debe reservar un diálogo para la cabecera: el título y una de separación.
pub const HEIGHT: u16 = 2;

/// Ancho del botón de cierre.
const CLOSE_WIDTH: u16 = 3;

/// Dibuja la cabecera en la primera fila de `area`: relleno de 3, título centrado en
/// negrita `#b5dc64` y botón `×` rojo `#ff7070` con el id `close_id`, que se registra
/// como enfocable. `title` es una clave de i18n.
pub fn render(frame: &mut Frame, area: Rect, ui: &mut Ui, title: &str, close_id: &str) {
    if area.width < CLOSE_WIDTH * 2 + 1 || area.is_empty() {
        return;
    }
    let title = ui.tr.text(title);
    let middle = Rect::new(
        area.x + CLOSE_WIDTH,
        area.y,
        area.width - CLOSE_WIDTH * 2,
        1,
    );
    let shown = text::truncate(title, usize::from(middle.width));
    frame.render_widget(
        Paragraph::new(shown).centered().style(theme::title()),
        middle,
    );
    let close = Rect::new(area.right() - CLOSE_WIDTH, area.y, CLOSE_WIDTH, 1);
    let mut style = Style::new()
        .fg(theme::ERROR_TEXT)
        .bg(theme::SIDEBAR)
        .add_modifier(Modifier::BOLD);
    if ui.is_focused(close_id) {
        style = style.bg(theme::CLOSE_FOCUS);
    }
    frame.render_widget(Paragraph::new(" × ").style(style), close);
    ui.focusable(close_id, close);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    #[test]
    fn dibuja_titulo_centrado_y_cierre_registrado() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let buffer = testing::render_buffer(30, 2, |frame, area| {
            let mut ui = focus.frame(&tr);
            render(frame, area, &mut ui, "Manifiesto", "manifest-close");
        });
        assert_eq!(
            testing::buffer_text(&buffer),
            "           Manifest         ×"
        );
        assert_eq!(focus.hit(28, 0).as_deref(), Some("manifest-close"));
        assert_eq!(focus.hit(10, 0), None);
        assert_eq!(buffer[(28, 0)].fg, theme::ERROR_TEXT);
        assert_eq!(buffer[(11, 0)].fg, theme::TITLE);
    }

    #[test]
    fn con_foco_el_cierre_cambia_de_fondo() {
        let tr = Translator::new("english");
        let mut focus = Focus::starting_at("x");
        let buffer = testing::render_buffer(30, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            render(frame, area, &mut ui, "T", "x");
        });
        assert_eq!(buffer[(28, 0)].bg, theme::CLOSE_FOCUS);
    }

    #[test]
    fn el_titulo_se_traduce() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let text = testing::render(30, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            render(frame, area, &mut ui, "GESTIONAR PROFILE", "x");
        });
        assert!(text.contains("MANAGE PROFILE"), "{text}");
    }
}
