//! Editor de `config.properties` o de las variables de un profile
//! (`ConfigurationDialog`).

use crossterm::event::KeyCode;
use mito_core::config::{Configuration, DEFAULT_SETTING, PREFIXES, PROPERTIES, Property, lookup};
use mito_core::error_tracking::{self, Options};
use mito_core::{Message, Result as CoreResult};
use mito_engine::Engine;
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::{Context, Dialog, Outcome, Step, describe_core, navigate};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::text;
use crate::widgets::{Button, Focus, Select, Stack, TextInput, Ui, Variant, WidgetEvent, header};

const CLOSE: &str = "configuration-close";
const KIND: &str = "configuration-kind";
const NAME: &str = "configuration-name";
const ADD: &str = "configuration-add";
const SAVE: &str = "configuration-save";

/// Propiedades que siempre aparecen en la configuración global (`setdefault`).
const ALWAYS: [&str; 3] = ["mito.language", "mcp.jobs.monitor", "terminal.defaultShell"];

/// Prefijo de las variables de entorno libres de `config.properties`.
const ENV: &str = "env.";

/// Ancho del botón «Eliminar» de cada fila.
const REMOVE_WIDTH: u16 = 12;

/// Campo del valor de una fila.
#[derive(Debug)]
enum Field {
    Text(TextInput),
    Choice(Select<String>),
}

/// Una fila del formulario: clave (editable si es libre) y valor.
#[derive(Debug)]
struct SettingRow {
    n: usize,
    key: String,
    free: bool,
    key_input: Option<TextInput>,
    value: Field,
    /// Valor guardado que no está en la lista permitida (se muestra como inválido).
    invalid: Option<String>,
}

impl SettingRow {
    fn new(n: usize, key: &str, value: &str, profile: bool) -> Self {
        let free = profile || key.starts_with(ENV);
        let key_input = free.then(|| {
            let shown = if profile {
                key
            } else {
                key.strip_prefix(ENV).unwrap_or(key)
            };
            TextInput::new(format!("setting-key-{n}")).with_value(shown)
        });
        let choices = if free {
            None
        } else {
            lookup(key).and_then(|property| property.choices)
        };
        let value_id = format!("setting-value-{n}");
        let (field, invalid) = match choices {
            Some(choices) => {
                let mut options: Vec<(String, String)> = choices
                    .iter()
                    .map(|choice| ((*choice).to_owned(), (*choice).to_owned()))
                    .collect();
                let lowered = value.to_lowercase();
                let found = choices.iter().position(|choice| *choice == lowered);
                let invalid = found.is_none().then(|| value.to_owned());
                if invalid.is_some() {
                    options.push((value.to_owned(), value.to_owned()));
                }
                let selected = found.unwrap_or(options.len() - 1);
                (
                    Field::Choice(Select::new(value_id, options, Some(selected))),
                    invalid,
                )
            }
            None => (
                Field::Text(TextInput::new(value_id).with_value(value)),
                None,
            ),
        };
        Self {
            n,
            key: key.to_owned(),
            free,
            key_input,
            value: field,
            invalid,
        }
    }

    fn key_id(&self) -> String {
        format!("setting-key-{}", self.n)
    }

    fn value_id(&self) -> String {
        format!("setting-value-{}", self.n)
    }

    fn remove_id(&self) -> String {
        format!("setting-remove-{}", self.n)
    }

    /// Clave tal como se guardaría: las libres, sin espacios y con `env.` en global.
    fn full_key(&self, profile: bool) -> String {
        match &self.key_input {
            Some(input) if profile => input.value.trim().to_owned(),
            Some(input) => format!("{ENV}{}", input.value.trim()),
            None => self.key.clone(),
        }
    }

    fn pair(&self, profile: bool) -> (String, String) {
        let value = match &self.value {
            Field::Text(input) => input.value.clone(),
            Field::Choice(select) => select.value().cloned().unwrap_or_default(),
        };
        (self.full_key(profile), value)
    }

