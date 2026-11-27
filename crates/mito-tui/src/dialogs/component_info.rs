//! Información de un componente instalado (`ComponentInfoDialog`).

use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use mito_core::error_tracking::{self, Options};
use mito_core::persistence;
use mito_core::storage::inside;
use mito_core::{Message, Result as CoreResult};
use mito_engine::{Engine, Record, isolation};
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use serde_json::Value;

use super::{Context, Dialog, Outcome, Step, describe_core, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::text;
use crate::widgets::{Button, Focus, Notice, Severity, Stack, Ui, header};

const CLOSE: &str = "info-close";
const RELEASE: &str = "info-release-version";

/// Ancho mínimo de un botón «Abrir» o «Liberar».
const BUTTON_WIDTH: u16 = 12;

type Opener = Box<dyn Fn(&Path) -> CoreResult<()> + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Directory,
    File,
    Missing,
}

impl Status {
    fn of(path: &Path) -> Self {
        if path.is_dir() {
            Self::Directory
        } else if path.is_file() {
            Self::File
        } else {
            Self::Missing
        }
    }

    /// Texto que se añade tras la ruta (clave de i18n con su salto de línea).
    fn note(self) -> Option<&'static str> {
        match self {
            Self::Directory => None,
            Self::File => Some("\nArchivo: se abrirá su carpeta"),
            Self::Missing => Some("\nTodavía no creada"),
        }
    }
}

#[derive(Debug, Clone)]
enum Scope {
    Profile(String),
    SelectProfile,
    Shared,
}

#[derive(Debug, Clone)]
enum Label {
    BaseBinaries,
    ProfileBinaries,
    Persistence(String),
}

#[derive(Debug, Clone)]
struct PathRow {
    label: Label,
    path: PathBuf,
    status: Status,
}

#[derive(Debug, Clone)]
struct EnvRow {
    name: String,
    value: String,
    path: Option<(PathBuf, Status)>,
}

#[derive(Debug, Clone)]
struct Environment {
    variables: Vec<EnvRow>,
    paths: Vec<EnvRow>,
}

#[derive(Debug, Clone)]
struct Pinned {
    version: String,
    source: String,
}

/// Paso del manifiesto del que se avisa.
#[derive(Debug, Clone)]
enum Hook {
    /// MITO lo ejecuta con PowerShell (`pre_install`, `installer.script`…).
    Runs(String),
    /// MITO no lo ejecuta (`post_install`).
    Skipped(String),
}

/// Fila visual del contenido, con el texto ya traducido.
#[derive(Debug, Clone)]
enum Row {
    Text(String, Style),
    Gap,
    Button {
        id: String,
        label: String,
        enabled: bool,
    },
}

/// Qué hace cada botón del contenido.
#[derive(Debug, Clone)]
enum Action {
    Release,
    Open(PathBuf),
}

/// «INFORMACIÓN DEL COMPONENTE»: versión, ámbito, descripción, versión retenida,
/// rutas con «Abrir», aportación al entorno, notas y avisos de pasos que MITO no ejecuta.
/// Se cierra con [`Outcome::Closed`] o, con «Liberar», con [`Outcome::Release`].
pub struct ComponentInfoDialog {
    focus: Focus,
    key: String,
    version: String,
    scope: Scope,
    description: Option<String>,
    pinned: Option<Pinned>,
    paths: Vec<PathRow>,
    declares_persistence: bool,
    environment: Option<Environment>,
    notes: Option<String>,
    hooks: Vec<Hook>,
    actions: Vec<(String, Action)>,
    opener: Opener,
    scroll: usize,
    max_scroll: usize,
    page: usize,
    last_focused: Option<String>,
}

/// Constructor de las filas del contenido.
#[derive(Default)]
struct Rows {
    rows: Vec<Row>,
    actions: Vec<(String, Action)>,
}

impl Rows {
    fn text(&mut self, content: String, style: Style) {
        self.rows.push(Row::Text(content, style));
    }

    fn gap(&mut self) {
        self.rows.push(Row::Gap);
    }

    fn button(&mut self, id: &str, label: &str, enabled: bool, action: Action) {
        self.rows.push(Row::Button {
            id: id.to_owned(),
            label: label.to_owned(),
            enabled,
        });
        self.actions.push((id.to_owned(), action));
    }
}

