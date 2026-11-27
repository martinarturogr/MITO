//! Entradas del usuario: teclado, ratón y pegado.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use super::home::row_of;
use super::state::App;
use crate::input::Input;
use crate::widgets::{Severity, WidgetEvent};

/// ¿Es Ctrl más el carácter `c`?
fn is_ctrl(input: &Input, c: char) -> bool {
    matches!(input, Input::Key(key)
        if key.code == KeyCode::Char(c) && key.modifiers.contains(KeyModifiers::CONTROL))
}

impl App {
    /// Procesa una entrada del usuario.
    pub fn handle(&mut self, input: &Input, now: Instant) {
        self.now = now;
        if is_ctrl(input, 'q') {
            self.action_quit();
        } else if !self.dialogs.is_empty() {
            self.dialog_input(input);
        } else {
            self.main_input(input);
        }
    }

    fn main_input(&mut self, input: &Input) {
        if self.profile.open {
            let event = match input {
                Input::Click { column, row } => self.profile.click(*column, *row),
                other => self.profile.handle(other),
            };
            if event.is_some() {
                self.profile_changed();
            }
            return;
        }
        if is_ctrl(input, 'f') {
            self.search();
        } else if input.is(KeyCode::Esc) {
            self.go_home();
        } else if input.is_tab() {
            self.focus.next();
            self.reveal_focus();
        } else if input.is_back_tab() {
            self.focus.previous();
            self.reveal_focus();
        } else {
            match input {
                Input::Click { column, row } => {
                    if let Some(id) = self.focus.hit(*column, *row) {
                        self.focus.set(&id);
                        self.reveal_focus();
                        self.click(&id, *column, *row);
                    }
                }
                Input::Scroll { column, row, down } => self.scroll(input, *column, *row, *down),
                _ => self.key(input),
            }
        }
    }

    /// El selector de profiles cambió: ajustes de la terminal e Inicio.
    fn profile_changed(&mut self) {
        self.refresh_terminal_default();
        self.refresh_home();
    }

    /// Pide a la siguiente pasada de dibujo que muestre el widget enfocado (filas de
    /// Inicio y casillas de bucket fuera de la vista).
    fn reveal_focus(&mut self) {
        self.home.reveal = true;
        self.catalog.reveal = true;
    }

    /// Pulsación en el widget `id`.
    fn click(&mut self, id: &str, column: u16, row: u16) {
        match id {
            "profile" => {
                if self.profile.click(column, row).is_some() {
                    self.profile_changed();
                }
            }
            "search" => self.catalog.search.click(column),
            "apps" => match self.catalog.table.click(column, row) {
                Some(WidgetEvent::Highlighted(_, key)) => self.select_row(&key),
                _ => self.select_cursor(),
            },
            "details" | "log" => {}
            _ => self.activate(id),
        }
    }

    /// Activa un botón o una casilla (Intro, Espacio o clic).
    fn activate(&mut self, id: &str) {
        if let Some(name) = id.strip_prefix("bucket-") {
            let name = name.to_owned();
            self.toggle_bucket(&name);
        } else if id.starts_with("active-") {
            if let Some(index) = row_of(id) {
                self.toggle_row(index);
            }
        } else if id == "copy-log" {
            self.copy_log();
        } else {
            // Un id desconocido no hace nada.
            let _ = self.sidebar_button(id) || self.home_button(id) || self.catalog_button(id);
        }
    }

    /// Tecla o pegado para el widget enfocado.
    fn key(&mut self, input: &Input) {
        let Some(id) = self.focus.focused().map(str::to_owned) else {
            return;
        };
        match id.as_str() {
            "profile" => {
                if self.profile.handle(input).is_some() {
                    self.profile_changed();
                }
            }
            "search" => {
                if let Some(WidgetEvent::Changed(_)) = self.catalog.search.handle(input) {
                    self.filter_catalog();
                }
            }
            "apps" => match self.catalog.table.handle(input) {
                Some(WidgetEvent::Highlighted(_, key)) => self.select_row(&key),
                _ if input.is_activation() => self.select_cursor(),
                _ => {}
            },
            "details" => self.catalog.details.handle(input),
            "log" => self.log.handle(input),
            _ if input.is_activation() => self.activate(&id),
            _ => {}
        }
    }

    /// Rueda del ratón sobre la celda (`column`, `row`).
    fn scroll(&mut self, input: &Input, column: u16, row: u16, down: bool) {
        let over = |app: &Self, id: &str| {
            app.focus
                .area_of(id)
                .is_some_and(|area| area.contains((column, row).into()))
        };
        let delta = if down { 1 } else { -1 };
        if over(self, "log") {
            self.log.handle(input);
        } else if over(self, "details") {
            self.catalog.details.handle(input);
        } else if over(self, "apps") {
            self.catalog.table.scroll(down);
        } else if over(self, "installed-list") {
            self.home.scroll_by(delta);
        } else if over(self, "bucket-filters") {
            let last = self.catalog.buckets.len().saturating_sub(1);
            self.catalog.scroll = self.catalog.scroll.saturating_add_signed(delta).min(last);
        }
    }

    /// «Copiar log»: el texto plano de todas las líneas, también durante una operación.
    pub(super) fn copy_log(&mut self) {
        if self.log.entries.is_empty() {
            let text = self.text("El log está vacío");
            self.notify(text, Severity::Info);
            return;
        }
        let text = self.log.plain();
        if (self.clipboard)(&text).is_ok() {
            let text = self.text("Log copiado al portapapeles");
            self.notify(text, Severity::Info);
        } else {
            let text = self.text("No se pudo copiar el log al portapapeles");
            self.notify(text, Severity::Error);
        }
    }
}
