//! Credenciales del portal cautivo (`ProxyDialog` de `network_ui.py`).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crossterm::event::KeyCode;
use mito_engine::network::Credentials;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::{Context, Dialog, Outcome, Step, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Button, Focus, Stack, TextInput, Ui, Variant, WidgetEvent, text};

/// Id del campo «Usuario».
pub const USER: &str = "proxy-user";
/// Id del campo «Contraseña».
pub const PASSWORD: &str = "proxy-password";
/// Id del botón «Conectar».
pub const SUBMIT: &str = "proxy-submit";
/// Id del botón «Cancelar».
pub const CANCEL: &str = "proxy-cancel";

/// Entra en el portal con las credenciales. Corre en un hilo aparte. `Ok(false)`: no se
/// pudo verificar Internet; `Err`: el texto (ya traducido) del error de configuración.
pub type Authenticator = Arc<dyn Fn(Credentials) -> Result<bool, String> + Send + Sync>;

/// Qué muestra `#proxy-result`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    /// Nada.
    Idle,
    /// `Conectando…` mientras corre el hilo.
    Connecting,
    /// El portal no dejó entrar o Internet sigue sin responder.
    Rejected,
    /// Error de configuración de `authenticate` (F1): su texto.
    Failed(String),
}

/// «AUTENTICACIÓN DE RED»: usuario y contraseña (enmascarada) para esta sesión. Se cierra
/// con [`Outcome::Confirmed`] (`true` = autenticado); Esc y «Cancelar» dan `false`.
///
/// La contraseña se saca del campo al enviar (queda vacío) y viaja solo en un
/// [`Credentials`] hasta el hilo; el diálogo no la conserva ni la muestra nunca.
pub struct ProxyDialog {
    focus: Focus,
    user: TextInput,
    password: TextInput,
    status: Status,
    authenticate: Authenticator,
    pending: Option<Receiver<Result<bool, String>>>,
}

impl ProxyDialog {
    /// Diálogo que autentica con `authenticate`.
    pub fn new(authenticate: Authenticator) -> Self {
        Self {
            focus: Focus::starting_at(USER),
            user: TextInput::new(USER),
            password: TextInput::new(PASSWORD).with_mask(),
            status: Status::Idle,
            authenticate,
            pending: None,
        }
    }

    /// ¿Hay un envío en curso (botón «Conectar» deshabilitado)?
    pub fn connecting(&self) -> bool {
        self.pending.is_some()
    }

    /// `action_cancel`: vacía la contraseña y se cierra sin autenticar. Un envío en curso
    /// se abandona (su resultado ya no tiene a quién llegar).
    fn cancel(&mut self) -> Step {
        self.password.wipe();
        self.pending = None;
        Step::Close(Outcome::Confirmed(false))
    }

    /// «Conectar»: vacía la contraseña del campo antes de nada, deshabilita el botón y
    /// autentica en un hilo.
    fn submit(&mut self) -> Step {
        if self.pending.is_some() {
            return Step::Stay;
        }
        let username = self.user.value.clone();
        let password = self.password.take_value();
        self.status = Status::Connecting;
        let (tx, rx) = channel();
        self.pending = Some(rx);
        let authenticate = Arc::clone(&self.authenticate);
        std::thread::spawn(move || {
            let result = authenticate(Credentials::new(username, password));
            // Sin receptor (diálogo cerrado): nadie espera el resultado.
            let _ = tx.send(result);
        });
        Step::Stay
    }

    fn act(&mut self, id: &str) -> Step {
        match id {
            SUBMIT => self.submit(),
            CANCEL => self.cancel(),
            _ => Step::Stay,
        }
    }

    fn result_text(&self, ui: &Ui) -> String {
        match &self.status {
            Status::Idle => String::new(),
            Status::Connecting => ui.tr.text("Conectando…").to_owned(),
            Status::Rejected => ui
                .tr
                .text("No se pudo verificar Internet. Puedes reintentar o cancelar.")
                .to_owned(),
            Status::Failed(text) => text.clone(),
        }
    }
}

