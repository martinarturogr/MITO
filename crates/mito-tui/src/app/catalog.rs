//! Página «Buscar e instalar»: búsqueda, filtros por bucket, tabla y detalle.

use std::collections::BTreeSet;

use mito_core::Message;
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::state::{App, Page};
use crate::model::Catalog;
use crate::theme;
use crate::widgets::text;
use crate::widgets::{
    Button, Checkbox, Column, Stack, Table, TableRow, TextInput, TextView, Ui, Variant,
};
use crate::worker::Action;

/// Filas del detalle (`#details-scroll`).
const DETAILS_HEIGHT: u16 = 9;
/// Filas de los botones de acción.
const ACTIONS_HEIGHT: u16 = 3;
/// Mínimo de filas del resumen.
const SUMMARY_MIN: u16 = 3;

/// Estado del catálogo.
#[derive(Debug)]
pub(super) struct CatalogState {
    /// Entradas cargadas; `None` = hay que recargarlas (`entries` vacío).
    pub data: Option<Catalog>,
    /// Campo de búsqueda.
    pub search: TextInput,
    /// Nombres de las casillas de bucket.
    pub buckets: Vec<String>,
    /// Primera casilla de bucket visible.
    pub scroll: usize,
    /// Hay que desplazar los filtros hasta el widget enfocado.
    pub reveal: bool,
    /// Tabla de componentes.
    pub table: Table,
    /// Detalle del componente seleccionado.
    pub details: TextView,
    /// Resumen bajo los filtros.
    pub summary: String,
    /// Aviso «Sin componentes…» cuando no hay filas.
    pub hint: Option<String>,
    /// No hay filas.
    pub empty: bool,
    /// Clave seleccionada (`selected`).
    pub selected: Option<String>,
    /// «Instalar» procede para la selección (sin contar `busy`).
    pub can_install: bool,
    /// Ids de los componentes instalados (`catalog_installed`).
    pub installed: BTreeSet<String>,
}

impl CatalogState {
    /// Estado inicial con las columnas y el texto del campo ya traducidos.
    pub fn new(tr: &Translator) -> Self {
        let columns = vec![
            Column::new(tr.text("Componente"), Constraint::Percentage(38)),
            Column::new(tr.text("Versión"), Constraint::Percentage(14)),
            Column::new(tr.text("Disponibilidad"), Constraint::Percentage(24)),
            Column::new(tr.text("Licencia"), Constraint::Percentage(24)),
        ];
        Self {
            data: None,
            search: TextInput::new("search")
                .with_placeholder(tr.text("Buscar aplicación, runtime o herramienta…")),
            buckets: Vec::new(),
            scroll: 0,
            reveal: false,
            table: Table::new("apps", columns),
            details: TextView::new("details").wrapped(true),
            summary: String::new(),
            hint: None,
            empty: false,
            selected: None,
            can_install: false,
            installed: BTreeSet::new(),
        }
    }
}

impl App {
    /// `action_search` (Ctrl+F y «Buscar e instalar…»): abre el catálogo y enfoca la
    /// búsqueda.
    pub(super) fn search(&mut self) {
        self.page = Page::Catalog;
        self.load_catalog();
        self.pending_focus = Some("search".to_owned());
    }

    /// `load_catalog`: carga las entradas si hace falta y filtra.
    pub(super) fn load_catalog(&mut self) {
        if self.catalog.data.is_none() {
            match Catalog::load(&self.engine) {
                Ok(data) => self.catalog.data = Some(data),
                Err(error) => {
                    self.capture(&error, "catalog.load");
                    let text = self.describe_error(&error);
                    let line = self.tr.tr(&Message::new("Error: {p1}").arg("p1", &text));
                    self.write_log(line, true);
                    self.notify(text, crate::widgets::Severity::Error);
                }
            }
        }
        self.catalog.buckets = Catalog::buckets(&self.engine);
        self.filter_catalog();
    }

