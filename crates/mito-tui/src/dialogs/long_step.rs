//! Aviso de un paso largo sin progreso medible (`LongStepDialog`).

use std::time::Instant;

use mito_core::Message;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::Content;
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Focus, Ui};

/// «PREPARANDO ARCHIVOS»: muestra el paso en curso y el tiempo transcurrido. No tiene
/// `Esc` ni botones: se cierra cuando la aplicación llama a [`finish`](Self::finish) y
/// llega el siguiente tic.
#[derive(Debug)]
pub struct LongStepDialog {
    focus: Focus,
    message: Message,
    started: Instant,
    finished: bool,
}

impl LongStepDialog {
    /// Diálogo para el paso `message`; el reloj empieza ahora.
    pub fn new(message: Message) -> Self {
        Self::with_start(message, Instant::now())
    }

    /// Como [`new`](Self::new) con el instante en que empezó el paso.
    pub fn with_start(message: Message, started: Instant) -> Self {
        Self {
            focus: Focus::new(),
            message,
            started,
            finished: false,
        }
    }

    /// Marca el paso como terminado: el diálogo se cierra en el siguiente tic.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// ¿Se marcó como terminado?
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Segundos enteros desde que empezó el paso.
    pub fn elapsed_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

impl Dialog for LongStepDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let elapsed = ui
            .tr
            .tr(&Message::new("Tiempo transcurrido: {p1} s").arg("p1", self.elapsed_seconds()));
        let mut content = Content::new();
        content
            .text(ui.tr.text("PREPARANDO ARCHIVOS"), theme::title())
            .gap(1)
            .text(ui.tr.tr(&self.message), theme::panel())
            .gap(1)
            .text(elapsed, theme::dim());
        let width = Size::Cells(72).resolve(screen.width);
        let height = content.height(width.saturating_sub(4)).saturating_add(2);
        let area = modal::area(screen, Size::Cells(72), Size::Auto(height));
        let inner = modal::frame(frame, area, theme::BORDER_WARNING);
        content.render(frame, inner);
    }

    fn handle(&mut self, _input: &Input, _context: &mut Context) -> Step {
        Step::Stay
    }

    fn tick(&mut self, _context: &mut Context) -> Step {
        if self.finished {
            Step::Close(Outcome::Closed)
        } else {
            Step::Stay
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;
    use crossterm::event::KeyCode;
    use std::time::Duration;

    fn message() -> Message {
        Message::new("Publicando {p1}").arg("p1", "main/tool")
    }

    #[test]
    fn aspecto_y_reloj() {
        let harness = Harness::spanish();
        let started = Instant::now().checked_sub(Duration::from_secs(7)).unwrap();
        let mut dialog = LongStepDialog::with_start(message(), started);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn el_reloj_empieza_en_cero() {
        let harness = Harness::spanish();
        let mut dialog = LongStepDialog::new(message());
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(text.contains("Tiempo transcurrido: 0 s"), "{text}");
    }

    #[test]
    fn esc_y_el_resto_de_teclas_no_lo_cierran() {
        let mut harness = Harness::new();
        let mut dialog = LongStepDialog::new(message());
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(harness.press(&mut dialog, KeyCode::Esc), Step::Stay);
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        assert_eq!(harness.tick(&mut dialog), Step::Stay);
    }

    #[test]
    fn finish_lo_cierra_en_el_siguiente_tic() {
        let mut harness = Harness::new();
        let mut dialog = LongStepDialog::new(message());
        dialog.finish();
        assert!(dialog.is_finished());
        assert_eq!(harness.tick(&mut dialog), Step::Close(Outcome::Closed));
    }
}