    /// Etiquetas traducibles de las opciones: «Automático (Windows)» y valor no válido.
    fn relabel(&mut self, tr: &Translator) {
        let Field::Choice(select) = &mut self.value else {
            return;
        };
        let last = select.options.len().saturating_sub(1);
        let invalid = self.invalid.clone();
        for (index, (label, value)) in select.options.iter_mut().enumerate() {
            if self.key == "mito.language" && value == DEFAULT_SETTING {
                tr.text("Automático (Windows)").clone_into(label);
            } else if index == last && invalid.as_deref() == Some(value.as_str()) {
                *label = tr.tr(&Message::new("{p0} (valor no válido)").arg("p0", value.as_str()));
            }
        }
    }
}

/// «CONFIGURACIÓN»: formulario de filas `clave | valor | Eliminar`, selector y nombre
/// para añadir propiedades y «Guardar». Con `profile` edita las variables de entorno de
/// `profiles/<profile>.json`; sin él, `config.properties`. Se cierra con
/// [`Outcome::Saved`]: `true` solo si se guardó; Esc o `×` no escriben nada.
#[derive(Debug)]
pub struct ConfigurationDialog {
    focus: Focus,
    profile: Option<String>,
    settings: Configuration,
    rows: Vec<SettingRow>,
    next: usize,
    kind: Select<String>,
    name: TextInput,
    error: String,
    scroll: usize,
    page: usize,
    reveal_last: bool,
    last_focused: Option<String>,
}

impl ConfigurationDialog {
    /// Abre la configuración global (`profile` = `None`) o la del profile.
    ///
    /// # Errors
    ///
    /// Los de [`Configuration::global`] y [`Configuration::profile`]: archivo ilegible,
    /// JSON inválido, profile inexistente o variables no válidas.
    pub fn new(engine: &Engine, profile: Option<&str>) -> CoreResult<Self> {
        let settings = match profile {
            Some(profile) => Configuration::profile(engine.layout(), profile)?,
            None => Configuration::global(engine.layout())?,
        };
        let is_profile = profile.is_some();
        let mut entries: Vec<(String, String)> = settings
            .values()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        if !is_profile {
            for key in ALWAYS {
                if !entries.iter().any(|(existing, _)| existing == key) {
                    let default = lookup(key).map_or("", |property| property.default);
                    entries.push((key.to_owned(), default.to_owned()));
                }
            }
        }
        let rows: Vec<SettingRow> = entries
            .iter()
            .enumerate()
            .map(|(n, (key, value))| SettingRow::new(n, key, value, is_profile))
            .collect();
        let mut options = vec![("Variable de entorno".to_owned(), ENV.to_owned())];
        if !is_profile {
            options.extend(
                PROPERTIES
                    .iter()
                    .map(|p| (p.key.to_owned(), p.key.to_owned())),
            );
            options.extend(
                PREFIXES
                    .iter()
                    .map(|prefix| (format!("{prefix}<nombre>"), (*prefix).to_owned())),
            );
        }
        let mut kind = Select::new(KIND, options, Some(0));
        kind.disabled = is_profile;
        let first = rows.first().map(|row| {
            if row.free {
                row.key_id()
            } else {
                row.value_id()
            }
        });
        let focus = Focus::starting_at(&first.unwrap_or_else(|| NAME.to_owned()));
        Ok(Self {
            focus,
            profile: profile.map(str::to_owned),
            settings,
            next: rows.len(),
            rows,
            kind,
            name: TextInput::new(NAME),
            error: String::new(),
            scroll: 0,
            page: 1,
            reveal_last: false,
            last_focused: None,
        })
    }

    fn is_profile(&self) -> bool {
        self.profile.is_some()
    }