    /// `filter_catalog`: rellena la tabla según la búsqueda y los buckets.
    pub(super) fn filter_catalog(&mut self) {
        let installed: BTreeSet<String> = self
            .engine
            .installed()
            .map(|records| records.iter().map(|(id, _)| id.to_owned()).collect())
            .unwrap_or_default();
        let state = &mut self.catalog;
        state.installed = installed;
        state.selected = None;
        state.can_install = false;
        let Some(data) = &state.data else {
            state.table.set_rows(Vec::new());
            state.summary.clear();
            state.hint = None;
            state.empty = true;
            return;
        };
        let listing = data.filter(&state.search.value, &state.installed, &self.tr);
        state.table.set_rows(
            listing
                .rows
                .iter()
                .map(|row| {
                    TableRow::new(
                        row.key.clone(),
                        vec![
                            row.key.clone(),
                            row.version.clone(),
                            row.availability.clone(),
                            row.license.clone(),
                        ],
                    )
                })
                .collect(),
        );
        state.empty = listing.rows.is_empty();
        state.summary = listing.summary;
        state.hint = listing.empty_hint;
        state.details.set_text(&listing.details);
    }

    /// `on_data_table_row_highlighted`: selecciona la fila `key` y muestra su detalle.
    pub(super) fn select_row(&mut self, key: &str) {
        let state = &mut self.catalog;
        let installed = state.installed.contains(key);
        let details = state
            .data
            .as_ref()
            .and_then(|data| data.details(key, installed, &self.tr));
        if let Some(details) = details {
            state.details.set_text(&details.text);
            state.can_install = details.can_install;
            state.selected = Some(key.to_owned());
        }
    }

    /// Selecciona la fila bajo el cursor de la tabla (Intro o clic sobre ella).
    pub(super) fn select_cursor(&mut self) {
        if let Some(key) = self.catalog.table.highlighted().map(str::to_owned) {
            self.select_row(&key);
        }
    }

    /// Cambia la casilla del bucket `name` y guarda `catalog-filters.json`.
    pub(super) fn toggle_bucket(&mut self, name: &str) {
        let Some(data) = self.catalog.data.as_mut() else {
            return;
        };
        let enable = data.disabled().contains(name);
        match data.set_enabled(name, enable) {
            Ok(()) => self.filter_catalog(),
            Err(error) => {
                self.capture(&error, "catalog.filters");
                let text = self.describe_error(&error);
                self.notify(text, crate::widgets::Severity::Error);
            }
        }
    }

    /// Botones del catálogo. Devuelve `true` si el id era de esta página.
    pub(super) fn catalog_button(&mut self, id: &str) -> bool {
        match id {
            "install" => {
                let key = self.catalog.selected.clone();
                if let Some(key) = key
                    && self.catalog.can_install
                    && !self.busy.active
                {
                    self.request(Action::Install(key));
                }
            }
            "manifest" => self.open_manifest(),
            "refresh" => self.request(Action::Refresh),
            "back" => self.go_home(),
            _ => return false,
        }
        true
    }

    /// Dibuja el catálogo en `area`.
    pub(super) fn render_catalog(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        let tr = ui.tr;
        let title = Style::new()
            .fg(theme::TITLE)
            .bg(theme::SCREEN)
            .add_modifier(Modifier::BOLD);
        let mut stack = Stack::new(area);
        stack.text(frame, tr.text("BUSCAR E INSTALAR · CATÁLOGOS SCOOP"), title);
        stack.gap(1);
        let row = stack.take(1);
        self.catalog.search.render(frame, row, ui);
        stack.gap(1);
        let row = stack.take(1);
        self.render_filters(frame, row, ui);
        stack.gap(1);

        let summary = self.summary_lines();
        // Las líneas largas se ajustan: cuentan por las filas que ocupan.
        let width = usize::from(area.width).max(1);
        let rows: u16 = summary
            .iter()
            .map(|line| text::height(&plain(line), width))
            .sum();
        let height = rows.max(SUMMARY_MIN);
        let rect = stack.take(height);
        let base = Style::new().fg(theme::SUMMARY).bg(theme::SCREEN);
        frame.render_widget(
            Paragraph::new(summary)
                .style(base)
                .wrap(Wrap { trim: false }),
            rect,
        );

        let fixed = DETAILS_HEIGHT + 1 + ACTIONS_HEIGHT;
        let table = stack.take(stack.remaining_height().saturating_sub(fixed));
        self.catalog.table.render(frame, table, ui);
        let details = stack.take(DETAILS_HEIGHT);
        frame.buffer_mut().set_style(details, theme::details());
        let inner = Rect::new(
            details.x.saturating_add(1),
            details.y,
            details.width.saturating_sub(2),
            details.height,
        );
        self.catalog.details.render(frame, inner, ui);
        stack.gap(1);
        let bar = stack.take(ACTIONS_HEIGHT);
        let busy = self.busy.active;
        let buttons = [
            Button::new("install", tr.text("Instalar"))
                .variant(Variant::Success)
                .disabled(busy || !self.catalog.can_install || self.catalog.selected.is_none()),
            Button::new("manifest", tr.text("Manifiesto"))
                .disabled(self.catalog.selected.is_none()),
            Button::new("refresh", tr.text("Actualizar catálogos")).disabled(busy),
            Button::new("back", tr.text("Volver a Inicio")),
        ];
        // Cada botón ocupa lo que pide su etiqueta (con `[ ]`), no un reparto igual.
        let widths = buttons
            .iter()
            .map(|button| u16::try_from(text::width(&button.label) + 4).unwrap_or(u16::MAX))
            .map(Constraint::Length);
        let cells = Layout::horizontal(widths)
            .spacing(1)
            .flex(Flex::Start)
            .split(bar);
        for (button, cell) in buttons.iter().zip(cells.iter()) {
            button.render(frame, *cell, ui);
        }
    }

