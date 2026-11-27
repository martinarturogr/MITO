//! Página «Inicio»: componentes instalados (`refresh_home` y sus botones).

use std::collections::BTreeMap;

use mito_catalog::Manifest;
use mito_core::Message;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use super::dialogs::ComponentDialog;
use super::state::{App, Page};
use crate::model::{self, Home, HomeRow};
use crate::theme;
use crate::widgets::text;
use crate::widgets::{Button, Checkbox, Stack, Ui, Variant};
use crate::worker::Action;

/// Filas que ocupa un componente: una de contenido y una de separación.
const ROW_HEIGHT: u16 = 2;
/// Filas de los botones de acción de la página.
const ACTIONS_HEIGHT: u16 = 3;

/// Estado de Inicio.
#[derive(Debug, Default)]
pub(super) struct HomeState {
    /// Último cálculo de Inicio.
    pub data: Option<Home>,
    /// Valor mostrado de cada casilla (`checkbox_states`).
    pub shown: Vec<bool>,
    /// Primera fila visible de la lista.
    pub scroll: usize,
    /// Filas que caben en la lista (último fotograma).
    pub visible: usize,
    /// Hay que desplazar la lista hasta el widget enfocado.
    pub reveal: bool,
}

impl HomeState {
    /// Desplaza la lista `delta` filas (la rueda).
    pub fn scroll_by(&mut self, delta: isize) {
        let count = self.data.as_ref().map_or(0, |home| home.rows.len());
        let max = count.saturating_sub(self.visible.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }
}

/// Número de fila de un id `{prefijo}-{n}` de la lista.
pub(super) fn row_of(id: &str) -> Option<usize> {
    let (prefix, number) = id.split_once('-')?;
    matches!(prefix, "active" | "open" | "info" | "update" | "remove")
        .then(|| number.parse().ok())
        .flatten()
}

/// Número de fila de un id con el prefijo dado.
fn indexed(id: &str, prefix: &str) -> Option<usize> {
    id.strip_prefix(prefix)?.parse().ok()
}

fn catalog_manifests(
    engine: &mito_engine::Engine,
) -> mito_engine::Result<BTreeMap<String, Manifest>> {
    Ok(engine.catalog()?.all()?.into_iter().collect())
}

impl App {
    /// `refresh_home`: vuelve a calcular la lista de componentes.
    pub(super) fn refresh_home(&mut self) {
        let profile = self.current_profile();
        // Un catálogo ilegible no impide ver lo instalado (solo faltan las
        // actualizaciones).
        let catalog = catalog_manifests(&self.engine).unwrap_or_default();
        match model::home(&self.engine, profile.as_deref(), &catalog) {
            Ok(home) => {
                self.home.shown = home.rows.iter().map(|row| row.active).collect();
                self.home.scroll = self.home.scroll.min(home.rows.len().saturating_sub(1));
                self.home.data = Some(home);
            }
            Err(error) => {
                self.capture(&error, "home.refresh");
                let text = self.describe_error(&error);
                let line = self.tr.tr(&Message::new("Error: {p1}").arg("p1", text));
                self.write_log(line, true);
            }
        }
    }

    /// Dibuja Inicio en `area`.
    pub(super) fn render_home(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        let title = Style::new()
            .fg(theme::TITLE)
            .bg(theme::SCREEN)
            .add_modifier(Modifier::BOLD);
        let mut stack = Stack::new(area);
        stack.text(frame, ui.tr.text("MIS COMPONENTES"), title);
        stack.gap(1);
        let profile = self.current_profile();
        let summary = self
            .home
            .data
            .as_ref()
            .map(|home| model::summary(home, profile.as_deref(), ui.tr))
            .unwrap_or_default();
        let rect = stack.take(3);
        let style = Style::new().fg(theme::SUMMARY).bg(theme::SCREEN);
        frame.render_widget(Paragraph::new(summary).style(style), rect);

        let reserved = ACTIONS_HEIGHT + 1;
        let list = stack.take(stack.remaining_height().saturating_sub(reserved));
        self.render_list(frame, list, ui);
        stack.gap(1);
        let bar = stack.take(ACTIONS_HEIGHT);

        let pending = self.home.data.as_ref().map_or(0, |home| home.pending.len());
        let columns = if pending > 1 { 2 } else { 1 };
        let cells = Layout::horizontal(vec![Constraint::Fill(1); columns])
            .spacing(1)
            .split(bar);
        Button::new("browse", ui.tr.text("Buscar e instalar nuevos componentes"))
            .variant(Variant::Primary)
            .render(frame, cells[0], ui);
        if pending > 1
            && let Some(data) = &self.home.data
        {
            Button::new("bulk-update", model::bulk_update_label(data, ui.tr))
                .variant(Variant::Success)
                .disabled(self.busy.active)
                .render(frame, cells[1], ui);
        }
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        ui.zone("installed-list", area);
        let Some(data) = self.home.data.take() else {
            return;
        };
        if data.rows.is_empty() {
            let style = Style::new().fg(theme::TEXT).bg(theme::SCREEN);
            frame.render_widget(Paragraph::new(model::empty_text(ui.tr)).style(style), area);
            self.home.data = Some(data);
            return;
        }
        let visible = ((usize::from(area.height) + 1) / usize::from(ROW_HEIGHT)).max(1);
        self.home.visible = visible;
        let count = data.rows.len();
        let mut scroll = self.home.scroll.min(count.saturating_sub(visible));
        if self.home.reveal {
            self.home.reveal = false;
            if let Some(index) = ui.focused().and_then(row_of) {
                if index < scroll {
                    scroll = index;
                } else if index >= scroll + visible {
                    scroll = index + 1 - visible;
                }
            }
        }
        self.home.scroll = scroll;
        let busy = self.busy.active;
        let has_profile = self.profile.value().is_some();
        for (index, row) in data.rows.iter().enumerate() {
            let place = index
                .checked_sub(scroll)
                .filter(|line| *line < visible)
                .and_then(|line| u16::try_from(line).ok())
                .map(|line| Rect::new(area.x, area.y + line * ROW_HEIGHT, area.width, 1));
            let shown = self.home.shown.get(index).copied().unwrap_or(row.active);
            render_row(frame, ui, index, row, shown, place, busy, has_profile);
        }
        self.home.data = Some(data);
    }

