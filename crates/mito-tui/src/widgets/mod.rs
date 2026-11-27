//! Kit mínimo de widgets con foco y zonas pulsables.
//!
//! Modo inmediato con estado retenido: cada widget es un `struct` con su estado y un
//! `render(&self, frame, area, ui)` que dibuja y **registra** su zona y su posición en
//! el orden de foco en el [`Ui`] del fotograma. El [`Focus`] de la pantalla recuerda el
//! id enfocado entre fotogramas. Cada widget expone `handle(&mut self, &Input)` para
//! cuando tiene el foco; los clics los resuelve la pantalla con [`Focus::hit`].

pub mod button;
pub mod checkbox;
pub mod focus;
pub mod header;
pub mod log_view;
pub mod modal;
pub mod notice;
pub mod select;
pub mod stack;
pub mod table;
pub mod text;
pub mod text_input;
pub mod text_view;

#[cfg(test)]
pub(crate) mod testing;

pub use button::{Button, Variant};
pub use checkbox::Checkbox;
pub use focus::{Focus, Ui, WidgetId};
pub use log_view::LogView;
pub use notice::{Notice, Severity};
pub use select::Select;
pub use stack::Stack;
pub use table::{Column, Table, TableRow};
pub use text_input::TextInput;
pub use text_view::TextView;

/// Evento que produce un widget al procesar una entrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetEvent {
    /// Se activó un botón.
    Activated(WidgetId),
    /// Cambió el valor de una casilla.
    Toggled(WidgetId, bool),
    /// Cambió el valor de un campo o la selección de un selector.
    Changed(WidgetId),
    /// Se pulsó Intro en un campo de texto.
    Submitted(WidgetId),
    /// El cursor de una tabla se movió a la fila con esa clave.
    Highlighted(WidgetId, String),
}
