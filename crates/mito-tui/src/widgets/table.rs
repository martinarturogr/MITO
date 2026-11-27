//! Tabla con cursor de fila y filas alternas.

use std::cell::Cell;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::focus::{Ui, WidgetId};
use super::{WidgetEvent, text};
use crate::input::Input;
use crate::theme;

/// Columna: título (ya traducido) y ancho.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Título de la columna.
    pub title: String,
    /// Ancho de la columna.
    pub width: Constraint,
}

impl Column {
    /// Columna con un título y un ancho.
    pub fn new(title: impl Into<String>, width: Constraint) -> Self {
        Self {
            title: title.into(),
            width,
        }
    }
}

/// Fila: clave estable y una celda de texto por columna.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    /// Clave de la fila (`bucket/app`), la que emite [`WidgetEvent::Highlighted`].
    pub key: String,
    /// Celdas, ya traducidas.
    pub cells: Vec<String>,
}

impl TableRow {
    /// Fila con clave y celdas.
    pub fn new(key: impl Into<String>, cells: Vec<String>) -> Self {
        Self {
            key: key.into(),
            cells,
        }
    }
}

/// Tabla con cabecera, filas alternas (zebra) y cursor de fila.
#[derive(Debug, Clone)]
pub struct Table {
    /// Id lógico.
    pub id: WidgetId,
    /// Columnas.
    pub columns: Vec<Column>,
    /// Filas.
    pub rows: Vec<TableRow>,
    /// Fila con el cursor.
    pub cursor: usize,
    /// Primera fila visible.
    pub offset: usize,
    /// Una tabla deshabilitada no se registra.
    pub disabled: bool,
    height: Cell<usize>,
    body: Cell<Rect>,
}

impl Table {
    /// Tabla vacía con las columnas dadas.
    pub fn new(id: impl Into<WidgetId>, columns: Vec<Column>) -> Self {
        Self {
            id: id.into(),
            columns,
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            disabled: false,
            height: Cell::new(10),
            body: Cell::new(Rect::ZERO),
        }
    }

    /// Sustituye las filas y lleva el cursor y el desplazamiento al principio.
    pub fn set_rows(&mut self, rows: Vec<TableRow>) {
        self.rows = rows;
        self.cursor = 0;
        self.offset = 0;
    }

    /// Clave de la fila con el cursor.
    pub fn highlighted(&self) -> Option<&str> {
        self.rows.get(self.cursor).map(|row| row.key.as_str())
    }

    fn follow_cursor(&mut self) {
        let height = self.height.get().max(1);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
    }

    fn move_to(&mut self, index: usize) -> Option<WidgetEvent> {
        if self.rows.is_empty() {
            return None;
        }
        let index = index.min(self.rows.len() - 1);
        if index == self.cursor {
            return None;
        }
        self.cursor = index;
        self.follow_cursor();
        self.highlighted()
            .map(|key| WidgetEvent::Highlighted(self.id.clone(), key.to_owned()))
    }

    /// Procesa una entrada cuando la tabla tiene el foco: ↑↓, RePág/AvPág, Inicio/Fin y
    /// la rueda (que desplaza sin mover el cursor).
    pub fn handle(&mut self, input: &Input) -> Option<WidgetEvent> {
        if self.disabled {
            return None;
        }
        let page = self.height.get().max(1);
        let cursor = self.cursor;
        match input {
            _ if input.is(KeyCode::Up) => self.move_to(cursor.saturating_sub(1)),
            _ if input.is(KeyCode::Down) => self.move_to(cursor + 1),
            _ if input.is(KeyCode::PageUp) => self.move_to(cursor.saturating_sub(page)),
            _ if input.is(KeyCode::PageDown) => self.move_to(cursor + page),
            _ if input.is(KeyCode::Home) => self.move_to(0),
            _ if input.is(KeyCode::End) => self.move_to(usize::MAX),
            Input::Scroll { down, .. } => {
                self.scroll(*down);
                None
            }
            _ => None,
        }
    }

    /// Desplaza la vista una fila (la rueda).
    pub fn scroll(&mut self, down: bool) {
        let max = self.rows.len().saturating_sub(self.height.get().max(1));
        self.offset = if down {
            (self.offset + 1).min(max)
        } else {
            self.offset.saturating_sub(1)
        };
    }

    /// Un clic sobre una fila coloca el cursor en ella.
    pub fn click(&mut self, column: u16, row: u16) -> Option<WidgetEvent> {
        let body = self.body.get();
        if self.disabled || !body.contains((column, row).into()) {
            return None;
        }
        let index = self.offset + usize::from(row - body.y);
        if index >= self.rows.len() {
            return None;
        }
        self.move_to(index)
    }

