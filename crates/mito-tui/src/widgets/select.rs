//! Selector con lista desplegable superpuesta.

use std::cell::Cell;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::focus::{Ui, WidgetId};
use super::{WidgetEvent, text};
use crate::input::Input;
use crate::theme;

/// Filas visibles como máximo de la lista desplegable.
const MAX_ROWS: usize = 8;

/// Selector: una lista de `(etiqueta, valor)` con un elemento elegido.
///
/// Cerrado ocupa una fila (`etiqueta ▼`). Abierto, la lista se dibuja superpuesta con
/// [`render_overlay`](Self::render_overlay), que hay que llamar **después** de dibujar
/// el resto de la pantalla.
#[derive(Debug, Clone)]
pub struct Select<T> {
    /// Id lógico.
    pub id: WidgetId,
    /// Opciones: etiqueta ya traducida y valor.
    pub options: Vec<(String, T)>,
    /// Índice elegido; `None` muestra el `prompt`.
    pub selected: Option<usize>,
    /// Texto que se muestra sin elección.
    pub prompt: String,
    /// ¿Está desplegada la lista?
    pub open: bool,
    /// Un selector deshabilitado no se registra y se dibuja atenuado.
    pub disabled: bool,
    /// Opción resaltada en la lista abierta.
    pub highlight: usize,
    list: Cell<Rect>,
    field: Cell<Rect>,
    offset: Cell<usize>,
}

impl<T> Select<T> {
    /// Selector cerrado con `options` y la opción `selected` elegida.
    pub fn new(
        id: impl Into<WidgetId>,
        options: Vec<(String, T)>,
        selected: Option<usize>,
    ) -> Self {
        let selected = selected.filter(|i| *i < options.len());
        Self {
            id: id.into(),
            options,
            selected,
            prompt: String::new(),
            open: false,
            disabled: false,
            highlight: selected.unwrap_or(0),
            list: Cell::new(Rect::ZERO),
            field: Cell::new(Rect::ZERO),
            offset: Cell::new(0),
        }
    }

    /// Texto que se muestra sin elección.
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    /// Valor de la opción elegida.
    pub fn value(&self) -> Option<&T> {
        self.selected
            .and_then(|i| self.options.get(i))
            .map(|(_, value)| value)
    }

