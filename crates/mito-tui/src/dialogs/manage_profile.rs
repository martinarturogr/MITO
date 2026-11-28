//! Diálogo «Gestionar profile» (`ManageProfileDialog`).

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::{Context, Dialog, Outcome, ProfileChoice, Step, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Button, Focus, Stack, TextInput, Ui, Variant, WidgetEvent, header};

const CLOSE: &str = "manage-profile-close";
const NEW_PROFILE: &str = "new-profile";
const CREATE: &str = "create";
const SHORTCUT: &str = "mito-shortcut";
const CONFIGURATION: &str = "profile-configuration";
const DELETE: &str = "delete-profile";

/// «GESTIONAR PROFILE». Ofrece «Crear profile», «Acceso directo» (F8), «Configuración» y
/// «Eliminar profile» (Exportar, Importar y Archivos de configuración llegan en fases
/// posteriores). Se cierra con [`Outcome::Profile`]; `Esc` y `×` dan
/// [`ProfileChoice::Cancel`].
#[derive(Debug)]
pub struct ManageProfileDialog {
    focus: Focus,
    profile: Option<String>,
    can_delete: bool,
    name: TextInput,
}

impl ManageProfileDialog {
    /// Diálogo para `profile` (ninguno si no hay selección). `can_delete` es cierto
    /// si hay un profile seleccionado y existe más de uno.
    pub fn new(profile: Option<&str>, can_delete: bool) -> Self {
        Self {
            focus: Focus::starting_at(NEW_PROFILE),
            profile: profile.map(str::to_owned),
            can_delete: can_delete && profile.is_some(),
            name: TextInput::new(NEW_PROFILE),
        }
    }

    fn choose(choice: ProfileChoice) -> Step {
        Step::Close(Outcome::Profile(choice))
    }

    fn create(&mut self) -> Step {
        let name = self.name.value.trim().to_owned();
        if name.is_empty() {
            // Sin mensaje: solo se devuelve el foco al campo, como el original.
            self.focus.set(NEW_PROFILE);
            return Step::Stay;
        }
        Self::choose(ProfileChoice::Create(name))
    }

    fn act(&mut self, id: &str) -> Step {
        match id {
            CLOSE => Self::choose(ProfileChoice::Cancel),
            CREATE => self.create(),
            SHORTCUT if self.profile.is_some() => Self::choose(ProfileChoice::Shortcut),
            CONFIGURATION if self.profile.is_some() => Self::choose(ProfileChoice::Configuration),
            DELETE if self.can_delete => Self::choose(ProfileChoice::Delete),
            _ => Step::Stay,
        }
    }
}

