//! Confirmación de los cambios de red (`NetworkSetupDialog` de `network_ui.py`).

use mito_core::Message;
use mito_engine::network::{Diagnosis, DiagnosisKind};
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::layout::Content;
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{Focus, Ui, Variant};

/// Id de «Guardar y conectar».
pub const SAVE: &str = "network-setup-save";
/// Id de «Cancelar».
pub const CANCEL: &str = "network-setup-cancel";

/// Origen de la ruta traducido (`source_label`). Lo que no se conoce se devuelve igual.
pub fn source_label(source: &str, tr: &Translator) -> String {
    match source {
        "manual" => tr.text("configuración manual de Windows"),
        "pac" => tr.text("script PAC"),
        "wpad" => tr.text("autodetección WPAD"),
        other => other,
    }
    .to_owned()
}

/// «CONFIGURACIÓN DE RED»: la ruta detectada y las claves de `config.properties` que se
/// guardarían. Se cierra con [`Outcome::Confirmed`] (`true` = «Guardar y conectar»); Esc
/// cancela.
#[derive(Debug)]
pub struct NetworkSetupDialog {
    prompt: Prompt,
    proxy: String,
    source: String,
    kind: DiagnosisKind,
    changes: Vec<(String, String)>,
}

impl NetworkSetupDialog {
    /// Diálogo para el `diagnosis` dado. Solo conserva la ruta y las claves a guardar:
    /// nunca hay credenciales en él.
    pub fn new(diagnosis: &Diagnosis) -> Self {
        let buttons = vec![
            PromptButton {
                id: SAVE,
                label: "Guardar y conectar",
                variant: Variant::Primary,
            },
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
        ];
        Self {
            prompt: Prompt::new(buttons, SAVE, Size::Cells(80), theme::BORDER),
            proxy: diagnosis.proxy.clone(),
            source: diagnosis.source.clone(),
            kind: diagnosis.kind,
            changes: diagnosis.changes.clone(),
        }
    }