    /// Etiqueta de la opción elegida.
    pub fn label(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.options.get(i))
            .map(|(label, _)| label.as_str())
    }

    /// Elige la opción `index` (si existe) y cierra la lista.
    pub fn select(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = Some(index);
            self.highlight = index;
        }
        self.open = false;
    }

    /// Cierra la lista sin cambiar la elección.
    pub fn close(&mut self) {
        self.open = false;
    }

    fn open_list(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.open = true;
        self.highlight = self.selected.unwrap_or(0);
    }

    fn choose(&mut self, index: usize) -> Option<WidgetEvent> {
        let changed = self.selected != Some(index);
        self.select(index);
        changed.then(|| WidgetEvent::Changed(self.id.clone()))
    }

    /// Procesa una entrada cuando el selector tiene el foco. Intro, Espacio o ↓ abren
    /// la lista; con ella abierta ↑↓ mueven, Intro o Espacio eligen y Esc cierra.
    pub fn handle(&mut self, input: &Input) -> Option<WidgetEvent> {
        if self.disabled {
            return None;
        }
        if !self.open {
            if input.is_activation() || input.is(KeyCode::Down) {
                self.open_list();
            }
            return None;
        }
        let last = self.options.len().saturating_sub(1);
        match input {
            _ if input.is(KeyCode::Up) => self.highlight = self.highlight.saturating_sub(1),
            _ if input.is(KeyCode::Down) => self.highlight = (self.highlight + 1).min(last),
            _ if input.is(KeyCode::PageUp) => {
                self.highlight = self.highlight.saturating_sub(MAX_ROWS);
            }
            _ if input.is(KeyCode::PageDown) => {
                self.highlight = (self.highlight + MAX_ROWS).min(last);
            }
            _ if input.is(KeyCode::Home) => self.highlight = 0,
            _ if input.is(KeyCode::End) => self.highlight = last,
            _ if input.is(KeyCode::Esc) => self.close(),
            _ if input.is_activation() => return self.choose(self.highlight),
            Input::Scroll { down, .. } => {
                self.highlight = if *down {
                    (self.highlight + 1).min(last)
                } else {
                    self.highlight.saturating_sub(1)
                };
            }
            _ => {}
        }
        None
    }

    /// Un clic: sobre una opción de la lista abierta la elige; en cualquier otro punto
    /// abre o cierra la lista.
    pub fn click(&mut self, column: u16, row: u16) -> Option<WidgetEvent> {
        if self.disabled {
            return None;
        }
        let list = self.list.get();
        if self.open && list.contains((column, row).into()) {
            let inner_row = usize::from(row.saturating_sub(list.y + 1));
            let index = self.offset.get() + inner_row;
            if index < self.options.len() && row > list.y && row < list.bottom().saturating_sub(1) {
                return self.choose(index);
            }
            return None;
        }
        if self.open {
            self.close();
        } else {
            self.open_list();
        }
        None
    }

    /// Dibuja el selector cerrado en la primera fila de `area` y, si está habilitado,
    /// registra su zona.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.is_empty() {
            return;
        }
        let row = Rect::new(area.x, area.y, area.width, 1);
        self.field.set(row);
        let focused = !self.disabled && ui.is_focused(&self.id);
        let base = if self.disabled {
            Style::new().fg(theme::DIM).bg(theme::INPUT_BG)
        } else if focused {
            theme::input_focused()
        } else {
            theme::input()
        };
        frame.buffer_mut().set_style(row, base);
        let room = usize::from(row.width).saturating_sub(2);
        let (shown, style) = match self.label() {
            Some(label) => (text::truncate(label, room), base),
            None => (text::truncate(&self.prompt, room), base.fg(theme::DIM)),
        };
        let line = Line::from(vec![Span::styled(shown, style)]);
        frame.render_widget(Paragraph::new(line), row);
        if row.width >= 2 {
            let arrow = Rect::new(row.right() - 1, row.y, 1, 1);
            frame.render_widget(Paragraph::new(Span::styled("▼", base)), arrow);
        }
        if !self.disabled {
            ui.focusable(&self.id, row);
        }
    }

    /// Dibuja la lista desplegable (si está abierta) sobre lo ya dibujado y registra su
    /// zona superpuesta. Llamar el último en el fotograma.
    pub fn render_overlay(&self, frame: &mut Frame, ui: &mut Ui) {
        if !self.open || self.options.is_empty() {
            return;
        }
        let screen = frame.area();
        let field = self.field.get();
        let rows = self.options.len().min(MAX_ROWS);
        let longest = self
            .options
            .iter()
            .map(|(label, _)| text::width(label))
            .max()
            .unwrap_or(0);
        let width = u16::try_from(longest + 4)
            .unwrap_or(u16::MAX)
            .max(field.width)
            .min(screen.width);
        let wanted = u16::try_from(rows + 2).unwrap_or(u16::MAX);
        let below = screen.bottom().saturating_sub(field.bottom());
        let (y, height) = if below >= wanted {
            (field.bottom(), wanted)
        } else if field.y.saturating_sub(screen.y) >= wanted {
            (field.y - wanted, wanted)
        } else {
            (field.bottom(), below.max(3))
        };
        let x = field.x.min(screen.right().saturating_sub(width));
        let list = Rect::new(x, y, width, height).intersection(screen);
        self.list.set(list);
        if list.height < 3 {
            return;
        }
        let visible = usize::from(list.height - 2);
        let mut offset = self
            .offset
            .get()
            .min(self.options.len().saturating_sub(visible));
        if self.highlight < offset {
            offset = self.highlight;
        } else if self.highlight >= offset + visible {
            offset = self.highlight + 1 - visible;
        }
        self.offset.set(offset);
        frame.render_widget(Clear, list);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::BORDER).bg(theme::DETAILS))
            .style(theme::details());
        let inner = block.inner(list);
        frame.render_widget(block, list);
        let lines: Vec<Line> = self
            .options
            .iter()
            .enumerate()
            .skip(offset)
            .take(visible)
            .map(|(index, (label, _))| {
                let style = if index == self.highlight {
                    theme::cursor()
                } else if Some(index) == self.selected {
                    theme::details().add_modifier(Modifier::BOLD)
                } else {
                    theme::details()
                };
                let label = text::truncate(label, usize::from(inner.width));
                Line::styled(
                    format!("{label:<width$}", width = usize::from(inner.width)),
                    style,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        ui.overlay(&self.id, list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    fn shells() -> Select<&'static str> {
        Select::new(
            "shell",
            vec![
                ("cmd".into(), "cmd"),
                ("powershell".into(), "powershell"),
                ("mito".into(), "mito"),
            ],
            Some(0),
        )
    }

    fn draw(select: &Select<&'static str>, focus: &mut Focus, height: u16) -> String {
        let tr = Translator::new("english");
        testing::render(24, height, |frame, area| {
            let mut ui = focus.frame(&tr);
            select.render(frame, Rect::new(area.x + 1, area.y + 1, 12, 1), &mut ui);
            select.render_overlay(frame, &mut ui);
        })
    }

    #[test]
    fn cerrado_muestra_la_etiqueta_o_el_prompt() {
        let mut focus = Focus::new();
        assert_eq!(draw(&shells(), &mut focus, 6), "\n cmd        ▼");
        let empty: Select<u8> = Select::new("s", vec![], None).with_prompt("Elige");
        let tr = Translator::new("english");
        let text = testing::render(10, 1, |frame, area| {
            let mut ui = focus.frame(&tr);
            empty.render(frame, area, &mut ui);
        });
        assert_eq!(text, "Elige    ▼");
    }

    #[test]
    fn abierto_dibuja_la_lista_superpuesta() {
        let mut select = shells();
        let mut focus = Focus::new();
        select.handle(&Input::key(KeyCode::Enter));
        assert!(select.open);
        let text = draw(&select, &mut focus, 8);
        assert_eq!(
            text,
            "\n cmd        ▼\n ╭────────────╮\n │cmd         │\n │powershell  │\n │mito        │\n ╰────────────╯"
        );
        assert_eq!(focus.hit(3, 4).as_deref(), Some("shell"));
    }

    #[test]
    fn teclas_eligen_y_solo_notifica_si_cambia() {
        let mut select = shells();
        select.handle(&Input::key(KeyCode::Enter));
        assert_eq!(select.handle(&Input::key(KeyCode::Down)), None);
        assert_eq!(
            select.handle(&Input::key(KeyCode::Enter)),
            Some(WidgetEvent::Changed("shell".into()))
        );
        assert!(!select.open);
        assert_eq!(select.value(), Some(&"powershell"));
        select.handle(&Input::key(KeyCode::Enter));
        assert_eq!(select.handle(&Input::key(KeyCode::Enter)), None);
        select.handle(&Input::key(KeyCode::Enter));
        select.handle(&Input::key(KeyCode::End));
        select.handle(&Input::key(KeyCode::Esc));
        assert!(!select.open);
        assert_eq!(select.value(), Some(&"powershell"));
    }

    #[test]
    fn el_clic_abre_elige_y_cierra() {
        let mut select = shells();
        let mut focus = Focus::new();
        draw(&select, &mut focus, 8);
        assert_eq!(select.click(3, 1), None);
        assert!(select.open);
        draw(&select, &mut focus, 8);
        // Tercera opción (`mito`) en la fila 5 del búfer.
        assert_eq!(
            select.click(3, 5),
            Some(WidgetEvent::Changed("shell".into()))
        );
        assert_eq!(select.label(), Some("mito"));
        assert!(!select.open);
        select.click(3, 1);
        select.click(0, 7);
        assert!(!select.open, "un clic fuera cierra");
    }

    #[test]
    fn deshabilitado_no_se_abre_ni_se_registra() {
        let mut select = shells();
        select.disabled = true;
        let mut focus = Focus::new();
        draw(&select, &mut focus, 4);
        assert!(!focus.is_registered("shell"));
        select.handle(&Input::key(KeyCode::Enter));
        assert!(!select.open);
        assert_eq!(select.click(1, 1), None);
        assert!(!select.open);
    }

    #[test]
    fn la_lista_larga_se_desplaza_con_el_resaltado() {
        let options: Vec<(String, usize)> = (0..20).map(|i| (format!("opción {i}"), i)).collect();
        let mut select = Select::new("l", options, Some(0));
        select.handle(&Input::key(KeyCode::Enter));
        select.handle(&Input::key(KeyCode::End));
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        let text = testing::render(20, 14, |frame, area| {
            let mut ui = focus.frame(&tr);
            select.render(frame, Rect::new(area.x, area.y, 14, 1), &mut ui);
            select.render_overlay(frame, &mut ui);
        });
        assert!(text.contains("opción 19"));
        assert!(!text.contains("opción 11"));
    }
}