    fn kind_property(&self) -> Option<&'static Property> {
        self.kind.value().and_then(|kind| lookup(kind))
    }

    fn open_select(&self) -> Option<String> {
        if self.kind.open {
            return Some(KIND.to_owned());
        }
        self.rows.iter().find_map(|row| match &row.value {
            Field::Choice(select) if select.open => Some(row.value_id()),
            _ => None,
        })
    }

    fn select_mut(&mut self, id: &str) -> Option<&mut Select<String>> {
        if id == KIND {
            return Some(&mut self.kind);
        }
        self.rows.iter_mut().find_map(|row| match &mut row.value {
            Field::Choice(select) if select.id == id => Some(select),
            _ => None,
        })
    }

    fn add(&mut self, tr: &Translator) {
        let name = self.name.value.trim().to_owned();
        let kind = self.kind.value().cloned().unwrap_or_else(|| ENV.to_owned());
        let known = lookup(&kind);
        if known.is_none() && name.is_empty() {
            tr.text("Introduce el nombre de la propiedad o variable.")
                .clone_into(&mut self.error);
            return;
        }
        let key = if self.is_profile() {
            name
        } else if known.is_some() {
            kind
        } else {
            format!("{kind}{name}")
        };
        let profile = self.is_profile();
        let wanted = key.to_lowercase();
        if self
            .rows
            .iter()
            .any(|row| row.full_key(profile).to_lowercase() == wanted)
        {
            tr.text("Esta propiedad ya está añadida.")
                .clone_into(&mut self.error);
            return;
        }
        let value = if profile {
            ""
        } else {
            lookup(&key).map_or("", |property| property.default)
        };
        self.rows
            .push(SettingRow::new(self.next, &key, value, profile));
        self.next += 1;
        self.reveal_last = true;
        self.name.set_value("");
    }

    fn save(&mut self, context: &mut Context) -> Step {
        let profile = self.is_profile();
        let pairs: Vec<(String, String)> = self.rows.iter().map(|row| row.pair(profile)).collect();
        match self.settings.save(pairs) {
            Ok(()) => Step::Close(Outcome::Saved(true)),
            Err(error) => {
                error_tracking::capture(
                    context.engine.layout(),
                    &error,
                    &Options::operation("configuration.save"),
                );
                self.error = describe_core(&error, context.tr);
                Step::Stay
            }
        }
    }

    fn remove(&mut self, id: &str) {
        self.rows.retain(|row| row.remove_id() != id);
    }

    /// Ejecuta la acción del botón `id`.
    fn press(&mut self, id: &str, context: &mut Context) -> Step {
        self.error.clear();
        match id {
            CLOSE => Step::Close(Outcome::Saved(false)),
            ADD => {
                self.add(context.tr);
                Step::Stay
            }
            SAVE => self.save(context),
            other if other.starts_with("setting-remove-") => {
                self.remove(other);
                Step::Stay
            }
            _ => Step::Stay,
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        let max = self.rows.len().saturating_sub(self.page);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    /// Fila (índice) a la que pertenece un id de widget de fila.
    fn row_of(&self, id: &str) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.key_id() == id || row.value_id() == id || row.remove_id() == id)
    }

    fn click(&mut self, column: u16, row: u16, context: &mut Context) -> Step {
        let Some(id) = self.focus.hit(column, row) else {
            return Step::Stay;
        };
        self.focus.set(&id);
        if id == KIND {
            self.kind.click(column, row);
            return Step::Stay;
        }
        if id == NAME {
            self.name.click(column);
            return Step::Stay;
        }
        if let Some(index) = self.row_of(&id) {
            let target = &mut self.rows[index];
            if id == target.key_id() {
                if let Some(input) = target.key_input.as_mut() {
                    input.click(column);
                }
                return Step::Stay;
            }
            if id == target.value_id() {
                match &mut target.value {
                    Field::Text(input) => input.click(column),
                    Field::Choice(select) => {
                        select.click(column, row);
                    }
                }
                return Step::Stay;
            }
        }
        self.press(&id, context)
    }

    fn type_into(&mut self, id: &str, input: &Input, context: &mut Context) -> Step {
        if id == NAME {
            if self.kind_property().is_some() {
                return Step::Stay;
            }
            if let Some(WidgetEvent::Submitted(_)) = self.name.handle(input) {
                self.error.clear();
                self.add(context.tr);
            }
            return Step::Stay;
        }
        if id == KIND {
            self.kind.handle(input);
            return Step::Stay;
        }
        if let Some(index) = self.row_of(id) {
            let row = &mut self.rows[index];
            if id == row.key_id() {
                if let Some(field) = row.key_input.as_mut() {
                    field.handle(input);
                }
            } else if id == row.value_id() {
                match &mut row.value {
                    Field::Text(field) => {
                        field.handle(input);
                    }
                    Field::Choice(select) => {
                        select.handle(input);
                    }
                }
            } else if input.is_activation() {
                return self.press(id, context);
            }
            return Step::Stay;
        }
        if input.is_activation() {
            return self.press(id, context);
        }
        Step::Stay
    }
}

