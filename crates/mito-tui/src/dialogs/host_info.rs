//! Información de una aplicación del host (`HostInfoDialog`).

use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use mito_core::error_tracking::{self, Options};
use mito_core::{Message, Result as CoreResult, persistence};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use super::{Context, Dialog, Outcome, Step, describe_core, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::text;
use crate::widgets::{Button, Focus, Notice, Severity, Stack, Ui, header};

const CLOSE: &str = "host-info-close";
const OPEN: &str = "host-folder-open";
/// Ancho del botón «Abrir».
const BUTTON_WIDTH: u16 = 10;

type Opener = Box<dyn Fn(&Path) -> CoreResult<()> + Send>;

/// «APLICACIÓN DEL HOST»: nombre, ruta, carpeta del ejecutable (con «Abrir») y un aviso si
/// el archivo ya no existe. Se cierra con [`Outcome::Closed`] (× o Esc).
pub struct HostInfoDialog {
    focus: Focus,
    path: PathBuf,
    opener: Opener,
}

impl std::fmt::Debug for HostInfoDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostInfoDialog")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl HostInfoDialog {
    /// Diálogo para la aplicación del host en `path`.
    pub fn new(path: PathBuf) -> Self {
        Self {
            focus: Focus::new(),
            path,
            opener: Box::new(persistence::open_location),
        }
    }

    /// Cambia la función que abre una carpeta en el Explorador (para pruebas).
    #[must_use]
    pub fn with_opener(
        mut self,
        opener: impl Fn(&Path) -> CoreResult<()> + Send + 'static,
    ) -> Self {
        self.opener = Box::new(opener);
        self
    }

    fn folder(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new(""))
    }

    fn open(&self, context: &mut Context) {
        if let Err(error) = (self.opener)(self.folder()) {
            error_tracking::capture(
                context.engine.layout(),
                &error,
                &Options::operation("host.open-location"),
            );
            context.notices.push(Notice::new(
                describe_core(&error, context.tr),
                Severity::Error,
            ));
        }
    }

    fn act(&mut self, id: &str, context: &mut Context) -> Step {
        match id {
            CLOSE => Step::Close(Outcome::Closed),
            OPEN if self.folder().is_dir() => {
                self.open(context);
                Step::Stay
            }
            _ => Step::Stay,
        }
    }
}

