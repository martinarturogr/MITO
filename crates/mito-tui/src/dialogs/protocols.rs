//! Diálogo «Reclamar protocolo» (`ProtocolDialog`) y la sesión que lo respalda.
//!
//! Mientras el diálogo está abierto, los esquemas reclamados abren el componente con el
//! entorno del profile; el diálogo los comprueba cada 5 s (`WATCH_SECONDS`) con su tic y
//! los recupera si otra aplicación los sobrescribe. Al cerrarse (o al soltarse) la sesión
//! libera todo lo reclamado.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use mito_core::Message;
use mito_core::diagnostics::{self, LogLevel};
use mito_desktop::open_with::Classes;
use mito_desktop::protocols::{self, Row, Session};
use mito_engine::{Engine, LogLine};
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step, drive_select, navigate, open_select};
use crate::input::Input;
use crate::model::describe;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::text;
use crate::widgets::{Button, Focus, Select, TextInput, Ui, Variant, WidgetEvent};
use crate::worker::Report;

const CLOSE: &str = "protocol-close";
const ENTRY: &str = "protocol-entry";
const SCHEME: &str = "protocol-scheme";
const ADD: &str = "protocol-add";
const TOGGLE_PREFIX: &str = "protocol-toggle-";
/// Ancho del botón «Reclamar»/«Liberar».
const TOGGLE_WIDTH: u16 = 14;
/// Filas de pantalla de cada esquema: etiqueta, comando y nota.
const ROW_HEIGHT: u16 = 3;

/// Lo que el diálogo necesita de una sesión de protocolos. Los errores ya vienen
/// descritos para el usuario.
pub trait ProtocolBackend: Send {
    /// Fila de un esquema (lo normaliza y valida).
    ///
    /// # Errors
    ///
    /// Texto del error (esquema no válido o reservado).
    fn describe(&mut self, scheme: &str) -> Result<Row, String>;

    /// Reclama el esquema con la entrada de aplicación indicada.
    ///
    /// # Errors
    ///
    /// Texto del error.
    fn claim(&mut self, scheme: &str, shortcut: Option<&str>) -> Result<Row, String>;

    /// Vuelve a afirmar un esquema reclamado; `true` si lo recuperó.
    ///
    /// # Errors
    ///
    /// Texto del error.
    fn check(&mut self, scheme: &str) -> Result<bool, String>;

    /// Libera un esquema o, sin él, todos los reclamados.
    ///
    /// # Errors
    ///
    /// Texto del error.
    fn release(&mut self, scheme: Option<&str>) -> Result<(), String>;
}

/// Orden para el hilo que posee la [`Session`].
enum Command {
    Describe(String, Sender<Result<Row, String>>),
    Claim(String, Option<String>, Sender<Result<Row, String>>),
    Check(String, Sender<Result<bool, String>>),
    Release(Option<String>, Sender<Result<(), String>>),
}

/// [`Session`] real de `mito-desktop`, que vive en su propio hilo (toma prestado el
/// motor) y libera lo reclamado al soltarse.
pub struct ThreadedSession {
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ThreadedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadedSession").finish_non_exhaustive()
    }
}

impl ThreadedSession {
    /// Abre la sesión de `installation` en `profile` sobre la rama de clases `classes`.
    /// Lo que registre la sesión va al log de la interfaz por `reports` y a
    /// `logs/mito.log`.
    pub fn open(
        engine: Arc<Engine>,
        classes: Classes,
        profile: String,
        installation: String,
        tr: Translator,
        reports: Sender<Report>,
    ) -> Self {
        let (commands, inbox) = channel::<Command>();
        let thread = std::thread::spawn(move || {
            let log = |line: LogLine| {
                diagnostics::write(
                    engine.layout(),
                    LogLevel::Info,
                    "ui",
                    &line.message.to_string(),
                    &[],
                );
                let _ = reports.send(Report::Log {
                    message: line.message,
                    error: false,
                    long_step: None,
                });
            };
            let mut session = Session::new(classes, &engine, &profile, &installation, &log);
            let fail = |error: mito_engine::Error| describe(&error, &tr);
            while let Ok(command) = inbox.recv() {
                match command {
                    Command::Describe(scheme, reply) => {
                        let _ = reply.send(session.describe(&scheme).map_err(fail));
                    }
                    Command::Claim(scheme, shortcut, reply) => {
                        let _ =
                            reply.send(session.claim(&scheme, shortcut.as_deref()).map_err(fail));
                    }
                    Command::Check(scheme, reply) => {
                        let _ = reply.send(session.check(&scheme).map_err(fail));
                    }
                    Command::Release(scheme, reply) => {
                        let _ = reply.send(session.release(scheme.as_deref()).map_err(fail));
                    }
                }
            }
            // Al soltar `session` se libera lo que quede reclamado.
        });
        Self {
            commands: Some(commands),
            thread: Some(thread),
        }
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<Result<T, String>>) -> Command) -> Result<T, String> {
        let (reply, response) = channel();
        let commands = self
            .commands
            .as_ref()
            .ok_or_else(|| "sesión de protocolos cerrada".to_owned())?;
        commands
            .send(make(reply))
            .map_err(|_| "sesión de protocolos cerrada".to_owned())?;
        response
            .recv()
            .map_err(|_| "sesión de protocolos cerrada".to_owned())?
    }
}