impl ConfigurationDialog {
    /// Mantiene a la vista la fila del widget enfocado cuando el foco cambia, y
    /// desplaza hasta el final tras añadir una fila.
    fn keep_visible(&mut self, ui: &Ui) {
        let max_scroll = self.rows.len().saturating_sub(self.page);
        if self.reveal_last {
            self.scroll = max_scroll;
            self.reveal_last = false;
        }
        let focused = ui.focused().map(str::to_owned);
        if focused != self.last_focused {
            if let Some(index) = focused.as_deref().and_then(|id| self.row_of(id)) {
                if index < self.scroll {
                    self.scroll = index;
                } else if index >= self.scroll + self.page {
                    self.scroll = index + 1 - self.page;
                }
            }
            self.last_focused = focused;
        }
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Dibuja las filas visibles; las demás solo entran en el orden de Tab.
    fn render_rows(&mut self, frame: &mut Frame, ui: &mut Ui, area: Rect) {
        let tr = ui.tr;
        let columns = Layout::horizontal([
            Constraint::Percentage(38),
            Constraint::Fill(1),
            Constraint::Length(REMOVE_WIDTH),
        ])
        .spacing(1);
        let remove_label = tr.text("Eliminar").to_owned();
        for (index, row) in self.rows.iter_mut().enumerate() {
            let visible = index >= self.scroll && index < self.scroll + self.page;
            if !visible {
                // Fuera de la vista siguen en el orden de Tab, sin zona pulsable.
                if row.key_input.is_some() {
                    ui.focusable(&row.key_id(), Rect::ZERO);
                }
                ui.focusable(&row.value_id(), Rect::ZERO);
                ui.focusable(&row.remove_id(), Rect::ZERO);
                continue;
            }
            let y = area.y + u16::try_from(index - self.scroll).unwrap_or(0);
            let line = Rect::new(area.x, y, area.width, 1);
            let [key_area, value_area, remove_area] = line.layout(&columns);
            if let Some(input) = row.key_input.as_mut() {
                tr.text("Variable").clone_into(&mut input.placeholder);
                input.render(frame, key_area, ui);
            } else {
                let shown = text::truncate(&row.key, usize::from(key_area.width));
                frame.render_widget(
                    Paragraph::new(Span::styled(shown, theme::panel())),
                    key_area,
                );
            }
            match &mut row.value {
                Field::Text(input) => {
                    tr.text("Valor").clone_into(&mut input.placeholder);
                    input.render(frame, value_area, ui);
                }
                Field::Choice(select) => select.render(frame, value_area, ui),
            }
            Button::new(row.remove_id(), remove_label.clone())
                .variant(Variant::Error)
                .render(frame, remove_area, ui);
        }
    }

    /// Dibuja la fila de añadir: tipo, nombre y «Añadir».
    fn render_add_row(&mut self, frame: &mut Frame, ui: &mut Ui, area: Rect) {
        let tr = ui.tr;
        if let Some(first) = self.kind.options.first_mut() {
            tr.text("Variable de entorno").clone_into(&mut first.0);
        }
        self.name.disabled = self.kind_property().is_some();
        let env = self.kind.value().is_some_and(|kind| kind == ENV);
        let placeholder = if env {
            "Nombre de la variable"
        } else {
            "Nombre"
        };
        tr.text(placeholder).clone_into(&mut self.name.placeholder);
        let columns = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(10),
        ])
        .spacing(1);
        let [kind_area, name_area, button_area] = area.layout(&columns);
        self.kind.render(frame, kind_area, ui);
        self.name.render(frame, name_area, ui);
        Button::new(ADD, tr.text("Añadir")).render(frame, button_area, ui);
    }
}

