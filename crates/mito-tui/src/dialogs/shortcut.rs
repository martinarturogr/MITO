//! Diálogo «Generar acceso directo» (`ShortcutDialog`).

use crossterm::event::KeyCode;
use mito_core::config::badge_text;
use mito_desktop::icons::BADGE_COLORS;
use mito_desktop::shortcuts;
use mito_engine::arguments::split;
use mito_engine::{Engine, Result};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{
    Context, Dialog, Outcome, Step, describe_core, drive_select, is_attached, navigate,
    open_select, process_select,
};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::text;
use crate::widgets::{Focus, Select, TextInput, Ui, Variant, WidgetEvent};

const CANCEL: &str = "shortcut-cancel";
const CONFIRM: &str = "shortcut-confirm";
const ENTRY: &str = "shortcut-entry";
const ARGUMENTS: &str = "shortcut-arguments";
const PROCESS: &str = "shortcut-coupled";
const BADGE: &str = "shortcut-badge";
const COLOR_PREFIX: &str = "shortcut-color-";
/// Columnas de la paleta de colores.
const COLOR_COLUMNS: usize = 4;

/// Lo que se eligió en «Generar acceso directo».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutChoice {
    /// Letras del icono, ya validadas y en mayúsculas.
    pub badge: String,
    /// Clave del color de la paleta (`naranja`, `azul`…).
    pub color: String,
    /// Proceso `attached`: abrir en una pestaña de la terminal de MITO.
    pub coupled: bool,
    /// Argumentos ya partidos (vacíos si el diálogo no es de aplicación).
    pub arguments: Vec<String>,
    /// Entrada de aplicación elegida, si había varias.
    pub entry: Option<String>,
}

/// «Generar acceso directo»: letras del icono, color de la paleta y, para una aplicación,
/// entrada, argumentos y proceso (`detached`/`attached`). Se cierra con
/// [`Outcome::Shortcut`]; «Cancelar» y Esc dan [`Outcome::Closed`].
#[derive(Debug)]
pub struct ShortcutDialog {
    prompt: Prompt,
    application: bool,
    entry: Option<Select<String>>,
    arguments: TextInput,
    process: Select<String>,
    badge: TextInput,
    color: String,
    error: String,
}

/// Abre el color de la paleta `name` como texto de botón: `Naranja`.
fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

impl ShortcutDialog {
    /// Diálogo con la etiqueta y el color por defecto de `config.properties`
    /// (`shortcut.badgeText` y `shortcut.badgeColor`). `entries` son las entradas de
    /// aplicación del manifiesto.
    ///
    /// # Errors
    ///
    /// Un valor no válido en `config.properties` (`La etiqueta admite de 1 a 4 letras o
    /// números`, `Elige un color de la paleta del icono`) o su lectura.
    pub fn new(engine: &Engine, application: bool, entries: Vec<String>) -> Result<Self> {
        let badge = shortcuts::badge_text(engine, None)?;
        let color = shortcuts::badge_color(engine, None)?;
        Ok(Self::with_defaults(application, entries, &badge, &color))
    }

    /// Diálogo con la etiqueta y el color por defecto dados.
    pub fn with_defaults(
        application: bool,
        entries: Vec<String>,
        badge: &str,
        color: &str,
    ) -> Self {
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Generar",
                variant: Variant::Primary,
            },
        ];
        let entry = (!entries.is_empty()).then(|| {
            let options = entries.into_iter().map(|e| (e.clone(), e)).collect();
            Select::new(ENTRY, options, Some(0))
        });
        let initial = if entry.is_some() {
            ENTRY
        } else if application {
            ARGUMENTS
        } else {
            BADGE
        };
        Self {
            prompt: Prompt::new(buttons, initial, Size::Cells(54), theme::BORDER),
            application,
            entry,
            arguments: TextInput::new(ARGUMENTS),
            process: process_select(PROCESS),
            badge: TextInput::new(BADGE).with_value(badge).with_max_length(4),
            color: color.to_owned(),
            error: String::new(),
        }
    }

    /// Valida y cierra, o deja el error en pantalla.
    fn confirm(&mut self, context: &Context) -> Step {
        let badge = match badge_text(&self.badge.value) {
            Ok(badge) => badge,
            Err(error) => {
                self.error = describe_core(&error, context.tr);
                return Step::Stay;
            }
        };
        if !BADGE_COLORS.iter().any(|(name, _)| *name == self.color) {
            context
                .tr
                .text("Elige un color de la paleta del icono")
                .clone_into(&mut self.error);
            return Step::Stay;
        }
        let (coupled, arguments) = if self.application {
            (is_attached(&self.process), split(&self.arguments.value))
        } else {
            (false, Vec::new())
        };
        Step::Close(Outcome::Shortcut(ShortcutChoice {
            badge,
            color: self.color.clone(),
            coupled,
            arguments,
            entry: self
                .entry
                .as_ref()
                .and_then(|select| select.value().cloned()),
        }))
    }

    fn on_button(&mut self, id: &str, context: &Context) -> Option<Step> {
        if let Some(name) = id.strip_prefix(COLOR_PREFIX) {
            if BADGE_COLORS.iter().any(|(color, _)| *color == name) {
                name.clone_into(&mut self.color);
            }
            return Some(Step::Stay);
        }
        match id {
            CANCEL => Some(Step::Close(Outcome::Closed)),
            CONFIRM => Some(self.confirm(context)),
            _ => None,
        }
    }

    fn selects(&mut self) -> Vec<&mut Select<String>> {
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        if self.application {
            selects.push(&mut self.process);
        }
        selects
    }
}