    /// Dibuja la cabecera y las filas visibles en `area` y registra la zona.
    pub fn render(&self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        if area.height < 2 {
            return;
        }
        let focused = !self.disabled && ui.is_focused(&self.id);
        let header = Rect::new(area.x, area.y, area.width, 1);
        let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        self.body.set(body);
        self.height.set(usize::from(body.height));
        let constraints: Vec<Constraint> = self.columns.iter().map(|c| c.width).collect();
        let header_style = Style::new()
            .fg(theme::TITLE)
            .bg(theme::SIDEBAR)
            .add_modifier(Modifier::BOLD);
        frame.buffer_mut().set_style(header, header_style);
        let header_cells = Layout::horizontal(constraints.clone()).split(header);
        for (column, cell) in self.columns.iter().zip(header_cells.iter()) {
            let title = text::truncate(&column.title, usize::from(cell.width));
            frame.render_widget(Paragraph::new(Span::styled(title, header_style)), *cell);
        }
        let max_offset = self.rows.len().saturating_sub(usize::from(body.height));
        let offset = self.offset.min(max_offset);
        for (line, (index, row)) in self
            .rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(usize::from(body.height))
            .enumerate()
        {
            let area = Rect::new(
                body.x,
                body.y + u16::try_from(line).unwrap_or(0),
                body.width,
                1,
            );
            let style = if index == self.cursor && focused {
                theme::cursor()
            } else if index == self.cursor {
                Style::new()
                    .fg(theme::TEXT)
                    .bg(theme::ROW_LIGHT)
                    .add_modifier(Modifier::BOLD)
            } else if index % 2 == 0 {
                Style::new().fg(theme::TEXT).bg(theme::ROW_DARK)
            } else {
                Style::new().fg(theme::TEXT).bg(theme::ROW_LIGHT)
            };
            frame.buffer_mut().set_style(area, style);
            let cells = Layout::horizontal(constraints.clone()).split(area);
            for (value, cell) in row.cells.iter().zip(cells.iter()) {
                let value = text::truncate(value, usize::from(cell.width));
                frame.render_widget(Paragraph::new(Span::styled(value, style)), *cell);
            }
        }
        if !self.disabled {
            ui.focusable(&self.id, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::focus::Focus;
    use crate::widgets::testing;
    use mito_i18n::Translator;

    fn table(rows: usize) -> Table {
        let mut table = Table::new(
            "apps",
            vec![
                Column::new("Componente", Constraint::Length(12)),
                Column::new("Versión", Constraint::Min(5)),
            ],
        );
        table.set_rows(
            (0..rows)
                .map(|i| {
                    TableRow::new(
                        format!("main/app{i}"),
                        vec![format!("app{i}"), format!("1.{i}")],
                    )
                })
                .collect(),
        );
        table
    }

    fn draw(table: &Table, focus: &mut Focus, height: u16) -> String {
        let tr = Translator::new("english");
        testing::render(24, height, |frame, area| {
            let mut ui = focus.frame(&tr);
            table.render(frame, area, &mut ui);
        })
    }

    #[test]
    fn dibuja_cabecera_y_filas() {
        let mut focus = Focus::new();
        let text = draw(&table(3), &mut focus, 5);
        assert_eq!(
            text,
            "Componente  Versión\napp0        1.0\napp1        1.1\napp2        1.2"
        );
    }

    #[test]
    fn el_cursor_emite_highlighted_y_la_vista_lo_sigue() {
        let mut table = table(10);
        let mut focus = Focus::new();
        draw(&table, &mut focus, 4); // 3 filas visibles
        assert_eq!(table.highlighted(), Some("main/app0"));
        assert_eq!(table.handle(&Input::key(KeyCode::Up)), None);
        for expected in 1..=4 {
            assert_eq!(
                table.handle(&Input::key(KeyCode::Down)),
                Some(WidgetEvent::Highlighted(
                    "apps".into(),
                    format!("main/app{expected}")
                ))
            );
        }
        assert_eq!(table.offset, 2);
        let text = draw(&table, &mut focus, 4);
        assert_eq!(
            text,
            "Componente  Versión\napp2        1.2\napp3        1.3\napp4        1.4"
        );
        assert_eq!(
            table.handle(&Input::key(KeyCode::End)),
            Some(WidgetEvent::Highlighted("apps".into(), "main/app9".into()))
        );
        assert_eq!(
            table.handle(&Input::key(KeyCode::Home)),
            Some(WidgetEvent::Highlighted("apps".into(), "main/app0".into()))
        );
        assert_eq!(table.offset, 0);
        assert!(table.handle(&Input::key(KeyCode::PageDown)).is_some());
        assert_eq!(table.cursor, 3);
    }

    #[test]
    fn el_clic_selecciona_fila_y_la_rueda_desplaza() {
        let mut table = table(10);
        let mut focus = Focus::new();
        draw(&table, &mut focus, 4);
        assert_eq!(
            table.click(2, 3),
            Some(WidgetEvent::Highlighted("apps".into(), "main/app2".into()))
        );
        assert_eq!(table.click(2, 0), None, "la cabecera no es una fila");
        table.handle(&Input::Scroll {
            column: 1,
            row: 1,
            down: true,
        });
        assert_eq!((table.offset, table.cursor), (1, 2));
        table.handle(&Input::Scroll {
            column: 1,
            row: 1,
            down: false,
        });
        assert_eq!(table.offset, 0);
    }

    #[test]
    fn una_tabla_vacia_no_se_mueve() {
        let mut table = table(0);
        assert_eq!(table.handle(&Input::key(KeyCode::Down)), None);
        assert_eq!(table.highlighted(), None);
    }
}