impl Dialog for ConfigurationDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let tr = ui.tr;
        let area = modal::area(screen, Size::PercentMax(95, 136), Size::Percent(90));
        let inner = modal::frame(frame, area, theme::BORDER);
        let mut stack = Stack::new(inner);
        let title = if self.profile.is_some() {
            "CONFIGURACIÓN DEL PROFILE"
        } else {
            "CONFIGURACIÓN"
        };
        header::render(frame, stack.take(header::HEIGHT), ui, title, CLOSE);
        let context = match &self.profile {
            Some(profile) => tr.tr(&Message::new(
                "Profile: {p1} · Variables que sobrescriben las globales.",
            )
            .arg("p1", profile)),
            None => tr
                .text("config.properties · Variables comunes a todos los profiles.")
                .to_owned(),
        };
        stack.text(frame, &context, theme::dim());
        stack.gap(1);
        // Zona inferior: añadir, error, hueco y guardar.
        let error_height = if self.error.is_empty() {
            0
        } else {
            text::height(&self.error, usize::from(inner.width))
        };
        let bottom = 1 + error_height + 1 + 1;
        let remaining = stack.remaining();
        let rows_area = Rect::new(
            remaining.x,
            remaining.y,
            remaining.width,
            remaining.height.saturating_sub(bottom + 1),
        );
        stack.take(rows_area.height + 1);
        let add_area = stack.take(1);
        let error_area = stack.take(error_height);
        stack.gap(1);
        let save_area = stack.take(1);

        for row in &mut self.rows {
            row.relabel(tr);
        }
        self.page = usize::from(rows_area.height).max(1);
        self.keep_visible(ui);
        self.render_rows(frame, ui, rows_area);
        self.render_add_row(frame, ui, add_area);
        if error_height > 0 {
            stack_text(frame, error_area, &self.error);
        }
        Button::new(SAVE, tr.text("Guardar"))
            .variant(Variant::Primary)
            .render(frame, save_area, ui);

        // Listas desplegables, encima de todo.
        for row in &self.rows {
            if let Field::Choice(select) = &row.value {
                select.render_overlay(frame, ui);
            }
        }
        self.kind.render_overlay(frame, ui);
    }

    fn handle(&mut self, input: &Input, context: &mut Context) -> Step {
        // Con una lista desplegada, ella recibe las entradas.
        if let Some(open) = self.open_select() {
            let hit = match input {
                Input::Click { column, row } => self.focus.hit(*column, *row),
                _ => None,
            };
            let closing = input.is_tab()
                || input.is_back_tab()
                || (matches!(input, Input::Click { .. }) && hit.as_deref() != Some(open.as_str()));
            if let Some(select) = self.select_mut(&open) {
                if closing {
                    select.close();
                } else {
                    if let Input::Click { column, row } = input {
                        select.click(*column, *row);
                    } else {
                        select.handle(input);
                    }
                    return Step::Stay;
                }
            }
        }
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Saved(false));
        }
        if navigate(&mut self.focus, input, false) {
            return Step::Stay;
        }
        let page = isize::try_from(self.page).unwrap_or(isize::MAX);
        match input {
            Input::Click { column, row } => return self.click(*column, *row, context),
            Input::Scroll { down, .. } => {
                self.scroll_by(if *down { 3 } else { -3 });
                return Step::Stay;
            }
            _ if input.is(KeyCode::PageUp) => {
                self.scroll_by(-page);
                return Step::Stay;
            }
            _ if input.is(KeyCode::PageDown) => {
                self.scroll_by(page);
                return Step::Stay;
            }
            _ => {}
        }
        let Some(id) = self.focus.focused().map(str::to_owned) else {
            return Step::Stay;
        };
        self.type_into(&id, input, context)
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

