//! Casilla de verificación.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::{WidgetEvent, text};
use crate::input::Input;
use crate::theme;

/// Casilla: `[X]` en verde encendida y `[ ]` en rojo apagada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    /// Id lógico.
    pub id: WidgetId,
    /// Texto, ya traducido.
    pub label: String,
    /// Valor actual.
    pub value: bool,
    /// Una casilla deshabilitada no se registra: no recibe foco ni clics.
    pub disabled: bool,
    /// Estilo base (fondo) sobre el que se dibuja.
    pub style: Style,
}

impl Checkbox {
    /// Casilla habilitada sobre el fondo del panel.
    pub fn new(id: impl Into<WidgetId>, label: impl Into<String>, value: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            disabled: false,
            style: theme::panel(),
        }
    }

    /// Cambia si está deshabilitada.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Cambia el estilo base (por ejemplo el fondo de una fila alterna).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Dibuja la casilla en la primera fila de `area` y, si está habilitada, registra su
    /// zona (todo el ancho de `area`).
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        let focused = !self.disabled && ui.is_focused(&self.id);
        let mark_color = if self.value {
            theme::CHECK_ON
        } else {
            theme::CHECK_OFF
        };
        let mut label_style = self.style;
        if self.disabled {
            label_style = label_style.fg(theme::DIM);
        }
        if focused {
            label_style = label_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        let mark = if self.value { "[X]" } else { "[ ]" };
        let label = text::truncate(&self.label, usize::from(area.width).saturating_sub(4));
        let line = Line::from(vec![
            Span::styled(mark, self.style.fg(mark_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {label}"), label_style),
        ]);
        let row = Rect::new(area.x, area.y, area.width, 1);
        frame.buffer_mut().set_style(row, self.style);
        frame.render_widget(Paragraph::new(line), row);
        if !self.disabled {
            ui.focusable(&self.id, row);
        }
    }

    /// Intro o Espacio cambian el valor.
    pub fn handle(&mut self, input: &Input) -> Option<WidgetEvent> {
        if self.disabled || !input.is_activation() {
            return None;
        }
        self.click()
    }

    /// Un clic cambia el valor.
    pub fn click(&mut self) -> Option<WidgetEvent> {
        if self.disabled {
            return None;
        }
        self.value = !self.value;
        Some(WidgetEvent::Toggled(self.id.clone(), self.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use crossterm::event::KeyCode;
    use mito_i18n::Translator;

    #[test]
    fn dibuja_el_estado_y_los_colores() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let on = Checkbox::new("on", "Encendida", true);
        let off = Checkbox::new("off", "Apagada", false);
        let buffer = testing::render_buffer(20, 2, |frame, area| {
            let mut ui = focus.frame(&tr);
            on.render(frame, Rect::new(area.x, 0, 20, 1), &mut ui);
            off.render(frame, Rect::new(area.x, 1, 20, 1), &mut ui);
        });
        assert_eq!(testing::buffer_text(&buffer), "[X] Encendida\n[ ] Apagada");
        assert_eq!(buffer[(1, 0)].fg, theme::CHECK_ON);
        assert_eq!(buffer[(1, 1)].fg, theme::CHECK_OFF);
    }

    #[test]
    fn intro_espacio_y_clic_alternan() {
        let mut checkbox = Checkbox::new("c", "Casilla", false);
        assert_eq!(
            checkbox.handle(&Input::key(KeyCode::Enter)),
            Some(WidgetEvent::Toggled("c".into(), true))
        );
        assert_eq!(
            checkbox.handle(&Input::character(' ')),
            Some(WidgetEvent::Toggled("c".into(), false))
        );
        assert_eq!(checkbox.handle(&Input::character('a')), None);
        assert_eq!(
            checkbox.click(),
            Some(WidgetEvent::Toggled("c".into(), true))
        );
    }

    #[test]
    fn deshabilitada_no_cambia_ni_se_registra() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let mut checkbox = Checkbox::new("c", "Casilla", false).disabled(true);
        testing::render(20, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            checkbox.render(frame, area, &mut ui);
        });
        assert!(!focus.is_registered("c"));
        assert_eq!(checkbox.click(), None);
        assert!(!checkbox.value);
    }
}