/// Línea visual del contenido tras ajustar los textos al ancho.
enum Visual<'a> {
    Text(String, Style),
    Gap,
    Button(&'a str, &'a str, bool),
}

impl std::fmt::Debug for ComponentInfoDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentInfoDialog")
            .field("key", &self.key)
            .field("version", &self.version)
            .field("scroll", &self.scroll)
            .finish_non_exhaustive()
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Notas del manifiesto (texto o lista de textos) unidas por salto de línea.
fn notes_of(effective: &serde_json::Map<String, Value>) -> Option<String> {
    match effective.get("notes") {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Array(items)) if !items.is_empty() => Some(
            items
                .iter()
                .map(mito_catalog::pyjson::py_str)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

/// Pasos del manifiesto de los que se avisa: los que MITO ejecuta con PowerShell y los
/// que omite.
fn hooks_of(effective: &serde_json::Map<String, Value>) -> Vec<Hook> {
    let mut hooks = Vec::new();
    if effective.get("pre_install").is_some_and(truthy) {
        hooks.push(Hook::Runs("pre_install".to_owned()));
    }
    if let Some(installer) = effective.get("installer").and_then(Value::as_object) {
        for hook in ["script", "file"] {
            if installer.contains_key(hook) {
                hooks.push(Hook::Runs(format!("installer.{hook}")));
            }
        }
    }
    if effective.get("post_install").is_some_and(truthy) {
        hooks.push(Hook::Skipped("post_install".to_owned()));
    }
    hooks
}

/// Versión retenida: versión y origen (`source` más los 10 primeros del `commit`).
fn pinned_of(pinned: &Value) -> Pinned {
    let field = |name: &str| {
        pinned
            .get(name)
            .map(mito_catalog::pyjson::py_str)
            .unwrap_or_default()
    };
    let commit = field("commit");
    let mut source = field("source");
    if !commit.is_empty() {
        source = format!("{source} {}", commit.chars().take(10).collect::<String>());
    }
    Pinned {
        version: field("version"),
        source,
    }
}

fn env_row(name: &str, value: &str, path_entry: bool) -> EnvRow {
    let candidate = PathBuf::from(value.trim_matches('"'));
    let is_path = candidate.is_absolute() && !value.contains(';');
    let path = (is_path || path_entry).then(|| {
        let status = Status::of(&candidate);
        (candidate, status)
    });
    EnvRow {
        name: name.to_owned(),
        value: value.to_owned(),
        path,
    }
}

impl ComponentInfoDialog {
    /// Diálogo para `record` en el contexto de `profile`. Lee del disco el estado de
    /// las rutas una sola vez.
    ///
    /// # Errors
    ///
    /// Un manifiesto sin versión Windows x64, rutas fuera de su raíz, nombres de profile
    /// no válidos y los demás errores del motor al calcular rutas y entorno.
    pub fn new(
        engine: &Engine,
        record: &Record,
        profile: Option<&str>,
    ) -> Result<Self, mito_engine::Error> {
        let layout = engine.layout();
        let effective = record.effective()?;
        let separate = isolation::isolated(record)?;
        let scope = match (separate, profile) {
            (true, Some(profile)) => Scope::Profile(profile.to_owned()),
            (true, None) => Scope::SelectProfile,
            (false, _) => Scope::Shared,
        };
        let row = |label, path: PathBuf| PathRow {
            label,
            status: Status::of(&path),
            path,
        };
        let mut paths = vec![row(
            Label::BaseBinaries,
            inside(engine.root(), &record.path)?,
        )];
        if separate && profile.is_some() {
            paths.push(row(
                Label::ProfileBinaries,
                isolation::payload_path(layout, record, profile)?,
            ));
        }
        let entries = persistence::definitions(effective.get("persist"))?;
        if !separate || profile.is_some() {
            let persist_profile = if separate { profile } else { None };
            for (relative, path) in
                persistence::locations(layout, &record.key, persist_profile, &entries)?
            {
                paths.push(row(Label::Persistence(relative), path));
            }
        }
        let environment = if !separate || profile.is_some() {
            let (variables, folders) = engine.component_environment(record, profile)?;
            Some(Environment {
                variables: variables
                    .iter()
                    .map(|(name, value)| env_row(name, value, false))
                    .collect(),
                paths: folders.iter().map(|path| env_row("", path, true)).collect(),
            })
        } else {
            None
        };
        let notes = notes_of(&effective);
        let hooks = hooks_of(&effective);
        let pinned = record.pinned.as_ref().map(pinned_of);
        Ok(Self {
            focus: Focus::starting_at(CLOSE),
            key: record.key.clone(),
            version: record.version_text(),
            scope,
            description: effective
                .get("description")
                .filter(|d| truthy(d))
                .map(mito_catalog::pyjson::py_str),
            pinned,
            paths,
            declares_persistence: !entries.is_empty(),
            environment,
            notes,
            hooks,
            actions: Vec::new(),
            opener: Box::new(persistence::open_location),
            scroll: 0,
            max_scroll: 0,
            page: 1,
            last_focused: None,
        })
    }

    /// Cambia la función que abre una ruta en el Explorador (para pruebas).
    #[must_use]
    pub fn with_opener(
        mut self,
        opener: impl Fn(&Path) -> CoreResult<()> + Send + 'static,
    ) -> Self {
        self.opener = Box::new(opener);
        self
    }

    /// Filas del contenido con el texto ya traducido, y la acción de cada botón.
    fn rows(&self, ui: &Ui) -> (Vec<Row>, Vec<(String, Action)>) {
        let tr = ui.tr;
        let mut out = Rows::default();
        let body = theme::panel();
        let scope = match &self.scope {
            Scope::Profile(profile) => tr.tr(&Message::new("Profile: {p1}").arg("p1", profile)),
            Scope::SelectProfile => tr
                .text("Selecciona un profile para ver sus datos")
                .to_owned(),
            Scope::Shared => tr.text("Compartido entre profiles").to_owned(),
        };
        out.text(format!("{} · {}\n{scope}", self.key, self.version), body);
        out.gap();
        out.text(
            self.description
                .clone()
                .unwrap_or_else(|| tr.text("Sin descripción.").to_owned()),
            body,
        );
        self.pinned_rows(tr, &mut out);
        self.path_rows(tr, &mut out);
        self.environment_rows(tr, &mut out);
        if let Some(notes) = &self.notes {
            out.gap();
            out.text(tr.text("NOTAS\n").trim().to_owned(), theme::title());
            out.text(notes.clone(), body);
        }
        if !self.hooks.is_empty() {
            out.gap();
        }
        for hook in &self.hooks {
            let text = match hook {
                Hook::Skipped(step) => {
                    tr.tr(&Message::new("Paso omitido: MITO no ejecuta {p1}.").arg("p1", step))
                }
                Hook::Runs(name) if name == "pre_install" => tr
                    .text("pre_install: se ejecuta con PowerShell durante la instalación.")
                    .to_owned(),
                Hook::Runs(name) => tr.tr(&Message::new(
                    "{hook}: se ejecuta con PowerShell durante la instalación.",
                )
                .arg("hook", name)),
            };
            out.text(text, theme::dim());
        }
        (out.rows, out.actions)
    }

    fn pinned_rows(&self, tr: &Translator, out: &mut Rows) {
        let Some(pinned) = &self.pinned else {
            return;
        };
        out.gap();
        out.text(tr.text("VERSIÓN RETENIDA").to_owned(), theme::title());
        out.text(
            tr.tr(
                &Message::new("{p1} elegida desde {p3}. Actualizar no la sustituirá.")
                    .arg("p1", &pinned.version)
                    .arg("p3", &pinned.source),
            ),
            theme::panel(),
        );
        out.button(RELEASE, tr.text("Liberar"), true, Action::Release);
    }

    fn path_rows(&self, tr: &Translator, out: &mut Rows) {
        out.gap();
        out.text(tr.text("RUTAS DEL COMPONENTE").to_owned(), theme::title());
        for (index, path) in self.paths.iter().enumerate() {
            let label = match &path.label {
                Label::BaseBinaries => tr.text("Binarios base").to_owned(),
                Label::ProfileBinaries => tr.text("Binarios del profile").to_owned(),
                Label::Persistence(relative) => {
                    tr.tr(&Message::new("Persistencia: {p1}").arg("p1", relative))
                }
            };
            out.text(label, theme::dim());
            let note = path.status.note().map_or("", |key| tr.text(key));
            out.text(format!("{}{note}", path.path.display()), theme::panel());
            out.button(
                &format!("info-open-{index}"),
                tr.text("Abrir"),
                path.status != Status::Missing,
                Action::Open(path.path.clone()),
            );
        }
        if !self.declares_persistence {
            out.text(
                tr.text("Este componente no declara persistencia.")
                    .to_owned(),
                theme::dim(),
            );
        }
    }

    fn environment_rows(&self, tr: &Translator, out: &mut Rows) {
        out.gap();
        out.text(tr.text("APORTACIÓN AL ENTORNO").to_owned(), theme::title());
        out.text(
            tr.text(
                "Se aplica al activar el componente. Otras variables del entorno pueden sobrescribir estos valores.",
            )
            .to_owned(),
            theme::panel(),
        );
        let Some(environment) = &self.environment else {
            out.text(
                tr.text("Selecciona un profile para ver sus datos")
                    .to_owned(),
                theme::dim(),
            );
            return;
        };
        let mut counter = 0usize;
        let sections = [
            (tr.text("Variables de entorno"), &environment.variables),
            ("path", &environment.paths),
        ];
        for (title, entries) in sections {
            out.gap();
            out.text(title.to_owned(), theme::dim());
            if entries.is_empty() {
                out.text("—".to_owned(), theme::panel());
            }
            for entry in entries {
                let status = entry
                    .path
                    .as_ref()
                    .and_then(|(_, status)| status.note())
                    .map_or("", |key| tr.text(key));
                let shown = if entry.name.is_empty() {
                    format!("{}{status}", entry.value)
                } else {
                    format!("{} = {}{status}", entry.name, entry.value)
                };
                out.text(shown, theme::panel());
                if let Some((path, found)) = &entry.path {
                    out.button(
                        &format!("info-env-open-{counter}"),
                        tr.text("Abrir"),
                        *found != Status::Missing,
                        Action::Open(path.clone()),
                    );
                    counter += 1;
                }
            }
        }
    }

    fn open(&self, path: &Path, context: &mut Context) {
        if let Err(error) = (self.opener)(path) {
            error_tracking::capture(
                context.engine.layout(),
                &error,
                &Options::operation("component.open-location"),
            );
            context.notices.push(Notice::new(
                describe_core(&error, context.tr),
                Severity::Error,
            ));
        }
    }

    fn act(&mut self, id: &str, context: &mut Context) -> Step {
        if id == CLOSE {
            return Step::Close(Outcome::Closed);
        }
        let action = self
            .actions
            .iter()
            .find(|(action_id, _)| action_id == id)
            .map(|(_, action)| action.clone());
        match action {
            Some(Action::Release) => Step::Close(Outcome::Release),
            Some(Action::Open(path)) => {
                self.open(&path, context);
                Step::Stay
            }
            None => Step::Stay,
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.max_scroll);
    }
}

impl Dialog for ComponentInfoDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let area = modal::area(screen, Size::PercentMax(90, 110), Size::Percent(85));
        let inner = modal::frame(frame, area, theme::BORDER);
        let mut stack = Stack::new(inner);
        header::render(
            frame,
            stack.take(header::HEIGHT),
            ui,
            "INFORMACIÓN DEL COMPONENTE",
            CLOSE,
        );
        let body = stack.remaining();
        let (rows, actions) = self.rows(ui);
        self.actions = actions;
        let width = usize::from(body.width);
        let mut lines: Vec<Visual> = Vec::new();
        for row in &rows {
            match row {
                Row::Text(content, style) => {
                    for part in text::wrap(content, width) {
                        lines.push(Visual::Text(part, *style));
                    }
                }
                Row::Gap => lines.push(Visual::Gap),
                Row::Button { id, label, enabled } => {
                    lines.push(Visual::Button(id, label, *enabled));
                }
            }
        }
        let height = usize::from(body.height);
        self.page = height.max(1);
        self.max_scroll = lines.len().saturating_sub(height);
        // El botón enfocado se mantiene a la vista cuando el foco cambia.
        let focused = ui.focused().map(str::to_owned);
        if focused != self.last_focused {
            if let Some(line) = focused.as_deref().and_then(|id| {
                lines
                    .iter()
                    .position(|line| matches!(line, Visual::Button(button, ..) if *button == id))
            }) {
                if line < self.scroll {
                    self.scroll = line;
                } else if height > 0 && line >= self.scroll + height {
                    self.scroll = line + 1 - height;
                }
            }
            self.last_focused = focused;
        }
        self.scroll = self.scroll.min(self.max_scroll);
        for (index, line) in lines.iter().enumerate() {
            let visible = index >= self.scroll && index < self.scroll + height;
            let y = body.y + u16::try_from(index.saturating_sub(self.scroll)).unwrap_or(0);
            match line {
                Visual::Text(content, style) if visible => frame.render_widget(
                    Paragraph::new(Span::styled(content.clone(), *style)),
                    Rect::new(body.x, y, body.width, 1),
                ),
                Visual::Button(id, label, enabled) => {
                    let button_width = u16::try_from(text::width(label) + 4)
                        .unwrap_or(BUTTON_WIDTH)
                        .max(BUTTON_WIDTH)
                        .min(body.width);
                    let button = Button::new(*id, *label).disabled(!*enabled);
                    if visible {
                        button.render(frame, Rect::new(body.x, y, button_width, 1), ui);
                    } else if *enabled {
                        // Fuera de la vista sigue en el orden de Tab, sin zona pulsable.
                        ui.focusable(id, Rect::ZERO);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle(&mut self, input: &Input, context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        if navigate(&mut self.focus, input, false) {
            return Step::Stay;
        }
        let page = isize::try_from(self.page).unwrap_or(isize::MAX);
        match input {
            Input::Scroll { down, .. } => self.scroll_by(if *down { 3 } else { -3 }),
            _ if input.is(KeyCode::Up) => self.scroll_by(-1),
            _ if input.is(KeyCode::Down) => self.scroll_by(1),
            _ if input.is(KeyCode::PageUp) => self.scroll_by(-page),
            _ if input.is(KeyCode::PageDown) => self.scroll_by(page),
            _ if input.is(KeyCode::Home) => self.scroll = 0,
            _ if input.is(KeyCode::End) => self.scroll = self.max_scroll,
            _ => {
                if let Some(id) = pressed(&mut self.focus, input) {
                    return self.act(&id, context);
                }
            }
        }
        Step::Stay
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn record(manifest: Value) -> Record {
        Record {
            key: "main/tool".into(),
            version: json!("1.2.3"),
            manifest_sha256: String::new(),
            path: "core/apps/main/tool".into(),
            manifest,
            pinned: None,
        }
    }

    fn manifest() -> Value {
        json!({
            "version": "1.2.3",
            "description": "Una herramienta",
            "bin": "tool.exe",
            "persist": ["data", ["cfg.ini", "settings.ini"]],
            "env_set": {"TOOL_HOME": "$dir"},
            "notes": ["Primera nota", "Segunda nota"],
            "installer": {"script": "Write-Host hola"},
            "post_install": "echo listo"
        })
    }

    fn make_dirs(harness: &Harness, record: &Record) {
        let base = harness.dir.path().join(&record.path);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("tool.exe"), b"x").unwrap();
    }

    #[test]
    fn aspecto_con_profile() {
        // Raíz inexistente y corta: el aspecto no depende del directorio temporal.
        let harness = Harness::new();
        let engine = Engine::new("C:/mito-snapshot");
        let record = record(manifest());
        let mut dialog = ComponentInfoDialog::new(&engine, &record, Some("uno")).unwrap();
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 60));
    }

    #[test]
    fn aspecto_aislado_sin_profile_pide_elegir_uno() {
        let harness = Harness::spanish();
        let record = record(json!({
            "version": "1",
            "persist": "data",
            "shortcuts": [["tool.exe", "Tool"]]
        }));
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, None).unwrap();
        let text = harness.draw(&mut dialog, 100, 40);
        assert!(
            text.contains("Selecciona un profile para ver sus datos"),
            "{text}"
        );
        assert!(!text.contains("Persistencia:"), "{text}");
        assert!(!text.contains("Variables de entorno"), "{text}");
    }

    #[test]
    fn version_retenida_con_liberar() {
        let mut harness = Harness::new();
        let mut record = record(json!({"version": "1.5"}));
        record.pinned = Some(json!({
            "version": "1.5",
            "source": "git",
            "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }));
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, None).unwrap();
        let text = harness.draw(&mut dialog, 100, 40);
        assert!(text.contains("PINNED VERSION"), "{text}");
        assert!(text.contains("1.5 chosen from git aaaaaaaaaa."), "{text}");
        harness.focus_on(&mut dialog, RELEASE);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Release)
        );
    }

    #[test]
    fn abrir_ruta_llama_al_abridor_solo_si_existe() {
        let mut harness = Harness::new();
        let record = record(manifest());
        make_dirs(&harness, &record);
        let opened = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&opened);
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, Some("uno"))
            .unwrap()
            .with_opener(move |path| {
                seen.lock().unwrap().push(path.to_path_buf());
                Ok(())
            });
        harness.draw(&mut dialog, 100, 60);
        // Las rutas de persistencia todavía no existen: sus botones no se registran.
        assert!(dialog.focus().is_registered("info-open-0"));
        assert!(!dialog.focus().is_registered("info-open-1"));
        harness.click(&mut dialog, "info-open-0");
        // Se comparan canonicalizadas: la carpeta temporal puede llegar con nombre corto
        // 8.3 (`RUNNER~1`) y el motor la devuelve con el nombre largo.
        let opened = opened.lock().unwrap().clone();
        assert_eq!(opened.len(), 1);
        assert_eq!(
            std::fs::canonicalize(&opened[0]).unwrap(),
            std::fs::canonicalize(harness.dir.path().join("core/apps/main/tool")).unwrap()
        );
    }

    #[test]
    fn un_error_al_abrir_avisa_y_no_cierra() {
        let mut harness = Harness::spanish();
        let record = record(manifest());
        make_dirs(&harness, &record);
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, Some("uno"))
            .unwrap()
            .with_opener(|_| {
                Err(mito_core::Error::domain(Message::new(
                    "Esta persistencia todavía no se ha creado",
                )))
            });
        harness.draw(&mut dialog, 100, 60);
        assert_eq!(harness.click(&mut dialog, "info-open-0"), Step::Stay);
        assert_eq!(harness.notices.len(), 1);
        assert_eq!(harness.notices[0].severity, Severity::Error);
        assert_eq!(
            harness.notices[0].text,
            "Esta persistencia todavía no se ha creado"
        );
    }

    #[test]
    fn tab_lleva_a_los_botones_fuera_de_la_vista_y_el_scroll_los_sigue() {
        let mut harness = Harness::new();
        let record = record(manifest());
        make_dirs(&harness, &record);
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, Some("uno")).unwrap();
        // Pantalla baja: el contenido no cabe.
        let text = harness.draw(&mut dialog, 100, 20);
        assert!(!text.contains("Segunda nota"), "{text}");
        harness.press(&mut dialog, KeyCode::End);
        let text = harness.draw(&mut dialog, 100, 20);
        assert!(text.contains("Segunda nota"), "{text}");
        harness.press(&mut dialog, KeyCode::Home);
        harness.draw(&mut dialog, 100, 20);
        // El último botón del orden está fuera de la vista; Mayús+Tab desde el cierre.
        harness.press(&mut dialog, KeyCode::BackTab);
        let last = dialog.focus().focused().unwrap().to_owned();
        assert!(last.starts_with("info-"), "{last}");
        let text = harness.draw(&mut dialog, 100, 20);
        assert!(
            text.contains("[ Open ]"),
            "el botón enfocado es visible: {text}"
        );
    }

    #[test]
    fn esc_y_la_cruz_cierran() {
        let mut harness = Harness::new();
        let record = record(manifest());
        let mut dialog = ComponentInfoDialog::new(&harness.engine, &record, Some("uno")).unwrap();
        harness.draw(&mut dialog, 100, 60);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Closed)
        );
    }

    #[test]
    fn un_manifiesto_sin_x64_es_un_error() {
        let harness = Harness::new();
        let record = record(json!({"architecture": {"32bit": {}}}));
        assert!(ComponentInfoDialog::new(&harness.engine, &record, None).is_err());
    }
}