/// Texto de error en su área, en rojo.
fn stack_text(frame: &mut Frame, area: Rect, content: &str) {
    let mut stack = Stack::new(area);
    stack.text(frame, content, theme::error());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;
    use crossterm::event::KeyCode;
    use serde_json::json;

    const ORIGINAL: &str = "# Keep this comment\nenv.CUSTOM=old\n";

    fn write_config(harness: &Harness, content: &str) {
        std::fs::write(harness.dir.path().join("config.properties"), content).unwrap();
    }

    fn read_config(harness: &Harness) -> String {
        std::fs::read_to_string(harness.dir.path().join("config.properties")).unwrap()
    }

    fn global(harness: &Harness) -> ConfigurationDialog {
        ConfigurationDialog::new(&harness.engine, None).unwrap()
    }

    fn profile_instance(harness: &Harness) -> ConfigurationDialog {
        harness.engine.create_profile("one", None).unwrap();
        ConfigurationDialog::new(&harness.engine, Some("one")).unwrap()
    }

    #[test]
    fn aspecto_global() {
        let harness = Harness::new();
        write_config(&harness, ORIGINAL);
        let mut dialog = global(&harness);
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn aspecto_de_un_profile() {
        let harness = Harness::spanish();
        let mut dialog = profile_instance(&harness);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("CONFIGURACIÓN DEL PROFILE"), "{text}");
        assert!(
            text.contains("Profile: one · Variables que sobrescriben las globales."),
            "{text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn las_tres_propiedades_fijas_siempre_aparecen() {
        let harness = Harness::new();
        let mut dialog = global(&harness);
        let text = harness.draw(&mut dialog, 100, 30);
        for key in ALWAYS {
            assert!(text.contains(key), "{key}: {text}");
        }
        assert!(text.contains("Automatic (Windows)"), "{text}");
    }

    #[test]
    fn esc_no_escribe_nada() {
        let mut harness = Harness::new();
        write_config(&harness, ORIGINAL);
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        harness.type_text(&mut dialog, "algo");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Saved(false))
        );
        assert_eq!(read_config(&harness), ORIGINAL);
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Saved(false))
        );
        assert_eq!(read_config(&harness), ORIGINAL);
    }

    #[test]
    fn anadir_borrar_y_guardar_conserva_el_comentario() {
        let mut harness = Harness::new();
        write_config(&harness, ORIGINAL);
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        // Quita la fila `env.CUSTOM` (la primera).
        harness.click(&mut dialog, "setting-remove-0");
        // Añade `env.NEW` y le da valor.
        harness.focus_on(&mut dialog, NAME);
        harness.type_text(&mut dialog, "NEW");
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("NEW"), "{text}");
        harness.focus_on(&mut dialog, "setting-value-4");
        harness.type_text(&mut dialog, "a=b");
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, SAVE),
            Step::Close(Outcome::Saved(true))
        );
        let saved = read_config(&harness);
        assert!(saved.starts_with("# Keep this comment\n"), "{saved}");
        assert!(saved.contains("env.NEW=a=b"), "{saved}");
        assert!(!saved.contains("CUSTOM"), "{saved}");
        assert!(saved.contains("mito.language=system"), "{saved}");
        assert!(saved.contains("mcp.jobs.monitor=false"), "{saved}");
        assert!(saved.contains("terminal.defaultShell=cmd"), "{saved}");
    }

    #[test]
    fn el_selector_de_shell_se_guarda() {
        let mut harness = Harness::new();
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        let shell = (0..4)
            .map(|n| format!("setting-value-{n}"))
            .find(|id| {
                dialog
                    .rows
                    .iter()
                    .any(|row| row.value_id() == *id && row.key == "terminal.defaultShell")
            })
            .unwrap();
        harness.focus_on(&mut dialog, &shell);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, SAVE);
        assert!(read_config(&harness).contains("terminal.defaultShell=powershell"));
    }

    #[test]
    fn nombre_vacio_y_clave_repetida_muestran_su_error() {
        let mut harness = Harness::spanish();
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("Introduce el nombre de la propiedad o variable."),
            "{text}"
        );
        harness.focus_on(&mut dialog, NAME);
        harness.type_text(&mut dialog, "uno");
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, NAME);
        harness.type_text(&mut dialog, "UNO");
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("Esta propiedad ya está añadida."), "{text}");
        // Pulsar otro botón limpia el error.
        harness.click(&mut dialog, "setting-remove-3");
        assert!(
            !harness
                .draw(&mut dialog, 100, 30)
                .contains("ya está añadida")
        );
    }

    #[test]
    fn el_selector_de_tipo_anade_una_propiedad_conocida() {
        let mut harness = Harness::new();
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, KIND);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.type_text(&mut dialog, "");
        // Segunda opción: `terminal.defaultShell` no; la primera propiedad de la lista.
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        let chosen = dialog.kind.value().cloned().unwrap();
        assert_eq!(chosen, PROPERTIES[0].key);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(!dialog.focus().is_registered(NAME), "{text}");
        harness.click(&mut dialog, ADD);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("This property has already been added."),
            "{text}"
        );
    }

    #[test]
    fn un_valor_no_valido_se_muestra_y_se_rechaza_al_guardar() {
        let mut harness = Harness::spanish();
        write_config(&harness, "terminal.defaultShell=zsh\n");
        let mut dialog = global(&harness);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("zsh (valor no válido)"), "{text}");
        harness.click(&mut dialog, SAVE);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("terminal.defaultShell: elige uno de los valores permitidos"),
            "{text}"
        );
        assert_eq!(read_config(&harness), "terminal.defaultShell=zsh\n");
    }

    #[test]
    fn una_variable_reservada_en_un_profile_muestra_el_error() {
        let mut harness = Harness::spanish();
        let mut dialog = profile_instance(&harness);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, NAME);
        harness.type_text(&mut dialog, "PATH");
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(harness.click(&mut dialog, SAVE), Step::Stay);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(text.contains("reservada"), "{text}");
    }

    #[test]
    fn guardar_un_profile_solo_cambia_env() {
        let mut harness = Harness::new();
        let mut dialog = profile_instance(&harness);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, NAME);
        harness.type_text(&mut dialog, "CUSTOM");
        harness.draw(&mut dialog, 100, 30);
        harness.click(&mut dialog, ADD);
        harness.draw(&mut dialog, 100, 30);
        harness.focus_on(&mut dialog, "setting-value-0");
        harness.type_text(&mut dialog, " value ");
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.click(&mut dialog, SAVE),
            Step::Close(Outcome::Saved(true))
        );
        let data: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(harness.dir.path().join("profiles/one.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(data["env"], json!({"CUSTOM": " value "}));
        assert!(!harness.dir.path().join("config.properties").exists());
    }

    #[test]
    fn el_tipo_esta_deshabilitado_en_un_profile() {
        let harness = Harness::new();
        let mut dialog = profile_instance(&harness);
        harness.draw(&mut dialog, 100, 30);
        assert!(!dialog.focus().is_registered(KIND));
    }

    #[test]
    fn el_archivo_que_cambia_fuera_del_dialogo_no_se_pisa() {
        let mut harness = Harness::new();
        write_config(&harness, ORIGINAL);
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 30);
        write_config(&harness, "# otro cambio\n");
        assert_eq!(harness.click(&mut dialog, SAVE), Step::Stay);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("La configuración cambió fuera del diálogo") || text.contains("changed"),
            "{text}"
        );
        assert_eq!(read_config(&harness), "# otro cambio\n");
    }

    #[test]
    fn una_lista_larga_se_desplaza_y_tab_alcanza_las_filas_ocultas() {
        let mut harness = Harness::new();
        let content = (0..40).fold(String::new(), |acc, i| acc + &format!("env.VAR{i}={i}\n"));
        write_config(&harness, &content);
        let mut dialog = global(&harness);
        harness.draw(&mut dialog, 100, 20);
        assert!(dialog.focus().is_registered("setting-value-39"));
        harness.focus_on(&mut dialog, "setting-value-39");
        let text = harness.draw(&mut dialog, 100, 20);
        assert!(text.contains("VAR39") || text.contains("39"), "{text}");
        assert!(dialog.scroll > 0);
    }
}