impl Dialog for HostInfoDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let info = ui.tr.tr(&Message::new(
            "{p0}\n{p2}\nSe ejecuta con el entorno del profile de MITO seleccionado, incluido APPDATA.",
        )
        .arg("p0", name)
        .arg("p2", self.path.display()));
        let location = ui
            .tr
            .tr(&Message::new("Carpeta del ejecutable\n{p1}").arg("p1", self.folder().display()));
        let missing =
            (!self.path.is_file()).then(|| ui.tr.text("El archivo ya no está disponible."));
        let outer = Size::PercentMax(90, 110).resolve(screen.width);
        let inner_width = outer.saturating_sub(6);
        let text_width = usize::from(inner_width.saturating_sub(BUTTON_WIDTH + 2));
        let rows = header::HEIGHT
            + text::height(&info, usize::from(inner_width))
            + 1
            + text::height(&location, text_width).max(3)
            + missing.map_or(0, |line| 1 + text::height(line, usize::from(inner_width)));
        let area = modal::area(screen, Size::PercentMax(90, 110), Size::Auto(rows + 4));
        let inner = modal::frame(frame, area, theme::BORDER);
        let inner = Rect::new(
            inner.x + 2,
            inner.y + 1,
            inner.width.saturating_sub(4),
            inner.height.saturating_sub(2),
        );
        let mut stack = Stack::new(inner);
        header::render(
            frame,
            stack.take(header::HEIGHT),
            ui,
            "APLICACIÓN DEL HOST",
            CLOSE,
        );
        stack.text(frame, &info, theme::panel());
        stack.gap(1);
        let row = stack.take(text::height(&location, text_width).max(3));
        let label = Rect::new(
            row.x,
            row.y,
            row.width.saturating_sub(BUTTON_WIDTH + 2),
            row.height,
        );
        frame.render_widget(
            Paragraph::new(
                text::wrap(&location, usize::from(label.width))
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .style(theme::panel()),
            label,
        );
        let button = Rect::new(
            row.right().saturating_sub(BUTTON_WIDTH),
            row.y + 1,
            BUTTON_WIDTH,
            1,
        );
        Button::new(OPEN, ui.tr.text("Abrir"))
            .disabled(!self.folder().is_dir())
            .render(frame, button, ui);
        if let Some(line) = missing {
            stack.gap(1);
            stack.text(frame, line, theme::panel());
        }
    }

    fn handle(&mut self, input: &Input, context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        if navigate(&mut self.focus, input, false) {
            return Step::Stay;
        }
        match pressed(&mut self.focus, input) {
            Some(id) => self.act(&id, context),
            None => Step::Stay,
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::dialogs::testing::Harness;

    fn existing() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Foo.exe");
        std::fs::write(&path, b"MZ").unwrap();
        (dir, path)
    }

    #[test]
    fn aspecto_con_el_archivo_disponible() {
        let harness = Harness::new();
        let mut dialog = HostInfoDialog::new(PathBuf::from(r"C:\Apps\Foo\Foo.exe"));
        // La carpeta de ejemplo no existe: el archivo no está disponible y «Abrir» se
        // deshabilita.
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn muestra_nombre_ruta_y_carpeta_en_espanol() {
        let harness = Harness::spanish();
        let (dir, path) = existing();
        let mut dialog = HostInfoDialog::new(path.clone());
        let text = harness.draw(&mut dialog, 120, 24);
        assert!(text.contains("APLICACIÓN DEL HOST"), "{text}");
        assert!(text.contains("Foo.exe"), "{text}");
        assert!(text.contains("incluido APPDATA."), "{text}");
        assert!(text.contains("Carpeta del ejecutable"), "{text}");
        assert!(!text.contains("ya no está disponible"), "{text}");
        assert!(dialog.focus().is_registered(OPEN));
        drop(dir);
    }

    #[test]
    fn con_el_archivo_ausente_avisa_y_deshabilita_abrir_si_no_hay_carpeta() {
        let harness = Harness::spanish();
        let mut dialog = HostInfoDialog::new(PathBuf::from(r"Z:\nada\Foo.exe"));
        let text = harness.draw(&mut dialog, 120, 24);
        assert!(text.contains("El archivo ya no está disponible."), "{text}");
        assert!(!dialog.focus().is_registered(OPEN));
    }

    #[test]
    fn abrir_pide_la_carpeta_del_ejecutable_y_los_errores_son_avisos() {
        let mut harness = Harness::new();
        let (dir, path) = existing();
        let seen: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let sink = Arc::clone(&seen);
        let mut dialog = HostInfoDialog::new(path).with_opener(move |folder| {
            sink.lock().unwrap().push(folder.to_path_buf());
            Ok(())
        });
        harness.draw(&mut dialog, 120, 24);
        assert_eq!(harness.click(&mut dialog, OPEN), Step::Stay);
        assert_eq!(*seen.lock().unwrap(), [dir.path().to_path_buf()]);

        let mut dialog = HostInfoDialog::new(dir.path().join("Foo.exe"))
            .with_opener(|_| Err(mito_core::Error::domain(Message::new("sin explorador"))));
        harness.draw(&mut dialog, 120, 24);
        harness.click(&mut dialog, OPEN);
        assert_eq!(harness.notices.len(), 1);
        assert_eq!(harness.notices[0].severity, Severity::Error);
    }

    #[test]
    fn la_cruz_y_esc_cierran() {
        let mut harness = Harness::new();
        let mut dialog = HostInfoDialog::new(PathBuf::from("Foo.exe"));
        harness.draw(&mut dialog, 120, 24);
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Closed)
        );
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
    }
}