/// Botón de color: fondo del color, texto blanco, `✓ ` si es el elegido.
fn color_button(
    frame: &mut Frame,
    area: Rect,
    ui: &mut Ui,
    name: &str,
    rgb: [u8; 3],
    chosen: bool,
) {
    let id = format!("{COLOR_PREFIX}{name}");
    let mut style = Style::new()
        .fg(Color::White)
        .bg(Color::Rgb(rgb[0], rgb[1], rgb[2]));
    if chosen {
        style = style.add_modifier(Modifier::BOLD);
    }
    if ui.is_focused(&id) {
        style = style.add_modifier(Modifier::REVERSED);
    }
    let label = if chosen {
        format!("✓ {}", capitalize(name))
    } else {
        capitalize(name)
    };
    let shown = text::truncate(&label, usize::from(area.width));
    frame.buffer_mut().set_style(area, style);
    frame.render_widget(Paragraph::new(shown).centered().style(style), area);
    ui.focusable(&id, area);
}

impl Dialog for ShortcutDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content
            .text(ui.tr.text("Generar acceso directo"), theme::title())
            .gap(1);
        if self.entry.is_some() {
            content.slot("entry", 1).gap(1);
        }
        if self.application {
            content
                .text(ui.tr.text("Argumentos"), theme::panel())
                .slot("arguments", 1)
                .gap(1)
                .text("Proceso", theme::panel())
                .slot("process", 1)
                .gap(1);
        }
        content
            .text(ui.tr.text("Letras del icono (máximo 4)"), theme::panel())
            .slot("badge", 1)
            .gap(1)
            .text(ui.tr.text("Color del fondo"), theme::panel())
            .slot("colors", 2);
        if !self.error.is_empty() {
            content.text(
                &self.error,
                Style::new().fg(theme::BADGE_ERROR).bg(theme::SIDEBAR),
            );
        }
        let (entry, arguments, process, badge) =
            (&self.entry, &self.arguments, &self.process, &self.badge);
        let (application, chosen) = (self.application, self.color.clone());
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                if let Some(select) = entry {
                    select.render(frame, slots.get("entry"), ui);
                }
                if application {
                    arguments.render(frame, slots.get("arguments"), ui);
                    process.render(frame, slots.get("process"), ui);
                }
                badge.render(frame, slots.get("badge"), ui);
                let rows = Layout::vertical([Constraint::Length(1); 2]).split(slots.get("colors"));
                for (line, row) in rows.iter().enumerate() {
                    let cells = Layout::horizontal([Constraint::Fill(1); COLOR_COLUMNS])
                        .spacing(1)
                        .split(*row);
                    for (cell, (name, rgb)) in cells
                        .iter()
                        .zip(BADGE_COLORS.iter().skip(line * COLOR_COLUMNS))
                    {
                        color_button(frame, *cell, ui, name, *rgb, *name == chosen);
                    }
                }
            });
        if let Some(select) = &self.entry {
            select.render_overlay(frame, ui);
        }
        if self.application {
            self.process.render_overlay(frame, ui);
        }
    }

    fn handle(&mut self, input: &Input, context: &mut Context) -> Step {
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        if self.application {
            selects.push(&mut self.process);
        }
        if open_select(&mut selects, &self.prompt.focus, input).is_some() {
            return Step::Stay;
        }
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        let focused = self.prompt.focus.focused().map(str::to_owned);
        let in_field = matches!(
            focused.as_deref(),
            Some(ARGUMENTS | BADGE | ENTRY | PROCESS)
        );
        if navigate(&mut self.prompt.focus, input, !in_field) {
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.prompt.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.prompt.focus.set(&id);
            if drive_select(&mut self.selects(), &id, input).is_some() {
                return Step::Stay;
            }
            match id.as_str() {
                ARGUMENTS => self.arguments.click(*column),
                BADGE => self.badge.click(*column),
                other => return self.on_button(other, context).unwrap_or(Step::Stay),
            }
            return Step::Stay;
        }
        let Some(id) = focused else {
            return Step::Stay;
        };
        if drive_select(&mut self.selects(), &id, input).is_some() {
            return Step::Stay;
        }
        match id.as_str() {
            ARGUMENTS => match self.arguments.handle(input) {
                Some(WidgetEvent::Submitted(_)) => self.confirm(context),
                _ => Step::Stay,
            },
            BADGE => match self.badge.handle(input) {
                // En un diálogo de aplicación, Intro pasa a los argumentos.
                Some(WidgetEvent::Submitted(_)) if self.application => {
                    self.prompt.focus.set(ARGUMENTS);
                    Step::Stay
                }
                Some(WidgetEvent::Submitted(_)) => self.confirm(context),
                _ => Step::Stay,
            },
            other if input.is_activation() => self.on_button(other, context).unwrap_or(Step::Stay),
            _ => Step::Stay,
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.prompt.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;

    fn application() -> ShortcutDialog {
        ShortcutDialog::with_defaults(true, vec![], "MITO", "naranja")
    }

    fn mito() -> ShortcutDialog {
        ShortcutDialog::with_defaults(false, vec![], "MITO", "naranja")
    }

    #[test]
    fn aspecto_de_aplicacion() {
        let harness = Harness::spanish();
        let mut dialog = ShortcutDialog::with_defaults(
            true,
            vec!["Tool".into(), "Tool Help".into()],
            "MITO",
            "azul",
        );
        insta::assert_snapshot!(harness.draw(&mut dialog, 80, 34));
    }

    #[test]
    fn aspecto_de_mito_sin_proceso_ni_argumentos() {
        let harness = Harness::spanish();
        let mut dialog = mito();
        let text = harness.draw(&mut dialog, 80, 30);
        assert!(
            !text.contains("Proceso") && !text.contains("Argumentos"),
            "{text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn la_paleta_tiene_ocho_botones_con_su_rgb_y_la_marca_en_el_elegido() {
        let harness = Harness::spanish();
        let mut dialog = application();
        let buffer = harness.buffer(&mut dialog, 80, 34);
        let text = crate::widgets::testing::buffer_text(&buffer);
        for name in [
            "Negro", "Rojo", "Azul", "Naranja", "Rosa", "Verde", "Marron", "Morado",
        ] {
            assert!(text.contains(name), "{name}: {text}");
        }
        assert!(text.contains("✓ Naranja"), "{text}");
        assert_eq!(text.matches('✓').count(), 1);
        for (name, [r, g, b]) in BADGE_COLORS {
            let area = dialog
                .focus()
                .area_of(&format!("{COLOR_PREFIX}{name}"))
                .unwrap();
            assert_eq!(buffer[(area.x, area.y)].bg, Color::Rgb(r, g, b), "{name}");
            assert_eq!(buffer[(area.x, area.y)].fg, Color::White, "{name}");
        }
    }

    #[test]
    fn elegir_un_color_mueve_la_marca_y_se_devuelve() {
        let mut harness = Harness::spanish();
        let mut dialog = application();
        harness.draw(&mut dialog, 80, 34);
        assert_eq!(
            harness.click(&mut dialog, "shortcut-color-azul"),
            Step::Stay
        );
        let text = harness.draw(&mut dialog, 80, 34);
        assert!(
            text.contains("✓ Azul") && !text.contains("✓ Naranja"),
            "{text}"
        );
        assert_eq!(
            harness.click(&mut dialog, CONFIRM),
            Step::Close(Outcome::Shortcut(ShortcutChoice {
                badge: "MITO".into(),
                color: "azul".into(),
                coupled: false,
                arguments: vec![],
                entry: None,
            }))
        );
    }

    #[test]
    fn la_etiqueta_se_recorta_a_mayusculas_y_cuatro_caracteres() {
        let mut harness = Harness::spanish();
        let mut dialog = mito();
        harness.draw(&mut dialog, 80, 30);
        assert_eq!(dialog.focus().focused(), Some(BADGE));
        for _ in 0..4 {
            harness.press(&mut dialog, KeyCode::Backspace);
        }
        harness.type_text(&mut dialog, " dev12345");
        // `max_length` es 4: el campo solo guarda las cuatro primeras.
        assert_eq!(dialog.badge.value, " dev");
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Shortcut(ShortcutChoice {
                badge: "DEV".into(),
                color: "naranja".into(),
                coupled: false,
                arguments: vec![],
                entry: None,
            }))
        );
    }

    #[test]
    fn una_etiqueta_vacia_es_mito_y_una_no_alfanumerica_es_un_error_sin_cerrar() {
        let mut harness = Harness::spanish();
        let mut dialog = mito();
        harness.draw(&mut dialog, 80, 30);
        for _ in 0..4 {
            harness.press(&mut dialog, KeyCode::Backspace);
        }
        let Step::Close(Outcome::Shortcut(choice)) = harness.press(&mut dialog, KeyCode::Enter)
        else {
            panic!("debía cerrar");
        };
        assert_eq!(choice.badge, "MITO");

        let mut dialog = mito();
        harness.draw(&mut dialog, 80, 30);
        for _ in 0..4 {
            harness.press(&mut dialog, KeyCode::Backspace);
        }
        harness.type_text(&mut dialog, "-");
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        let text = harness.draw(&mut dialog, 80, 30);
        assert!(
            text.contains("La etiqueta admite de 1 a 4 letras o números"),
            "{text}"
        );
    }

    #[test]
    fn en_una_aplicacion_intro_en_la_etiqueta_pasa_a_los_argumentos_y_alli_confirma() {
        let mut harness = Harness::spanish();
        let mut dialog = application();
        harness.draw(&mut dialog, 80, 34);
        assert_eq!(dialog.focus().focused(), Some(ARGUMENTS));
        harness.focus_on(&mut dialog, BADGE);
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        assert_eq!(dialog.focus().focused(), Some(ARGUMENTS));
        harness.type_text(&mut dialog, "--x \"dos palabras\"");
        harness.focus_on(&mut dialog, PROCESS);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 80, 34);
        assert_eq!(
            harness.click(&mut dialog, CONFIRM),
            Step::Close(Outcome::Shortcut(ShortcutChoice {
                badge: "MITO".into(),
                color: "naranja".into(),
                coupled: true,
                arguments: vec!["--x".into(), "dos palabras".into()],
                entry: None,
            }))
        );
    }

    #[test]
    fn con_entradas_devuelve_la_elegida() {
        let mut harness = Harness::spanish();
        let mut dialog =
            ShortcutDialog::with_defaults(true, vec!["Tool".into(), "Help".into()], "TL", "rojo");
        harness.draw(&mut dialog, 80, 34);
        assert_eq!(dialog.focus().focused(), Some(ENTRY));
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Down);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 80, 34);
        let Step::Close(Outcome::Shortcut(choice)) = harness.click(&mut dialog, CONFIRM) else {
            panic!("debía cerrar");
        };
        assert_eq!(choice.entry.as_deref(), Some("Help"));
        assert_eq!(choice.badge, "TL");
        assert_eq!(choice.color, "rojo");
    }

    #[test]
    fn cancelar_y_esc_cierran_sin_resultado() {
        let mut harness = Harness::spanish();
        let mut dialog = mito();
        harness.draw(&mut dialog, 80, 30);
        assert_eq!(
            harness.click(&mut dialog, CANCEL),
            Step::Close(Outcome::Closed)
        );
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
    }

    #[test]
    fn los_valores_por_defecto_salen_de_config_properties() {
        let harness = Harness::spanish();
        std::fs::create_dir_all(harness.engine.layout().config_file().parent().unwrap()).unwrap();
        std::fs::write(
            harness.engine.layout().config_file(),
            "shortcut.badgeText=dev\nshortcut.badgeColor=Verde\n",
        )
        .unwrap();
        let dialog = ShortcutDialog::new(&harness.engine, false, vec![]).unwrap();
        assert_eq!(dialog.badge.value, "DEV");
        assert_eq!(dialog.color, "verde");
        std::fs::write(
            harness.engine.layout().config_file(),
            "shortcut.badgeColor=fucsia\n",
        )
        .unwrap();
        assert!(ShortcutDialog::new(&harness.engine, false, vec![]).is_err());
    }
}