    /// Líneas `clave=valor` que se guardarían (`#network-setup-keys`).
    pub fn keys(&self) -> String {
        self.changes
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Dialog for NetworkSetupDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let tr = ui.tr;
        let route = if self.proxy.is_empty() {
            tr.text("Conexión directa, sin proxy").to_owned()
        } else {
            tr.tr(&Message::new("Proxy: {p0} ({p1})")
                .arg("p0", &self.proxy)
                .arg("p1", source_label(&self.source, tr)))
        };
        let state = if self.kind == DiagnosisKind::Ok {
            tr.text("El proxy da acceso a Internet.")
        } else {
            tr.text("La red redirige a un portal de autenticación.")
        };
        let keys_style = Style::new().fg(theme::TITLE).bg(theme::SIDEBAR);
        let mut content = Content::new();
        content
            .text(tr.text("CONFIGURACIÓN DE RED"), theme::title())
            .text(route, theme::panel())
            .text(state, theme::panel())
            .text(
                tr.text("Se guardarán estas claves en config.properties:"),
                theme::panel(),
            )
            .gap(1)
            .text(self.keys(), keys_style)
            .gap(1);
        if self.kind == DiagnosisKind::Portal {
            content.text(
                tr.text("Tu usuario y contraseña nunca se guardan: MITO los pedirá en cada sesión que el portal lo requiera."),
                theme::panel(),
            );
        }
        self.prompt
            .render(frame, screen, ui, &mut content, |_, _, _| {});
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        match self.prompt.handle(input) {
            Some(event) => Step::Close(Outcome::Confirmed(event.confirms(SAVE))),
            None => Step::Stay,
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
    use crossterm::event::KeyCode;

    fn portal() -> Diagnosis {
        Diagnosis {
            kind: DiagnosisKind::Portal,
            proxy: "http://proxy.example:8080".into(),
            source: "pac".into(),
            status: 0,
            authenticate: String::new(),
            portal: "http://portal.test/login".into(),
            changes: vec![
                ("https_proxy".into(), "http://proxy.example:8080".into()),
                ("enable_proxy_authentication".into(), "true".into()),
                (
                    "proxy_authentication_url".into(),
                    "http://portal.test/login".into(),
                ),
            ],
        }
    }

    fn working() -> Diagnosis {
        Diagnosis {
            kind: DiagnosisKind::Ok,
            proxy: String::new(),
            source: "direct".into(),
            changes: vec![("https_proxy".into(), String::new())],
            ..portal()
        }
    }

    #[test]
    fn aspecto_de_portal() {
        let harness = Harness::spanish();
        let mut dialog = NetworkSetupDialog::new(&portal());
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn aspecto_de_proxy_que_funciona() {
        let harness = Harness::spanish();
        let mut dialog = NetworkSetupDialog::new(&working());
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 24));
    }

    #[test]
    fn textos_del_contrato_en_orden() {
        let harness = Harness::spanish();
        let mut dialog = NetworkSetupDialog::new(&portal());
        let text = harness.draw(&mut dialog, 100, 24);
        let order = [
            "CONFIGURACIÓN DE RED",
            "Proxy: http://proxy.example:8080 (script PAC)",
            "La red redirige a un portal de autenticación.",
            "Se guardarán estas claves en config.properties:",
            "https_proxy=http://proxy.example:8080",
            "enable_proxy_authentication=true",
            "proxy_authentication_url=http://portal.test/login",
            "Tu usuario y contraseña nunca se guardan: MITO los pedirá",
            "Guardar y conectar",
            "Cancelar",
        ];
        let mut last = 0;
        for label in order {
            let at = text[last..]
                .find(label)
                .unwrap_or_else(|| panic!("{label} no aparece tras {last}\n{text}"));
            last += at + label.len();
        }
    }

    #[test]
    fn la_ruta_directa_y_el_proxy_que_funciona() {
        let harness = Harness::spanish();
        let mut dialog = NetworkSetupDialog::new(&working());
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(text.contains("Conexión directa, sin proxy"), "{text}");
        assert!(text.contains("El proxy da acceso a Internet."), "{text}");
        assert!(
            !text.contains("nunca se guardan"),
            "solo con portal\n{text}"
        );
    }

    #[test]
    fn un_origen_conocido_se_traduce_y_el_resto_no() {
        let tr = Translator::new("español");
        assert_eq!(source_label("config.properties", &tr), "config.properties");
        assert_eq!(
            source_label("manual", &tr),
            "configuración manual de Windows"
        );
        assert_eq!(source_label("pac", &tr), "script PAC");
        assert_eq!(source_label("wpad", &tr), "autodetección WPAD");
        assert_eq!(source_label("otro", &tr), "otro");
    }

    #[test]
    fn una_url_con_corchetes_se_muestra_tal_cual() {
        let harness = Harness::spanish();
        let mut diagnosis = portal();
        diagnosis.proxy = "http://[::1]:8080".into();
        let mut dialog = NetworkSetupDialog::new(&diagnosis);
        let text = harness.draw(&mut dialog, 100, 24);
        assert!(
            text.contains("Proxy: http://[::1]:8080 (script PAC)"),
            "{text}"
        );
    }

    #[test]
    fn solo_guardar_confirma() {
        let mut harness = Harness::spanish();
        let mut dialog = NetworkSetupDialog::new(&portal());
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(dialog.focus().focused(), Some(SAVE));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Confirmed(true))
        );
        let mut dialog = NetworkSetupDialog::new(&portal());
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(
            harness.click(&mut dialog, CANCEL),
            Step::Close(Outcome::Confirmed(false))
        );
        let mut dialog = NetworkSetupDialog::new(&portal());
        harness.draw(&mut dialog, 100, 24);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Confirmed(false))
        );
    }
}