impl ProtocolBackend for ThreadedSession {
    fn describe(&mut self, scheme: &str) -> Result<Row, String> {
        self.ask(|reply| Command::Describe(scheme.to_owned(), reply))
    }

    fn claim(&mut self, scheme: &str, shortcut: Option<&str>) -> Result<Row, String> {
        self.ask(|reply| Command::Claim(scheme.to_owned(), shortcut.map(str::to_owned), reply))
    }

    fn check(&mut self, scheme: &str) -> Result<bool, String> {
        self.ask(|reply| Command::Check(scheme.to_owned(), reply))
    }

    fn release(&mut self, scheme: Option<&str>) -> Result<(), String> {
        self.ask(|reply| Command::Release(scheme.map(str::to_owned), reply))
    }
}

impl Drop for ThreadedSession {
    fn drop(&mut self) {
        // Cerrar el canal hace que el hilo termine y suelte la sesión (que libera).
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Id del botón de alternancia de un esquema.
fn toggle_id(scheme: &str) -> String {
    let clean: String = scheme
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{TOGGLE_PREFIX}{clean}")
}

/// «Reclamar protocolo»: esquemas candidatos del componente con su estado, y un botón por
/// esquema para reclamarlo o liberarlo. Se cierra con [`Outcome::Closed`] (Cerrar o Esc),
/// tras liberar todo lo reclamado.
pub struct ProtocolDialog {
    prompt: Prompt,
    name: String,
    entries: Vec<String>,
    entry: Option<Select<String>>,
    rows: BTreeMap<String, Row>,
    /// Nota de cada esquema reclamado (`None` si no hay nota propia).
    claimed: BTreeMap<String, Option<String>>,
    scheme: TextInput,
    error: String,
    backend: Box<dyn ProtocolBackend>,
    interval: Duration,
    last_check: Instant,
    offset: usize,
    visible: usize,
}

impl std::fmt::Debug for ProtocolDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolDialog")
            .field("name", &self.name)
            .field("claimed", &self.claimed.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl ProtocolDialog {
    /// Diálogo para `name` con las `entries` (entradas de aplicación), las filas iniciales
    /// y la sesión que reclama y libera.
    pub fn new(
        name: impl Into<String>,
        entries: Vec<String>,
        rows: Vec<Row>,
        backend: Box<dyn ProtocolBackend>,
    ) -> Self {
        let buttons = vec![PromptButton {
            id: CLOSE,
            label: "Cerrar",
            variant: Variant::Primary,
        }];
        let entry = (entries.len() > 1).then(|| {
            let options = entries.iter().map(|e| (e.clone(), e.clone())).collect();
            Select::new(ENTRY, options, Some(0))
        });
        Self {
            prompt: Prompt::new(buttons, SCHEME, Size::Cells(90), theme::BORDER),
            name: name.into(),
            entries,
            entry,
            rows: rows
                .into_iter()
                .map(|row| (row.scheme.clone(), row))
                .collect(),
            claimed: BTreeMap::new(),
            scheme: TextInput::new(SCHEME),
            error: String::new(),
            backend,
            interval: Duration::from_secs(protocols::WATCH_SECONDS),
            last_check: Instant::now(),
            offset: 0,
            visible: 1,
        }
    }

    /// Cambia cada cuánto se comprueban los esquemas reclamados (para pruebas).
    #[must_use]
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Esquemas reclamados por el diálogo.
    pub fn claimed(&self) -> Vec<&str> {
        self.claimed.keys().map(String::as_str).collect()
    }

    /// Entrada de aplicación con la que se reclama.
    fn shortcut(&self) -> Option<String> {
        match &self.entry {
            Some(select) => select.value().cloned(),
            None => self.entries.first().cloned(),
        }
    }

    /// `Añadir`: describe el esquema y añade su fila.
    fn add(&mut self) {
        let text = self.scheme.value.trim().to_owned();
        if text.is_empty() {
            return;
        }
        match self.backend.describe(&text) {
            Ok(row) => {
                self.error.clear();
                self.rows.insert(row.scheme.clone(), row);
                self.scheme.set_value("");
            }
            Err(error) => self.error = error,
        }
    }

    /// Alterna un esquema: libera si está reclamado y si no, lo reclama.
    fn toggle(&mut self, scheme: &str) {
        let result = if self.claimed.contains_key(scheme) {
            self.backend.release(Some(scheme)).and_then(|()| {
                self.claimed.remove(scheme);
                self.backend.describe(scheme)
            })
        } else {
            let shortcut = self.shortcut();
            self.backend
                .claim(scheme, shortcut.as_deref())
                .inspect(|_| {
                    self.claimed.insert(scheme.to_owned(), None);
                })
        };
        match result {
            Ok(row) => {
                self.error.clear();
                self.rows.insert(row.scheme.clone(), row);
            }
            Err(error) => self.error = error,
        }
    }

    /// Comprueba los esquemas reclamados (`watch`).
    fn watch(&mut self, context: &Context) {
        let schemes: Vec<String> = self.claimed.keys().cloned().collect();
        for scheme in schemes {
            match self.backend.check(&scheme) {
                Ok(true) => {
                    let now = jiff::ZonedDateTime::now().strftime("%H:%M:%S").to_string();
                    let note = context.tr.tr(&Message::new(
                        "Recuperado a las {p1}: otra app lo había sobrescrito.",
                    )
                    .arg("p1", now));
                    self.claimed.insert(scheme.clone(), Some(note));
                    if let Ok(row) = self.backend.describe(&scheme) {
                        self.rows.insert(scheme, row);
                    }
                }
                Ok(false) => {}
                Err(error) => self.error = error,
            }
        }
    }

    fn close(&mut self) -> Step {
        if let Err(error) = self.backend.release(None) {
            self.error = error;
        }
        self.claimed.clear();
        Step::Close(Outcome::Closed)
    }

    fn scheme_of(&self, id: &str) -> Option<String> {
        self.rows
            .keys()
            .find(|scheme| toggle_id(scheme) == id)
            .cloned()
    }

    fn on_button(&mut self, id: &str) -> Option<Step> {
        if id == CLOSE {
            return Some(self.close());
        }
        if id == ADD {
            self.add();
            return Some(Step::Stay);
        }
        let scheme = self.scheme_of(id)?;
        self.toggle(&scheme);
        Some(Step::Stay)
    }

    /// Dibuja las filas visibles y registra (sin zona) las que quedan fuera.
    fn render_rows(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        self.visible = usize::from(area.height / ROW_HEIGHT).max(1);
        let schemes: Vec<String> = self.rows.keys().cloned().collect();
        if let Some(index) = ui
            .focused()
            .and_then(|id| schemes.iter().position(|scheme| toggle_id(scheme) == id))
        {
            if index < self.offset {
                self.offset = index;
            } else if index >= self.offset + self.visible {
                self.offset = index + 1 - self.visible;
            }
        }
        self.offset = self.offset.min(schemes.len().saturating_sub(self.visible));
        let tr = ui.tr;
        for (index, scheme) in schemes.iter().enumerate() {
            let Some(row) = self.rows.get(scheme) else {
                continue;
            };
            let id = toggle_id(scheme);
            let claimed = self.claimed.contains_key(scheme);
            let (label, variant) = if claimed {
                (tr.text("Liberar"), Variant::Success)
            } else {
                (tr.text("Reclamar"), Variant::Default)
            };
            let button = Button::new(id.clone(), label).variant(variant);
            let Some(line) = index
                .checked_sub(self.offset)
                .filter(|line| *line < self.visible)
                .and_then(|line| u16::try_from(line).ok())
            else {
                ui.focusable(&id, Rect::ZERO);
                continue;
            };
            let top = area.y + line * ROW_HEIGHT;
            let width = area.width.saturating_sub(TOGGLE_WIDTH + 1);
            let state = tr.tr(&row.status.label());
            let head = format!("{scheme}://  ·  {state}");
            frame.render_widget(
                Paragraph::new(text::truncate(&head, usize::from(width))).style(theme::panel()),
                Rect::new(area.x, top, width, 1),
            );
            button.render(
                frame,
                Rect::new(
                    area.right().saturating_sub(TOGGLE_WIDTH),
                    top,
                    TOGGLE_WIDTH,
                    1,
                ),
                ui,
            );
            let command = row.command.as_deref().unwrap_or("—");
            frame.render_widget(
                Paragraph::new(text::truncate(command, usize::from(area.width)))
                    .style(theme::dim()),
                Rect::new(area.x, top + 1, area.width, 1),
            );
            if let Some(note) = self.claimed.get(scheme) {
                let note = note.clone().unwrap_or_else(|| {
                    tr.text("Reclamado mientras este diálogo esté abierto.")
                        .to_owned()
                });
                frame.render_widget(
                    Paragraph::new(text::truncate(&note, usize::from(area.width)))
                        .style(Style::new().fg(theme::CHECK_ON).bg(theme::SIDEBAR)),
                    Rect::new(area.x, top + 2, area.width, 1),
                );
            }
        }
    }
}

impl Dialog for ProtocolDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let width = Size::Cells(90).resolve(screen.width).saturating_sub(4);
        let help = ui.tr.text(
            "Pulsa Reclamar: mientras este diálogo esté abierto, los enlaces del esquema, como el retorno de un inicio de sesión en el navegador, abrirán el componente con el entorno del profile. MITO lo comprueba cada 5 s y lo recupera si otra app lo sobrescribe. Al cerrar se restaura el registro anterior.",
        );
        let make = |rows: u16| {
            let mut content = Content::new();
            content
                .text(ui.tr.text("Reclamar protocolo"), theme::title())
                .gap(1)
                .text(&self.name, theme::panel())
                .gap(1);
            if self.entry.is_some() {
                content.slot("entry", 1).gap(1);
            }
            content.slot("rows", rows).gap(1).slot("adder", 1).gap(1);
            content.text(help, theme::panel());
            if !self.error.is_empty() {
                content.text(
                    &self.error,
                    Style::new().fg(theme::BADGE_ERROR).bg(theme::SIDEBAR),
                );
            }
            content
        };
        // Alto disponible para las filas: lo que cabe en el 90 % de la pantalla.
        let limit = u32::from(screen.height) * 9 / 10;
        let fixed = u32::from(make(0).height(width)) + 4;
        let room = u16::try_from(limit.saturating_sub(fixed)).unwrap_or(0);
        let wanted = u16::try_from(self.rows.len())
            .unwrap_or(u16::MAX)
            .saturating_mul(ROW_HEIGHT);
        let rows_height = wanted.min(room - room % ROW_HEIGHT).max(ROW_HEIGHT);
        let mut content = make(rows_height);
        self.scheme.placeholder = ui.tr.text("Esquema (por ejemplo: claude)").to_owned();
        let (entry, scheme) = (self.entry.take(), &self.scheme);
        let mut rendered: Option<Rect> = None;
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                if let Some(select) = &entry {
                    select.render(frame, slots.get("entry"), ui);
                }
                rendered = Some(slots.get("rows"));
                let cells = Layout::horizontal([Constraint::Fill(1), Constraint::Length(12)])
                    .spacing(1)
                    .split(slots.get("adder"));
                scheme.render(frame, cells[0], ui);
                Button::new(ADD, ui.tr.text("Añadir")).render(frame, cells[1], ui);
            });
        self.entry = entry;
        if let Some(area) = rendered {
            // Las filas van encima del botón «Cerrar» en el orden de foco: se dibujan
            // después, pero se registran antes de él gracias al registro del `Ui`.
            self.render_rows(frame, area, ui);
        }
        if let Some(select) = &self.entry {
            select.render_overlay(frame, ui);
        }
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        if open_select(&mut selects, &self.prompt.focus, input).is_some() {
            return Step::Stay;
        }
        if input.is(KeyCode::Esc) {
            return self.close();
        }
        let focused = self.prompt.focus.focused().map(str::to_owned);
        let in_field = matches!(focused.as_deref(), Some(SCHEME | ENTRY));
        if navigate(&mut self.prompt.focus, input, !in_field) {
            return Step::Stay;
        }
        if let Input::Scroll { down, .. } = input {
            let max = self.rows.len().saturating_sub(self.visible);
            self.offset = if *down {
                (self.offset + 1).min(max)
            } else {
                self.offset.saturating_sub(1)
            };
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.prompt.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.prompt.focus.set(&id);
            let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
            if drive_select(&mut selects, &id, input).is_some() {
                return Step::Stay;
            }
            if id == SCHEME {
                self.scheme.click(*column);
                return Step::Stay;
            }
            return self.on_button(&id).unwrap_or(Step::Stay);
        }
        let Some(id) = focused else {
            return Step::Stay;
        };
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        if drive_select(&mut selects, &id, input).is_some() {
            return Step::Stay;
        }
        if id == SCHEME {
            if let Some(WidgetEvent::Submitted(_)) = self.scheme.handle(input) {
                self.add();
            }
            return Step::Stay;
        }
        if input.is_activation() {
            return self.on_button(&id).unwrap_or(Step::Stay);
        }
        Step::Stay
    }

    fn tick(&mut self, context: &mut Context) -> Step {
        if self.last_check.elapsed() >= self.interval {
            self.last_check = Instant::now();
            self.watch(context);
        }
        Step::Stay
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.prompt.focus
    }
}