    /// Cambia la casilla de la fila `index` (`on_checkbox_changed`).
    pub(super) fn toggle_row(&mut self, index: usize) {
        if self.busy.active {
            return;
        }
        let Some(row) = self
            .home
            .data
            .as_ref()
            .and_then(|home| home.rows.get(index))
        else {
            return;
        };
        let (installation, membership) = (row.installation.clone(), row.active);
        let Some(shown) = self.home.shown.get_mut(index) else {
            return;
        };
        *shown = !*shown;
        let value = *shown;
        if membership != value {
            self.request(Action::Toggle {
                installation,
                active: value,
            });
        }
    }

    /// Botones de Inicio. Devuelve `true` si el id era de esta página.
    pub(super) fn home_button(&mut self, id: &str) -> bool {
        let installation = |app: &Self, index: usize| {
            app.home
                .data
                .as_ref()
                .and_then(|home| home.rows.get(index))
                .map(|row| row.installation.clone())
        };
        if id == "browse" {
            self.search();
        } else if id == "bulk-update" {
            let pending = self
                .home
                .data
                .as_ref()
                .map(|home| home.pending.clone())
                .filter(|pending| pending.len() > 1);
            if let Some(pending) = pending {
                self.request(Action::UpdateAll(pending));
            }
        } else if let Some(index) = indexed(id, "info-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::Info, &installation);
            }
        } else if let Some(index) = indexed(id, "remove-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::Remove, &installation);
            }
        } else if let Some(index) = indexed(id, "update-") {
            if let Some(installation) = installation(self, index) {
                self.request(Action::UpdateComponent(installation));
            }
        } else if let Some(index) = indexed(id, "open-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::Open, &installation);
            }
        } else {
            return false;
        }
        true
    }

    /// `action_home`: vuelve a Inicio.
    pub(super) fn go_home(&mut self) {
        self.page = Page::Home;
    }
}

/// Dibuja una fila (`place` es `None` si está fuera de la vista: solo se registran sus
/// widgets para que Tab pueda llegar a ellos).
#[allow(clippy::too_many_arguments, reason = "datos de una fila dibujada")]
fn render_row(
    frame: &mut Frame,
    ui: &mut Ui,
    index: usize,
    row: &HomeRow,
    shown: bool,
    place: Option<Rect>,
    busy: bool,
    has_profile: bool,
) {
    let tr = ui.tr;
    let background = if index.is_multiple_of(2) {
        theme::ROW_DARK
    } else {
        theme::ROW_LIGHT
    };
    let style = Style::new().fg(theme::TEXT).bg(background);
    let checkbox = Checkbox::new(format!("active-{index}"), model::row_label(row, tr), shown)
        .disabled(!has_profile || busy)
        .style(style);
    let mut buttons = Vec::new();
    if row.active && row.can_open {
        buttons.push(Button::new(format!("open-{index}"), tr.text("Abrir")).disabled(busy));
    }
    buttons.push(Button::new(format!("info-{index}"), tr.text("Info")).disabled(busy));
    if row.update.is_some() {
        buttons.push(
            Button::new(format!("update-{index}"), tr.text("Actualizar"))
                .variant(Variant::Success)
                .disabled(busy || !row.update_is_compatible()),
        );
    }
    buttons.push(Button::new(format!("remove-{index}"), tr.text("Eliminar")).disabled(busy));

    let Some(place) = place else {
        if !checkbox.disabled {
            ui.focusable(&checkbox.id, Rect::ZERO);
        }
        for button in buttons.iter().filter(|button| !button.disabled) {
            ui.focusable(&button.id, Rect::ZERO);
        }
        return;
    };
    frame.buffer_mut().set_style(place, style);
    let inner = Rect::new(
        place.x.saturating_add(1),
        place.y,
        place.width.saturating_sub(2),
        1,
    );
    let widths: Vec<u16> = buttons
        .iter()
        .map(|button| u16::try_from(text::width(&button.label) + 4).unwrap_or(u16::MAX))
        .collect();
    let used = widths.iter().map(|w| w.saturating_add(1)).sum::<u16>();
    let box_width = inner.width.saturating_sub(used);
    checkbox.render(frame, Rect::new(inner.x, inner.y, box_width, 1), ui);
    let mut x = inner.x + box_width;
    for (button, width) in buttons.iter().zip(widths) {
        x = x.saturating_add(1);
        let width = width.min(inner.right().saturating_sub(x));
        button.render(frame, Rect::new(x, inner.y, width, 1), ui);
        x = x.saturating_add(width);
    }
}
