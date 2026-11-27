//! Utilidades de prueba: dibujar en un `TestBackend` y volcar el búfer como texto.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::text;

/// Búfer como texto: una línea por fila, sin espacios finales.
pub fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        let mut skip = 0usize;
        for x in area.left()..area.right() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buffer[(x, y)].symbol();
            skip = text::width(symbol).saturating_sub(1);
            line.push_str(symbol);
        }
        lines.push(line.trim_end().to_owned());
    }
    // Sin filas vacías finales, para que los snapshots no dependan de la altura.
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

/// Dibuja con `draw` en una pantalla de `width` x `height` y devuelve el texto.
pub fn render(width: u16, height: u16, draw: impl FnOnce(&mut Frame, Rect)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal de prueba");
    terminal
        .draw(|frame| {
            let area = frame.area();
            draw(frame, area);
        })
        .expect("dibujo");
    buffer_text(terminal.backend().buffer())
}

/// Dibuja con `draw` y devuelve el búfer, para comprobar estilos.
pub fn render_buffer(width: u16, height: u16, draw: impl FnOnce(&mut Frame, Rect)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal de prueba");
    terminal
        .draw(|frame| {
            let area = frame.area();
            draw(frame, area);
        })
        .expect("dibujo");
    terminal.backend().buffer().clone()
}