impl Drop for ProtocolDialog {
    fn drop(&mut self) {
        // Salvaguarda: cerrar de cualquier forma libera lo reclamado.
        if !self.claimed.is_empty() {
            let _ = self.backend.release(None);
        }
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use std::sync::Mutex;

    use mito_desktop::protocols::Status;

    use super::*;
    use crate::dialogs::testing::Harness;

    /// Sesión falsa que apunta lo que se le pide.
    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<String>>>,
        recovered: Arc<Mutex<bool>>,
        claimed: Arc<Mutex<Vec<String>>>,
    }

    impl Fake {
        fn row(scheme: &str, status: Status, command: Option<&str>) -> Row {
            Row {
                scheme: scheme.to_owned(),
                status,
                command: command.map(str::to_owned),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ProtocolBackend for Fake {
        fn describe(&mut self, scheme: &str) -> Result<Row, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("describe {scheme}"));
            if scheme == "http" {
                return Err("Esquema reservado del sistema: http".to_owned());
            }
            let claimed = self.claimed.lock().unwrap().contains(&scheme.to_owned());
            Ok(Self::row(
                scheme,
                if claimed { Status::Mine } else { Status::Other },
                Some("claude.exe"),
            ))
        }

        fn claim(&mut self, scheme: &str, shortcut: Option<&str>) -> Result<Row, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("claim {scheme} {shortcut:?}"));
            self.claimed.lock().unwrap().push(scheme.to_owned());
            Ok(Self::row(scheme, Status::Mine, Some("mitow.exe run")))
        }

