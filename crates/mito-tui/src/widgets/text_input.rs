//! Campo de texto de una línea.

use std::cell::Cell;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::{WidgetEvent, text};
use crate::input::Input;
use crate::theme;

/// Campo de texto con cursor. El cursor y el límite se cuentan en `char`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    /// Id lógico.
    pub id: WidgetId,
    /// Contenido.
    pub value: String,
    /// Posición del cursor, en caracteres desde el inicio.
    pub cursor: usize,
    /// Texto atenuado que se muestra con el campo vacío.
    pub placeholder: String,
    /// Longitud máxima en caracteres.
    pub max_length: Option<usize>,
    /// Un campo deshabilitado no se registra y se dibuja atenuado.
    pub disabled: bool,
    scroll: Cell<usize>,
    area: Cell<Rect>,
}

impl TextInput {
    /// Campo vacío y habilitado.
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id: id.into(),
            value: String::new(),
            cursor: 0,
            placeholder: String::new(),
            max_length: None,
            disabled: false,
            scroll: Cell::new(0),
            area: Cell::new(Rect::ZERO),
        }
    }

    /// Campo con un valor inicial y el cursor al final.
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.set_value(value);
        self
    }

    /// Cambia el texto atenuado del campo vacío.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Limita la longitud en caracteres.
    #[must_use]
    pub fn with_max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Sustituye el contenido y deja el cursor al final (respeta `max_length`).
    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.value = match self.max_length {
            Some(max) => value.chars().take(max).collect(),
            None => value,
        };
        self.cursor = self.len();
    }

    /// Número de caracteres.
    pub fn len(&self) -> usize {
        self.value.chars().count()
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(index, _)| index)
    }

    /// Inserta `text` en el cursor sin saltos de línea y respetando `max_length`.
    pub fn insert(&mut self, text: &str) -> bool {
        let room = self
            .max_length
            .map_or(usize::MAX, |max| max.saturating_sub(self.len()));
        let filtered: String = text
            .chars()
            .filter(|c| !matches!(c, '\r' | '\n'))
            .take(room)
            .collect();
        if filtered.is_empty() {
            return false;
        }
        let at = self.byte_index(self.cursor);
        self.value.insert_str(at, &filtered);
        self.cursor += filtered.chars().count();
        true
    }

    /// Procesa una tecla o un pegado cuando el campo tiene el foco.
    pub fn handle(&mut self, input: &Input) -> Option<WidgetEvent> {
        if self.disabled {
            return None;
        }
        let changed = WidgetEvent::Changed(self.id.clone());
        if let Input::Paste(pasted) = input {
            return self.insert(pasted).then_some(changed);
        }
        if let Some(c) = input.typed() {
            return self.insert(c.encode_utf8(&mut [0; 4])).then_some(changed);
        }
        let Input::Key(_) = input else {
            return None;
        };
        if input.is(KeyCode::Enter) {
            return Some(WidgetEvent::Submitted(self.id.clone()));
        }
        if input.is(KeyCode::Left) {
            self.cursor = self.cursor.saturating_sub(1);
        } else if input.is(KeyCode::Right) {
            self.cursor = (self.cursor + 1).min(self.len());
        } else if input.is(KeyCode::Home) {
            self.cursor = 0;
        } else if input.is(KeyCode::End) {
            self.cursor = self.len();
        } else if input.is(KeyCode::Backspace) {
            if self.cursor == 0 {
                return None;
            }
            let start = self.byte_index(self.cursor - 1);
            let end = self.byte_index(self.cursor);
            self.value.replace_range(start..end, "");
            self.cursor -= 1;
            return Some(changed);
        } else if input.is(KeyCode::Delete) {
            if self.cursor >= self.len() {
                return None;
            }
            let start = self.byte_index(self.cursor);
            let end = self.byte_index(self.cursor + 1);
            self.value.replace_range(start..end, "");
            return Some(changed);
        }
        None
    }

    /// Un clic en la columna `column` coloca el cursor en ese carácter.
    pub fn click(&mut self, column: u16) {
        let area = self.area.get();
        let target = usize::from(column.saturating_sub(area.x));
        let mut used = 0;
        let mut index = self.scroll.get();
        for c in self.value.chars().skip(self.scroll.get()) {
            let w = text::width(c.encode_utf8(&mut [0; 4]));
            if used + w > target {
                break;
            }
            used += w;
            index += 1;
        }
        self.cursor = index.min(self.len());
    }

    /// Dibuja el campo en la primera fila de `area` y, si está habilitado, registra su
    /// zona. Con el foco coloca el cursor del terminal.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        let row = Rect::new(area.x, area.y, area.width, 1);
        self.area.set(row);
        let focused = !self.disabled && ui.is_focused(&self.id);
        let base: Style = if self.disabled {
            Style::new().fg(theme::DIM).bg(theme::INPUT_BG)
        } else if focused {
            theme::input_focused()
        } else {
            theme::input()
        };
        frame.buffer_mut().set_style(row, base);
        let width = usize::from(row.width);
        if self.value.is_empty() {
            let placeholder = text::truncate(&self.placeholder, width);
            frame.render_widget(
                Paragraph::new(Span::styled(placeholder, base.fg(theme::DIM))),
                row,
            );
            self.scroll.set(0);
        } else {
            // Ventana horizontal: el cursor siempre queda dentro.
            let chars: Vec<char> = self.value.chars().collect();
            let mut scroll = self.scroll.get().min(self.cursor);
            let cell = |c: char| text::width(c.encode_utf8(&mut [0; 4]));
            while chars[scroll..self.cursor.min(chars.len())]
                .iter()
                .map(|c| cell(*c))
                .sum::<usize>()
                >= width.max(2)
            {
                scroll += 1;
            }
            self.scroll.set(scroll);
            let mut shown = String::new();
            let mut used = 0;
            for c in &chars[scroll..] {
                let w = cell(*c);
                if used + w > width {
                    break;
                }
                shown.push(*c);
                used += w;
            }
            frame.render_widget(Paragraph::new(Span::styled(shown, base)), row);
        }
        if !self.disabled {
            ui.focusable(&self.id, row);
        }
        if focused {
            let chars: Vec<char> = self.value.chars().collect();
            let offset: usize = chars
                [self.scroll.get().min(chars.len())..self.cursor.min(chars.len())]
                .iter()
                .map(|c| text::width(c.encode_utf8(&mut [0; 4])))
                .sum();
            let x = row
                .x
                .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX))
                .min(row.right().saturating_sub(1));
            frame.set_cursor_position((x, row.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    fn type_text(input: &mut TextInput, text: &str) {
        for c in text.chars() {
            input.handle(&Input::character(c));
        }
    }

    #[test]
    fn escribe_borra_y_mueve_el_cursor() {
        let mut input = TextInput::new("n");
        type_text(&mut input, "hola");
        assert_eq!(input.value, "hola");
        assert_eq!(input.cursor, 4);
        input.handle(&Input::key(KeyCode::Left));
        input.handle(&Input::key(KeyCode::Left));
        type_text(&mut input, "X");
        assert_eq!(input.value, "hoXla");
        input.handle(&Input::key(KeyCode::Backspace));
        assert_eq!(input.value, "hola");
        input.handle(&Input::key(KeyCode::Delete));
        assert_eq!(input.value, "hoa");
        input.handle(&Input::key(KeyCode::Home));
        assert_eq!(input.cursor, 0);
        assert_eq!(input.handle(&Input::key(KeyCode::Backspace)), None);
        input.handle(&Input::key(KeyCode::End));
        assert_eq!(input.cursor, 3);
        assert_eq!(input.handle(&Input::key(KeyCode::Delete)), None);
    }

    #[test]
    fn cuenta_caracteres_no_bytes() {
        let mut input = TextInput::new("n").with_max_length(3);
        type_text(&mut input, "añoz");
        assert_eq!(input.value, "año");
        input.handle(&Input::key(KeyCode::Left));
        input.handle(&Input::key(KeyCode::Backspace));
        assert_eq!(input.value, "ao");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn pegar_quita_saltos_de_linea_y_respeta_el_maximo() {
        let mut input = TextInput::new("n").with_max_length(5);
        assert_eq!(
            input.handle(&Input::Paste("ab\r\ncd\nefgh".into())),
            Some(WidgetEvent::Changed("n".into()))
        );
        assert_eq!(input.value, "abcde");
        assert_eq!(input.handle(&Input::Paste("x".into())), None);
    }

    #[test]
    fn intro_envia_y_los_atajos_no_escriben() {
        let mut input = TextInput::new("n");
        assert_eq!(
            input.handle(&Input::key(KeyCode::Enter)),
            Some(WidgetEvent::Submitted("n".into()))
        );
        assert_eq!(input.handle(&Input::ctrl('a')), None);
        assert_eq!(input.value, "");
        input.disabled = true;
        assert_eq!(input.handle(&Input::character('a')), None);
    }

    #[test]
    fn el_placeholder_se_ve_solo_con_el_campo_vacio() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let empty = TextInput::new("n").with_placeholder("Nombre");
        let text = testing::render(20, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            empty.render(frame, area, &mut ui);
        });
        assert_eq!(text, "Nombre");
        let filled = TextInput::new("n")
            .with_placeholder("Nombre")
            .with_value("abc");
        let text = testing::render(20, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            filled.render(frame, area, &mut ui);
        });
        assert_eq!(text, "abc");
    }

    #[test]
    fn el_texto_largo_se_desplaza_para_ver_el_cursor() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let input = TextInput::new("n").with_value("0123456789");
        let text = testing::render(5, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            input.render(frame, area, &mut ui);
        });
        assert_eq!(text, "6789");
    }

    #[test]
    fn el_clic_coloca_el_cursor() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let mut input = TextInput::new("n").with_value("abcdef");
        testing::render(10, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            input.render(frame, Rect::new(area.x + 2, area.y, 8, 1), &mut ui);
        });
        input.click(5);
        assert_eq!(input.cursor, 3);
        input.click(40);
        assert_eq!(input.cursor, 6);
    }
}