impl Dialog for ManageProfileDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let area = modal::area(screen, Size::Cells(62), Size::Auto(15));
        let inner = modal::frame(frame, area, theme::BORDER);
        let mut stack = Stack::new(inner);
        header::render(
            frame,
            stack.take(header::HEIGHT),
            ui,
            "GESTIONAR PROFILE",
            CLOSE,
        );
        let shown = self
            .profile
            .clone()
            .unwrap_or_else(|| ui.tr.text("Ningún profile seleccionado").to_owned());
        stack.centered(frame, &shown, theme::title());
        stack.gap(1);
        self.name.placeholder = ui.tr.text("Nombre del nuevo profile").to_owned();
        self.name.render(frame, stack.take(1), ui);
        stack.gap(1);
        Button::new(CREATE, ui.tr.text("Crear profile"))
            .variant(Variant::Primary)
            .render(frame, stack.take(1), ui);
        stack.gap(1);
        Button::new(SHORTCUT, ui.tr.text("Acceso directo"))
            .disabled(self.profile.is_none())
            .render(frame, stack.take(1), ui);
        stack.gap(1);
        Button::new(CONFIGURATION, ui.tr.text("Configuración"))
            .disabled(self.profile.is_none())
            .render(frame, stack.take(1), ui);
        stack.gap(1);
        Button::new(DELETE, ui.tr.text("Eliminar profile"))
            .variant(Variant::Error)
            .disabled(!self.can_delete)
            .render(frame, stack.take(1), ui);
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return Self::choose(ProfileChoice::Cancel);
        }
        let focused = self.focus.focused().map(str::to_owned);
        let in_field = focused.as_deref() == Some(NEW_PROFILE);
        // Fuera del campo, las flechas también recorren los botones.
        if navigate(&mut self.focus, input, !in_field) {
            return Step::Stay;
        }
        if !in_field && input.is(KeyCode::Down) {
            self.focus.next();
            return Step::Stay;
        }
        if !in_field && input.is(KeyCode::Up) {
            self.focus.previous();
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.focus.set(&id);
            if id == NEW_PROFILE {
                self.name.click(*column);
                return Step::Stay;
            }
            return self.act(&id);
        }
        if in_field {
            return match self.name.handle(input) {
                Some(WidgetEvent::Submitted(_)) => self.create(),
                _ => Step::Stay,
            };
        }
        match pressed(&mut self.focus, input) {
            Some(id) => self.act(&id),
            None => Step::Stay,
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;

    #[test]
    fn aspecto_con_profile() {
        let harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("trabajo"), true);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
    }

    #[test]
    fn aspecto_sin_profile_deshabilita_configuracion_y_eliminar() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(None, false);
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 24));
        assert!(!dialog.focus().is_registered(SHORTCUT));
        assert!(!dialog.focus().is_registered(CONFIGURATION));
        assert!(!dialog.focus().is_registered(DELETE));
        assert_eq!(harness.click(&mut dialog, CREATE), Step::Stay);
    }

    #[test]
    fn no_ofrece_lo_de_fases_posteriores() {
        let harness = Harness::spanish();
        let mut dialog = ManageProfileDialog::new(Some("uno"), true);
        let text = harness.draw(&mut dialog, 80, 24);
        for hidden in ["Exportar", "Importar", "Archivos de configuración"] {
            assert!(!text.contains(hidden), "{hidden}: {text}");
        }
    }

    #[test]
    fn crear_con_el_nombre_recortado() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("uno"), false);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(NEW_PROFILE));
        harness.type_text(&mut dialog, "  dos ");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Profile(ProfileChoice::Create("dos".into())))
        );
        let mut dialog = ManageProfileDialog::new(Some("uno"), false);
        harness.draw(&mut dialog, 80, 24);
        harness.type_text(&mut dialog, "tres");
        assert_eq!(
            harness.click(&mut dialog, CREATE),
            Step::Close(Outcome::Profile(ProfileChoice::Create("tres".into())))
        );
    }

    #[test]
    fn crear_con_nombre_vacio_enfoca_el_campo_sin_cerrar() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("uno"), false);
        harness.draw(&mut dialog, 80, 24);
        harness.type_text(&mut dialog, "   ");
        harness.focus_on(&mut dialog, CREATE);
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        assert_eq!(dialog.focus().focused(), Some(NEW_PROFILE));
        assert!(harness.notices.is_empty(), "sin mensaje de error");
    }

    #[test]
    fn configuracion_y_eliminar_devuelven_su_eleccion() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("uno"), true);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, CONFIGURATION),
            Step::Close(Outcome::Profile(ProfileChoice::Configuration))
        );
        let mut dialog = ManageProfileDialog::new(Some("uno"), true);
        harness.draw(&mut dialog, 80, 24);
        harness.focus_on(&mut dialog, DELETE);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Profile(ProfileChoice::Delete))
        );
    }

    #[test]
    fn acceso_directo_devuelve_su_eleccion() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("uno"), true);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.click(&mut dialog, SHORTCUT),
            Step::Close(Outcome::Profile(ProfileChoice::Shortcut))
        );
    }

    #[test]
    fn esc_y_la_cruz_cancelan() {
        let mut harness = Harness::new();
        let mut dialog = ManageProfileDialog::new(Some("uno"), true);
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Profile(ProfileChoice::Cancel))
        );
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Profile(ProfileChoice::Cancel))
        );
    }
}