impl Dialog for ProxyDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let result = self.result_text(ui);
        let width = Size::Cells(66);
        let inner_width = width.resolve(screen.width).saturating_sub(4).max(1);
        let result_rows = u16::try_from(text::wrap(&result, usize::from(inner_width)).len())
            .unwrap_or(u16::MAX)
            .max(1);
        // Borde, título, texto, hueco, usuario, hueco, contraseña, resultado, hueco,
        // «Conectar», hueco, «Cancelar».
        let height = 2 + 1 + 1 + 1 + 1 + 1 + 1 + result_rows + 1 + 1 + 1 + 1;
        let area = modal::area(screen, width, Size::Auto(height));
        let inner = modal::frame(frame, area, theme::BORDER);
        let mut stack = Stack::new(inner);
        stack.text(frame, ui.tr.text("AUTENTICACIÓN DE RED"), theme::title());
        stack.text(
            frame,
            ui.tr
                .text("Las credenciales no se guardan. Escape cancela."),
            theme::panel(),
        );
        stack.gap(1);
        self.user.placeholder = ui.tr.text("Usuario").to_owned();
        self.user.render(frame, stack.take(1), ui);
        stack.gap(1);
        self.password.placeholder = ui.tr.text("Contraseña").to_owned();
        self.password.render(frame, stack.take(1), ui);
        stack.text(frame, &result, theme::panel());
        stack.gap(1);
        Button::new(SUBMIT, ui.tr.text("Conectar"))
            .variant(Variant::Primary)
            .disabled(self.pending.is_some())
            .render(frame, stack.take(1), ui);
        stack.gap(1);
        Button::new(CANCEL, ui.tr.text("Cancelar")).render(frame, stack.take(1), ui);
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return self.cancel();
        }
        let focused = self.focus.focused().map(str::to_owned);
        let field = focused.filter(|id| id == USER || id == PASSWORD);
        if navigate(&mut self.focus, input, field.is_none()) {
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.focus.set(&id);
            return match id.as_str() {
                USER => {
                    self.user.click(*column);
                    Step::Stay
                }
                PASSWORD => {
                    self.password.click(*column);
                    Step::Stay
                }
                other => self.act(other),
            };
        }
        if let Some(id) = field {
            let target = if id == USER {
                &mut self.user
            } else {
                &mut self.password
            };
            // Intro envía, como el `AcceptButton` de WinForms (decisión del port, D3).
            return match target.handle(input) {
                Some(WidgetEvent::Submitted(_)) => self.submit(),
                _ => Step::Stay,
            };
        }
        match pressed(&mut self.focus, input) {
            Some(id) => self.act(&id),
            None => Step::Stay,
        }
    }

    fn tick(&mut self, _context: &mut Context) -> Step {
        let Some(receiver) = &self.pending else {
            return Step::Stay;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Step::Stay,
            // El hilo terminó sin responder (pánico): igual que un rechazo.
            Err(TryRecvError::Disconnected) => Ok(false),
        };
        self.pending = None;
        match result {
            Ok(true) => Step::Close(Outcome::Confirmed(true)),
            Ok(false) => {
                self.status = Status::Rejected;
                Step::Stay
            }
            Err(text) => {
                self.status = Status::Failed(text);
                Step::Stay
            }
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::dialogs::testing::Harness;

    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    fn dialog_with(result: Result<bool, String>) -> (ProxyDialog, Seen) {
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        let authenticate: Authenticator = Arc::new(move |credentials| {
            log.lock().unwrap().push((
                credentials.username.clone(),
                credentials.password.to_string(),
            ));
            result.clone()
        });
        (ProxyDialog::new(authenticate), seen)
    }

    /// Da tics hasta que el diálogo se cierre o deje de estar conectando.
    fn settle(harness: &mut Harness, dialog: &mut ProxyDialog) -> Step {
        let limit = Instant::now() + Duration::from_secs(10);
        loop {
            let step = harness.tick(dialog);
            if step != Step::Stay || !dialog.connecting() {
                return step;
            }
            assert!(Instant::now() < limit, "el hilo no respondió");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn fill(harness: &mut Harness, dialog: &mut ProxyDialog, user: &str, password: &str) {
        harness.draw(dialog, 100, 24);
        harness.type_text(dialog, user);
        harness.press(dialog, KeyCode::Tab);
        harness.type_text(dialog, password);
    }

    #[test]
    fn aspecto() {
        let harness = Harness::spanish();
        let (mut dialog, _) = dialog_with(Ok(true));
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn aspecto_con_la_contrasena_enmascarada_y_el_fallo() {
        let mut harness = Harness::spanish();
        let (mut dialog, _) = dialog_with(Ok(false));
        fill(&mut harness, &mut dialog, "ana", "secreto");
        insta::assert_snapshot!("rellenado", harness.draw(&mut dialog, 100, 24));
        harness.press(&mut dialog, KeyCode::Enter);
        settle(&mut harness, &mut dialog);
        insta::assert_snapshot!("rechazado", harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn la_contrasena_se_dibuja_enmascarada() {
        let mut harness = Harness::spanish();
        let (mut dialog, _) = dialog_with(Ok(true));
        fill(&mut harness, &mut dialog, "ana", "secreto");
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(text.contains("ana"), "{text}");
        assert!(!text.contains("secreto"), "{text}");
        assert!(text.contains("•••••••"), "{text}");
        assert!(!format!("{:?}", dialog.password).contains("secreto"));
    }

    #[test]
    fn enviar_vacia_la_contrasena_y_deshabilita_el_boton() {
        let mut harness = Harness::spanish();
        let (mut dialog, seen) = dialog_with(Ok(true));
        fill(&mut harness, &mut dialog, "ana", "secreto");
        harness.focus_on(&mut dialog, SUBMIT);
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        assert!(
            dialog.password.is_empty(),
            "la contraseña ya no está en el campo"
        );
        assert!(dialog.connecting());
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(text.contains("Conectando…"), "{text}");
        assert!(!dialog.focus().is_registered(SUBMIT), "botón deshabilitado");
        assert_eq!(
            settle(&mut harness, &mut dialog),
            Step::Close(Outcome::Confirmed(true))
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![("ana".to_owned(), "secreto".to_owned())]
        );
    }

    #[test]
    fn intro_en_un_campo_envia() {
        let mut harness = Harness::spanish();
        let (mut dialog, seen) = dialog_with(Ok(true));
        harness.draw(&mut dialog, 100, 24);
        harness.type_text(&mut dialog, "ana");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(
            settle(&mut harness, &mut dialog),
            Step::Close(Outcome::Confirmed(true))
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn el_fallo_deja_reintentar_y_la_contrasena_hay_que_escribirla_de_nuevo() {
        let mut harness = Harness::spanish();
        let (mut dialog, seen) = dialog_with(Ok(false));
        fill(&mut harness, &mut dialog, "ana", "mala");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(settle(&mut harness, &mut dialog), Step::Stay);
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(
            text.contains("No se pudo verificar Internet. Puedes reintentar o cancelar."),
            "{text}"
        );
        assert!(dialog.password.is_empty());
        assert!(dialog.focus().is_registered(SUBMIT), "botón habilitado");
        harness.type_text(&mut dialog, "buena");
        harness.press(&mut dialog, KeyCode::Enter);
        settle(&mut harness, &mut dialog);
        assert_eq!(seen.lock().unwrap()[1].1, "buena");
    }

    #[test]
    fn el_error_de_configuracion_se_muestra_f1() {
        let mut harness = Harness::spanish();
        let texto = "El portal exige HTTPS para enviar la contraseña".to_owned();
        let (mut dialog, _) = dialog_with(Err(texto));
        fill(&mut harness, &mut dialog, "ana", "x");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(settle(&mut harness, &mut dialog), Step::Stay);
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(text.contains("El portal exige HTTPS"), "{text}");
        assert!(
            !text.contains("No se pudo verificar Internet"),
            "no el genérico\n{text}"
        );
    }

    #[test]
    fn escape_y_cancelar_borran_la_contrasena_y_cierran() {
        let mut harness = Harness::spanish();
        let (mut dialog, seen) = dialog_with(Ok(true));
        fill(&mut harness, &mut dialog, "ana", "secreto");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
        assert!(dialog.password.is_empty());
        let (mut dialog, _) = dialog_with(Ok(true));
        fill(&mut harness, &mut dialog, "ana", "secreto");
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(
            harness.click(&mut dialog, CANCEL),
            Step::Close(Outcome::Confirmed(false))
        );
        assert!(dialog.password.is_empty());
        assert!(seen.lock().unwrap().is_empty(), "nunca se envió");
    }

    #[test]
    fn cancelar_con_un_envio_en_curso_no_espera_el_resultado() {
        let mut harness = Harness::spanish();
        let (mut dialog, _) = dialog_with(Ok(true));
        fill(&mut harness, &mut dialog, "ana", "x");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
        assert!(!dialog.connecting());
    }
}
