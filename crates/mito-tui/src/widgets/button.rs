//! Botón con variantes de color del tema.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::{WidgetEvent, text};
use crate::input::Input;
use crate::theme;

/// Variante de color de un botón (las de Textual).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    /// Sin énfasis.
    #[default]
    Default,
    /// Acción principal.
    Primary,
    /// Acción afirmativa.
    Success,
    /// Acción que pide cuidado.
    Warning,
    /// Acción destructiva.
    Error,
}

impl Variant {
    /// Fondo del botón.
    pub fn background(self) -> Color {
        match self {
            Self::Default => theme::BUTTON_DEFAULT,
            Self::Primary => theme::BUTTON_PRIMARY,
            Self::Success => theme::BUTTON_SUCCESS,
            Self::Warning => theme::BUTTON_WARNING,
            Self::Error => theme::BUTTON_ERROR,
        }
    }

    /// Color del texto sobre el fondo.
    pub fn foreground(self) -> Color {
        match self {
            Self::Success | Self::Warning => theme::SCREEN,
            _ => Color::White,
        }
    }
}

/// Botón: se activa con Intro, Espacio o un clic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// Id lógico (el del original: `"install"`, `"remove-3"`…).
    pub id: WidgetId,
    /// Texto, ya traducido.
    pub label: String,
    /// Variante de color.
    pub variant: Variant,
    /// Un botón deshabilitado no se registra: no recibe foco ni clics.
    pub disabled: bool,
}

impl Button {
    /// Botón habilitado de la variante por defecto.
    pub fn new(id: impl Into<WidgetId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: Variant::Default,
            disabled: false,
        }
    }

    /// Cambia la variante.
    #[must_use]
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Cambia si está deshabilitado.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dibuja el botón en `area` (centrado en vertical; con `[ … ]` si cabe) y, si está
    /// habilitado, registra su zona.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        let focused = !self.disabled && ui.is_focused(&self.id);
        let mut style = if self.disabled {
            Style::new().fg(theme::DIM).bg(theme::INPUT_BG)
        } else {
            Style::new()
                .fg(self.variant.foreground())
                .bg(self.variant.background())
        };
        if focused {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        let width = usize::from(area.width);
        let label = if text::width(&self.label) + 4 <= width {
            format!("[ {} ]", self.label)
        } else {
            text::truncate(&self.label, width)
        };
        frame.buffer_mut().set_style(area, style);
        let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        frame.render_widget(
            Paragraph::new(Span::raw(label)).centered().style(style),
            row,
        );
        if !self.disabled {
            ui.focusable(&self.id, area);
        }
    }

    /// Intro o Espacio activan el botón.
    pub fn handle(&mut self, input: &Input) -> Option<WidgetEvent> {
        if self.disabled || !input.is_activation() {
            return None;
        }
        self.click()
    }

    /// Un clic activa el botón.
    pub fn click(&mut self) -> Option<WidgetEvent> {
        (!self.disabled).then(|| WidgetEvent::Activated(self.id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;
    use ratatui::style::Modifier;

    #[test]
    fn dibuja_la_etiqueta_centrada_y_registra_la_zona() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let button = Button::new("ok", "Aceptar").variant(Variant::Primary);
        let text = testing::render(20, 3, |frame, area| {
            let mut ui = focus.frame(&tr);
            button.render(frame, Rect::new(area.x, area.y, 20, 3), &mut ui);
        });
        assert_eq!(text, "\n     [ Aceptar ]");
        assert_eq!(focus.hit(1, 1).as_deref(), Some("ok"));
    }

    #[test]
    fn el_boton_deshabilitado_no_se_registra() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let button = Button::new("ok", "Aceptar").disabled(true);
        testing::render(20, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            button.render(frame, area, &mut ui);
        });
        assert!(!focus.is_registered("ok"));
        assert_eq!(focus.hit(1, 0), None);
    }

    #[test]
    fn con_foco_se_dibuja_invertido() {
        let tr = Translator::new("english");
        let mut focus = Focus::starting_at("ok");
        let button = Button::new("ok", "Aceptar");
        let buffer = testing::render_buffer(20, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            button.render(frame, area, &mut ui);
        });
        assert!(buffer[(1, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn intro_espacio_y_clic_activan() {
        let mut button = Button::new("ok", "Aceptar");
        let activated = Some(WidgetEvent::Activated("ok".into()));
        assert_eq!(
            button.handle(&Input::key(crossterm::event::KeyCode::Enter)),
            activated
        );
        assert_eq!(button.handle(&Input::character(' ')), activated);
        assert_eq!(button.handle(&Input::character('x')), None);
        assert_eq!(button.click(), activated);
        button.disabled = true;
        assert_eq!(button.click(), None);
        assert_eq!(
            button.handle(&Input::key(crossterm::event::KeyCode::Enter)),
            None
        );
    }

    #[test]
    fn una_etiqueta_larga_se_recorta() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let button = Button::new("ok", "Cancelar instalación");
        let text = testing::render(10, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            button.render(frame, area, &mut ui);
        });
        assert_eq!(text, "Cancelar …");
    }
}
