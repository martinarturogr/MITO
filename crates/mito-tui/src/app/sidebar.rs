//! Barra lateral: profiles y acciones (`#sidebar`).

use mito_engine::Engine;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use super::state::App;
use crate::dialogs::describe_core;
use crate::theme;
use crate::widgets::{Button, Severity, Stack, Ui, Variant};
use crate::worker::Action;

/// Ruta de la carpeta `home` de la instancia.
fn home_folder(engine: &Engine) -> std::path::PathBuf {
    engine.root().join("home")
}

impl App {
    /// Dibuja la barra lateral en `area`.
    pub(super) fn render_sidebar(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        let tr = ui.tr;
        frame.buffer_mut().set_style(area, theme::panel());
        let inner = Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(1),
            area.width.saturating_sub(4),
            area.height.saturating_sub(1),
        );
        let busy = self.busy.active;
        let mut stack = Stack::new(inner);
        // Con poca altura se quitan las filas de separación entre botones.
        let gap = u16::from(inner.height >= 19);

        stack.text(frame, tr.text("PROFILES"), theme::title());
        self.profile.disabled = busy;
        let row = stack.take(1);
        self.profile.render(frame, row, ui);
        stack.gap(1);
        let row = stack.take(1);
        Button::new("manage-profile", tr.text("Gestionar profile"))
            .disabled(busy)
            .render(frame, row, ui);
        stack.gap(1 + gap);
        stack.text(frame, tr.text("ACCIONES"), theme::title());

        let buttons = [
            (
                "home-button",
                "Inicio · Instalados",
                Variant::Success,
                false,
            ),
            (
                "check-updates",
                "Buscar actualizaciones",
                Variant::Default,
                true,
            ),
            ("open-home", "Abrir Home", Variant::Default, false),
            ("configuration", "Configuración", Variant::Default, true),
        ];
        for (id, label, variant, disables) in buttons {
            stack.gap(gap);
            let row = stack.take(1);
            Button::new(id, tr.text(label))
                .variant(variant)
                .disabled(disables && busy)
                .render(frame, row, ui);
        }
        if self.busy.indicator {
            stack.gap(gap);
            let rect = stack.take(3);
            let style = Style::new()
                .fg(theme::BUSY)
                .bg(theme::SIDEBAR)
                .add_modifier(Modifier::BOLD);
            frame.render_widget(Paragraph::new("⏳ …").style(style), rect);
        }
    }

    /// Botones de la barra lateral. Devuelve `true` si el id era de ella.
    pub(super) fn sidebar_button(&mut self, id: &str) -> bool {
        match id {
            "manage-profile" => self.open_manage_profile(),
            "home-button" => self.go_home(),
            "check-updates" => self.request(Action::CheckUpdates),
            "open-home" => self.open_home_folder(),
            "configuration" => self.open_configuration(None),
            _ => return false,
        }
        true
    }

    /// «Abrir Home»: abre `root/home` en el Explorador.
    fn open_home_folder(&mut self) {
        let folder = home_folder(&self.engine);
        if let Err(error) = (self.opener)(&folder) {
            self.capture(&error, "home.open-location");
            let text = describe_core(&error, &self.tr);
            self.notify(text, Severity::Error);
        }
    }
}
