//! Base común de los diálogos «mensaje y botones»: panel, texto y fila de botones.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::layout::{self, Content, Slots};
use super::{navigate, pressed};
use crate::input::Input;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Button, Focus, Ui, Variant};

/// Botón de un diálogo: id, clave de traducción de la etiqueta y variante.
#[derive(Debug, Clone, Copy)]
pub struct PromptButton {
    pub id: &'static str,
    pub label: &'static str,
    pub variant: Variant,
}

/// Qué ocurrió con una entrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Se pulsó (o se hizo clic en) el botón con este id.
    Pressed(String),
    /// Se pulsó Esc.
    Escape,
}

impl Event {
    /// ¿Es una confirmación? Solo lo es pulsar el botón `confirm_id`; Esc o cualquier
    /// otro botón cancelan.
    pub fn confirms(&self, confirm_id: &str) -> bool {
        matches!(self, Self::Pressed(id) if id == confirm_id)
    }
}

/// Panel modal con contenido, y una fila de botones al final.
#[derive(Debug)]
pub struct Prompt {
    pub focus: Focus,
    buttons: Vec<PromptButton>,
    width: Size,
    border: Color,
}

impl Prompt {
    /// Diálogo con el foco inicial en el botón `initial`.
    pub fn new(buttons: Vec<PromptButton>, initial: &str, width: Size, border: Color) -> Self {
        Self {
            focus: Focus::starting_at(initial),
            buttons,
            width,
            border,
        }
    }

    /// Dibuja el panel con `content` y, debajo, la fila de botones. `fields` dibuja los
    /// widgets de las casillas con nombre **antes** que los botones (así van antes en el
    /// orden de Tab). Devuelve las casillas, por si hay que dibujar algo encima de todo
    /// (la lista de un selector).
    pub fn render(
        &mut self,
        frame: &mut Frame,
        screen: Rect,
        ui: &mut Ui,
        content: &mut Content,
        fields: impl FnOnce(&mut Frame, &Slots, &mut Ui),
    ) -> Slots {
        content.gap(1).slot("buttons", 1);
        let outer_width = self.width.resolve(screen.width);
        let inner_width = outer_width.saturating_sub(4);
        let height = content.height(inner_width).saturating_add(2);
        let area = modal::area(screen, self.width, Size::Auto(height));
        let inner = modal::frame(frame, area, self.border);
        let slots = content.render(frame, inner);
        fields(frame, &slots, ui);
        let cells = layout::columns(slots.get("buttons"), self.buttons.len(), 1);
        for (spec, cell) in self.buttons.iter().zip(cells) {
            Button::new(spec.id, ui.tr.text(spec.label))
                .variant(spec.variant)
                .render(frame, cell, ui);
        }
        slots
    }

    /// Navegación (Tab, ←→), activación y Esc.
    pub fn handle(&mut self, input: &Input) -> Option<Event> {
        if input.is(KeyCode::Esc) {
            return Some(Event::Escape);
        }
        if navigate(&mut self.focus, input, true) {
            return None;
        }
        let id = pressed(&mut self.focus, input)?;
        self.buttons
            .iter()
            .any(|button| button.id == id)
            .then_some(Event::Pressed(id))
    }
}
