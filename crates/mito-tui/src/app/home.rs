//! Página «Inicio»: componentes instalados y aplicaciones del host (`refresh_home` y sus
//! botones).

use std::collections::BTreeMap;

use mito_catalog::Manifest;
use mito_core::Message;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use super::dialogs::ComponentDialog;
use super::state::{App, Page};
use crate::model::{self, Home, HomeRow, HostRow};
use crate::theme;
use crate::widgets::text;
use crate::widgets::{Button, Checkbox, Stack, Ui, Variant};
use crate::worker::Action;

/// Filas que ocupa un elemento: el nombre y dos líneas de botones.
const ROW_HEIGHT: u16 = 3;
/// Filas de un botón de acción de la página.
const ACTION_HEIGHT: u16 = 3;

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
    /// Elementos de la lista: componentes y después aplicaciones del host.
    fn count(&self) -> usize {
        self.data
            .as_ref()
            .map_or(0, |home| home.rows.len() + home.host.len())
    }

    /// Desplaza la lista `delta` filas (la rueda).
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.count().saturating_sub(self.visible.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    /// Posición en la lista del widget `id`, si es de una fila.
    pub fn item_of(&self, id: &str) -> Option<usize> {
        if let Some(index) = row_of(id) {
            return Some(index);
        }
        let home = self.data.as_ref()?;
        let identity = host_of(id)?.1;
        let position = home.host.iter().position(|row| row.identity == identity)?;
        Some(home.rows.len() + position)
    }
}

/// Prefijos de los ids de una fila de componente: `{prefijo}-{n}`.
const ROW_PREFIXES: [&str; 9] = [
    "active", "open", "link", "openwith", "protocol", "expose", "info", "update", "remove",
];

/// Número de fila de un id `{prefijo}-{n}` de un componente.
pub(super) fn row_of(id: &str) -> Option<usize> {
    let (prefix, number) = id.split_once('-')?;
    ROW_PREFIXES
        .contains(&prefix)
        .then(|| number.parse().ok())
        .flatten()
}