        fn check(&mut self, scheme: &str) -> Result<bool, String> {
            self.calls.lock().unwrap().push(format!("check {scheme}"));
            Ok(std::mem::take(&mut *self.recovered.lock().unwrap()))
        }

        fn release(&mut self, scheme: Option<&str>) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("release {scheme:?}"));
            match scheme {
                Some(scheme) => self.claimed.lock().unwrap().retain(|s| s != scheme),
                None => self.claimed.lock().unwrap().clear(),
            }
            Ok(())
        }
    }

    fn dialog(fake: &Fake, entries: &[&str]) -> ProtocolDialog {
        ProtocolDialog::new(
            "main/claude",
            entries.iter().map(|e| (*e).to_owned()).collect(),
            vec![Fake::row("claude", Status::Other, Some("claude.exe"))],
            Box::new(fake.clone()),
        )
        .with_interval(Duration::ZERO)
    }

    #[test]
    fn aspecto() {
        let harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 34));
    }

    #[test]
    fn aspecto_reclamado_con_nota() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        *fake.recovered.lock().unwrap() = true;
        harness.tick(&mut dialog);
        let text = harness.draw(&mut dialog, 100, 34);
        assert!(
            text.contains("Liberar") && text.contains("Recuperado a las "),
            "{text}"
        );
    }

    #[test]
    fn anadir_un_esquema_reservado_muestra_el_error_y_uno_valido_la_fila() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        harness.draw(&mut dialog, 100, 34);
        assert_eq!(dialog.focus().focused(), Some(SCHEME));
        harness.type_text(&mut dialog, "http");
        harness.press(&mut dialog, KeyCode::Enter);
        let text = harness.draw(&mut dialog, 100, 34);
        assert!(
            text.contains("Esquema reservado del sistema: http"),
            "{text}"
        );
        assert_eq!(dialog.rows.len(), 1);
        for _ in 0..4 {
            harness.press(&mut dialog, KeyCode::Backspace);
        }
        harness.type_text(&mut dialog, "mi-app");
        harness.click(&mut dialog, ADD);
        let text = harness.draw(&mut dialog, 100, 34);
        assert!(text.contains("mi-app://  ·  Otra app"), "{text}");
        assert!(!text.contains("reservado"), "{text}");
    }

    #[test]
    fn reclamar_vigilar_y_cerrar_libera_todo() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        assert_eq!(fake.calls(), ["claim claude Some(\"Claude\")"]);
        assert_eq!(dialog.claimed(), ["claude"]);
        // Sin sobrescritura no hay nota; con ella aparece «Recuperado».
        harness.tick(&mut dialog);
        assert_eq!(dialog.claimed.get("claude"), Some(&None));
        *fake.recovered.lock().unwrap() = true;
        harness.tick(&mut dialog);
        let note = dialog.claimed.get("claude").cloned().flatten().unwrap();
        assert!(note.starts_with("Recuperado a las "), "{note}");
        assert!(note.ends_with(": otra app lo había sobrescrito."), "{note}");
        harness.draw(&mut dialog, 100, 34);
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Closed)
        );
        assert!(fake.calls().contains(&"release None".to_owned()));
        assert!(fake.claimed.lock().unwrap().is_empty());
        assert!(dialog.claimed().is_empty());
    }

    #[test]
    fn liberar_un_esquema_solo_libera_ese() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        harness.click(&mut dialog, "protocol-toggle-claude");
        assert!(
            fake.calls()
                .contains(&"release Some(\"claude\")".to_owned())
        );
        assert!(dialog.claimed().is_empty());
        let text = harness.draw(&mut dialog, 100, 34);
        assert!(text.contains("Reclamar"), "{text}");
    }

    #[test]
    fn esc_cierra_liberando_y_soltarlo_tambien_libera() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]);
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
        assert!(fake.claimed.lock().unwrap().is_empty());

        let fake = Fake::default();
        let mut dialog = dialog_for_drop(&fake);
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        assert_eq!(fake.claimed.lock().unwrap().len(), 1);
        drop(dialog);
        assert!(fake.claimed.lock().unwrap().is_empty());
    }

    fn dialog_for_drop(fake: &Fake) -> ProtocolDialog {
        dialog(fake, &["Claude"])
    }

    #[test]
    fn con_varias_entradas_se_reclama_con_la_elegida() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude", "Claude Code"]);
        harness.draw(&mut dialog, 100, 38);
        harness.focus_on(&mut dialog, ENTRY);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 100, 38);
        harness.click(&mut dialog, "protocol-toggle-claude");
        assert_eq!(fake.calls(), ["claim claude Some(\"Claude Code\")"]);
    }

    #[test]
    fn sin_pasar_el_intervalo_no_se_comprueba() {
        let mut harness = Harness::spanish();
        let fake = Fake::default();
        let mut dialog = dialog(&fake, &["Claude"]).with_interval(Duration::from_secs(3600));
        harness.draw(&mut dialog, 100, 34);
        harness.click(&mut dialog, "protocol-toggle-claude");
        harness.tick(&mut dialog);
        assert!(!fake.calls().iter().any(|call| call.starts_with("check")));
    }
}