    /// Líneas del resumen: con la tabla vacía, la primera va en negrita azul y debajo
    /// el aviso «Sin componentes…».
    fn summary_lines(&self) -> Vec<Line<'static>> {
        let state = &self.catalog;
        let mut lines = Vec::new();
        for (index, line) in state.summary.lines().enumerate() {
            if state.empty && index == 0 {
                let style = Style::new()
                    .fg(theme::EMPTY_CATALOG)
                    .bg(theme::SCREEN)
                    .add_modifier(Modifier::BOLD);
                lines.push(Line::from(Span::styled(line.to_owned(), style)));
            } else {
                lines.push(Line::from(line.to_owned()));
            }
        }
        if let Some(hint) = state.hint.as_deref().filter(|_| state.empty) {
            lines.push(Line::from(hint.to_owned()));
        }
        lines
    }

    /// Casillas de bucket en una fila con desplazamiento horizontal.
    fn render_filters(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        ui.zone("bucket-filters", area);
        let busy = self.busy.active;
        let disabled = self
            .catalog
            .data
            .as_ref()
            .map(|data| data.disabled().clone())
            .unwrap_or_default();
        let names = self.catalog.buckets.clone();
        let width_of = |name: &String| u16::try_from(text::width(name) + 4).unwrap_or(u16::MAX);
        let mut scroll = self.catalog.scroll.min(names.len().saturating_sub(1));
        if self.catalog.reveal {
            self.catalog.reveal = false;
            let focused = ui
                .focused()
                .and_then(|id| id.strip_prefix("bucket-"))
                .and_then(|name| names.iter().position(|n| n == name));
            if let Some(index) = focused {
                scroll = scroll.min(index);
                while scroll < index
                    && names[scroll..=index]
                        .iter()
                        .map(|n| width_of(n).saturating_add(1))
                        .sum::<u16>()
                        > area.width
                {
                    scroll += 1;
                }
            }
        }
        self.catalog.scroll = scroll;
        let style = Style::new().fg(theme::TEXT).bg(theme::SCREEN);
        frame.buffer_mut().set_style(area, style);
        let mut x = area.x;
        for (index, name) in names.iter().enumerate() {
            let checkbox = Checkbox::new(
                format!("bucket-{name}"),
                name.clone(),
                !disabled.contains(name),
            )
            .disabled(busy)
            .style(style);
            let width = width_of(name);
            if index >= scroll && x.saturating_add(width) <= area.right() {
                checkbox.render(frame, Rect::new(x, area.y, width, 1), ui);
                x = x.saturating_add(width).saturating_add(1);
            } else if !checkbox.disabled {
                ui.focusable(&checkbox.id, Rect::ZERO);
            }
        }
    }
}

/// Texto plano de una línea.
fn plain(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