/// Acción e identidad de un id `host-{acción}-{identidad}` de una aplicación del host.
pub(super) fn host_of(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("host-")?;
    let (action, identity) = rest.split_once('-')?;
    matches!(
        action,
        "open" | "cache" | "data" | "link" | "info" | "remove"
    )
    .then_some((action, identity))
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

/// Lo que se dibuja a la izquierda del nombre de una fila.
enum Head {
    /// Casilla de activación de un componente.
    Check(Checkbox),
    /// Nombre de una aplicación del host.
    Name(String),
}

/// Una fila de la lista de Inicio.
struct Item {
    head: Head,
    buttons: Vec<Button>,
    /// Fondo de la fila (alterna con el índice; el host usa otra gama).
    background: ratatui::style::Color,
}

impl App {
    /// `refresh_home`: vuelve a calcular la lista de componentes y de aplicaciones del
    /// host.
    pub(super) fn refresh_home(&mut self) {
        let profile = self.current_profile();
        // Un catálogo ilegible no impide ver lo instalado (solo faltan las
        // actualizaciones).
        let catalog = catalog_manifests(&self.engine).unwrap_or_default();
        match model::home(&self.engine, profile.as_deref(), &catalog) {
            Ok(mut home) => {
                if let Err(error) = model::host_state(
                    &mut home,
                    &self.engine,
                    profile.as_deref(),
                    &self.integration,
                ) {
                    self.capture(&error, "home.host");
                    let text = self.describe_error(&error);
                    let line = self.tr.tr(&Message::new("Error: {p1}").arg("p1", text));
                    self.write_log(line, true);
                }
                self.home.shown = home.rows.iter().map(|row| row.active).collect();
                let items = home.rows.len() + home.host.len();
                self.home.scroll = self.home.scroll.min(items.saturating_sub(1));
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

    /// Botones de acción de la página, con su ancho natural.
    fn home_actions(&self, tr: &mito_i18n::Translator) -> Vec<Button> {
        let mut buttons = vec![
            Button::new("browse", tr.text("Buscar e instalar nuevos componentes"))
                .variant(Variant::Primary),
            Button::new("host-add", tr.text("Agregar aplicación del host"))
                .disabled(self.busy.active),
        ];
        if let Some(data) = &self.home.data
            && data.pending.len() > 1
        {
            buttons.push(
                Button::new("bulk-update", model::bulk_update_label(data, tr))
                    .variant(Variant::Success)
                    .disabled(self.busy.active),
            );
        }
        buttons
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

        let actions = self.home_actions(ui.tr);
        let widths: Vec<u16> = actions
            .iter()
            .map(|button| button_width(&button.label))
            .collect();
        let lines = flow(&widths, area.width);
        let bar_height = ACTION_HEIGHT
            * u16::try_from(lines.last().map_or(0, |(line, _)| line + 1)).unwrap_or(1);
        let list = stack.take(stack.remaining_height().saturating_sub(bar_height + 1));
        self.render_list(frame, list, ui);
        stack.gap(1);
        let bar = stack.take(bar_height);
        for (button, (line, x)) in actions.iter().zip(lines) {
            let width = button_width(&button.label).min(bar.width.saturating_sub(x));
            let y = bar.y + u16::try_from(line).unwrap_or(0) * ACTION_HEIGHT;
            button.render(frame, Rect::new(bar.x + x, y, width, ACTION_HEIGHT), ui);
        }
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        ui.zone("installed-list", area);
        let Some(data) = self.home.data.take() else {
            return;
        };
        let count = data.rows.len() + data.host.len();
        if count == 0 {
            let style = Style::new().fg(theme::TEXT).bg(theme::SCREEN);
            frame.render_widget(Paragraph::new(model::empty_text(ui.tr)).style(style), area);
            self.home.data = Some(data);
            return;
        }
        let visible = usize::from(area.height / ROW_HEIGHT).max(1);
        self.home.visible = visible;
        let mut scroll = self.home.scroll.min(count.saturating_sub(visible));
        if self.home.reveal {
            self.home.reveal = false;
            if let Some(index) = ui.focused().and_then(|id| self.home.item_of(id)) {
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
        let items: Vec<Item> = data
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let shown = self.home.shown.get(index).copied().unwrap_or(row.active);
                component_item(ui, index, row, shown, busy, has_profile)
            })
            .chain(data.host.iter().enumerate().map(|(offset, row)| {
                host_item(ui, data.rows.len() + offset, row, busy, has_profile)
            }))
            .collect();
        for (index, item) in items.iter().enumerate() {
            let place = index
                .checked_sub(scroll)
                .filter(|line| *line < visible)
                .and_then(|line| u16::try_from(line).ok())
                .map(|line| Rect::new(area.x, area.y + line * ROW_HEIGHT, area.width, ROW_HEIGHT));
            render_item(frame, ui, item, place);
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
        } else if id == "host-add" {
            self.request(Action::HostAdd);
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
        } else if let Some((action, identity)) = host_of(id) {
            let (action, identity) = (action.to_owned(), identity.to_owned());
            self.host_button(&action, &identity);
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
        } else if let Some(index) = indexed(id, "link-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::Shortcut, &installation);
            }
        } else if let Some(index) = indexed(id, "openwith-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::OpenWith, &installation);
            }
        } else if let Some(index) = indexed(id, "protocol-") {
            if let Some(installation) = installation(self, index) {
                self.open_component_dialog(ComponentDialog::Protocols, &installation);
            }
        } else if let Some(index) = indexed(id, "expose-") {
            if let Some(installation) = installation(self, index) {
                self.request(Action::HostExpose(installation));
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

/// Ancho de un botón con su etiqueta entre corchetes.
fn button_width(label: &str) -> u16 {
    u16::try_from(text::width(label) + 4).unwrap_or(u16::MAX)
}

/// Reparte botones de los anchos dados en líneas de `width` celdas separados por una
/// celda. Devuelve, para cada uno, `(línea, columna)`.
fn flow(widths: &[u16], width: u16) -> Vec<(usize, u16)> {
    let mut line = 0;
    let mut x = 0u16;
    let mut places = Vec::new();
    for &button in widths {
        if x > 0 && x.saturating_add(button) > width {
            line += 1;
            x = 0;
        }
        places.push((line, x));
        x = x.saturating_add(button).saturating_add(1);
    }
    places
}

/// Fila de un componente instalado.
fn component_item(
    ui: &Ui,
    index: usize,
    row: &HomeRow,
    shown: bool,
    busy: bool,
    has_profile: bool,
) -> Item {
    let tr = ui.tr;
    let background = if index.is_multiple_of(2) {
        theme::ROW_DARK
    } else {
        theme::ROW_LIGHT
    };
    let style = Style::new().fg(theme::TEXT).bg(background);
    let check = Checkbox::new(format!("active-{index}"), model::row_label(row, tr), shown)
        .disabled(!has_profile || busy)
        .style(style);
    let mut buttons = Vec::new();
    if row.active && row.can_open {
        let extensions = format!("{} ", tr.text("Extensiones"));
        buttons.push(Button::new(format!("open-{index}"), tr.text("Abrir")).disabled(busy));
        buttons.push(Button::new(format!("link-{index}"), tr.text("Acc. directo")).disabled(busy));
        buttons.push(
            Button::new(
                format!("openwith-{index}"),
                model::state_label(&extensions, row.open_with),
            )
            .disabled(busy),
        );
        buttons
            .push(Button::new(format!("protocol-{index}"), tr.text("Protocolos")).disabled(busy));
    }
    if row.can_open {
        buttons.push(
            Button::new(
                format!("expose-{index}"),
                model::state_label(tr.text("Host · "), row.exposed),
            )
            .disabled(busy),
        );
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
    Item {
        head: Head::Check(check),
        buttons,
        background,
    }
}

/// Fila de una aplicación del host.
fn host_item(ui: &Ui, index: usize, row: &HostRow, busy: bool, has_profile: bool) -> Item {
    let tr = ui.tr;
    let id = &row.identity;
    let background = if index.is_multiple_of(2) {
        theme::HOST_DARK
    } else {
        theme::HOST_LIGHT
    };
    let buttons = vec![
        Button::new(format!("host-open-{id}"), tr.text("Abrir"))
            .disabled(busy || !row.available || !has_profile),
        Button::new(
            format!("host-cache-{id}"),
            model::state_label(tr.text("Cache · "), row.cache),
        )
        .disabled(busy),
        Button::new(
            format!("host-data-{id}"),
            model::state_label(tr.text("Data · "), row.data),
        )
        .disabled(busy),
        Button::new(format!("host-link-{id}"), tr.text("Acc. directo")).disabled(busy),
        Button::new(format!("host-info-{id}"), tr.text("Info")).disabled(busy),
        Button::new(format!("host-remove-{id}"), tr.text("Eliminar")).disabled(busy),
    ];
    Item {
        head: Head::Name(model::host_label(row, tr)),
        buttons,
        background,
    }
}

/// Dibuja una fila (`place` es `None` si está fuera de la vista: solo se registran sus
/// widgets para que Tab pueda llegar a ellos).
fn render_item(frame: &mut Frame, ui: &mut Ui, item: &Item, place: Option<Rect>) {
    let Some(place) = place else {
        if let Head::Check(check) = &item.head
            && !check.disabled
        {
            ui.focusable(&check.id, Rect::ZERO);
        }
        for button in item.buttons.iter().filter(|button| !button.disabled) {
            ui.focusable(&button.id, Rect::ZERO);
        }
        return;
    };
    let style = Style::new().fg(theme::TEXT).bg(item.background);
    frame.buffer_mut().set_style(place, style);
    let inner = Rect::new(
        place.x.saturating_add(1),
        place.y,
        place.width.saturating_sub(2),
        place.height,
    );
    let head = Rect::new(inner.x, inner.y, inner.width, 1);
    match &item.head {
        Head::Check(check) => check.render(frame, head, ui),
        Head::Name(name) => frame.render_widget(
            Paragraph::new(text::truncate(name, usize::from(head.width)))
                .style(style.add_modifier(Modifier::BOLD)),
            head,
        ),
    }
    let widths: Vec<u16> = item
        .buttons
        .iter()
        .map(|b| button_width(&b.label))
        .collect();
    for (button, (line, x)) in item.buttons.iter().zip(flow(&widths, inner.width)) {
        let y = inner.y + 1 + u16::try_from(line).unwrap_or(u16::MAX);
        if y >= place.bottom() {
            // No caben más líneas: se registra sin zona para que Tab llegue a él.
            if !button.disabled {
                ui.focusable(&button.id, Rect::ZERO);
            }
            continue;
        }
        let width = button_width(&button.label).min(inner.width.saturating_sub(x));
        button.render(frame, Rect::new(inner.x + x, y, width, 1), ui);
    }
}
