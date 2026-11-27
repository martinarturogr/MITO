//! Manifiesto de un componente en JSON (`ManifestDialog`).

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use serde_json::Value;

use super::{Context, Dialog, Outcome, Step, navigate, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Focus, Stack, TextView, Ui, header};

const CLOSE: &str = "manifest-close";
const CONTENT: &str = "manifest-content";

/// «Manifiesto»: el JSON del manifiesto con sangría de 4 espacios, sin ajuste de línea
/// y con desplazamiento horizontal. Esc o `×` lo cierran con [`Outcome::Closed`].
#[derive(Debug)]
pub struct ManifestDialog {
    focus: Focus,
    key: String,
    view: TextView,
}

/// JSON como `json.dumps(value, ensure_ascii=False, indent=4)`: sangría de 4 espacios,
/// `": "` entre clave y valor, y `{}` / `[]` para los contenedores vacíos.
pub fn pretty_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (index, (key, item)) in map.iter().enumerate() {
                indent(out, depth + 1);
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                write_value(out, item, depth + 1);
                out.push_str(if index + 1 < map.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push(']');
        }
        // Escalares y contenedores vacíos: serde_json ya los escribe como Python.
        other => out.push_str(&other.to_string()),
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

impl ManifestDialog {
    /// Diálogo para el manifiesto de `key`.
    pub fn new(key: impl Into<String>, manifest: &Value) -> Self {
        let mut view = TextView::from_text(CONTENT, &pretty_json(manifest));
        view.wrap = false;
        Self {
            focus: Focus::starting_at(CONTENT),
            key: key.into(),
            view,
        }
    }

    /// Texto JSON que se muestra.
    pub fn json(&self) -> String {
        self.view.plain()
    }
}

impl Dialog for ManifestDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let area = modal::area(screen, Size::Percent(90), Size::Percent(90));
        let inner = modal::frame(frame, area, theme::BORDER);
        let mut stack = Stack::new(inner);
        header::render(frame, stack.take(header::HEIGHT), ui, "Manifiesto", CLOSE);
        stack.text(frame, &self.key, theme::panel());
        stack.gap(1);
        self.view.render(frame, stack.remaining(), ui);
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        if matches!(input, Input::Scroll { .. }) {
            self.view.handle(input);
            return Step::Stay;
        }
        if navigate(&mut self.focus, input, false) {
            return Step::Stay;
        }
        if matches!(input, Input::Click { .. }) || self.focus.focused() == Some(CLOSE) {
            if let Some(id) = pressed(&mut self.focus, input)
                && id == CLOSE
            {
                return Step::Close(Outcome::Closed);
            }
            return Step::Stay;
        }
        self.view.handle(input);
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

    fn manifest() -> Value {
        json!({
            "version": "1.2.3",
            "description": "Herramienta ñandú",
            "bin": ["tool.exe", ["tool.exe", "tool"]],
            "env_set": {},
            "notes": [],
            "ratio": 1.5
        })
    }

    #[test]
    fn el_json_es_como_json_dumps_con_indent_4() {
        let text = pretty_json(&manifest());
        assert_eq!(
            text,
            "{\n    \"version\": \"1.2.3\",\n    \"description\": \"Herramienta ñandú\",\n    \"bin\": [\n        \"tool.exe\",\n        [\n            \"tool.exe\",\n            \"tool\"\n        ]\n    ],\n    \"env_set\": {},\n    \"notes\": [],\n    \"ratio\": 1.5\n}"
        );
        assert_eq!(pretty_json(&json!("a\"b\n")), "\"a\\\"b\\n\"");
        assert_eq!(pretty_json(&json!(null)), "null");
        assert_eq!(pretty_json(&json!([])), "[]");
    }

    #[test]
    fn aspecto() {
        let harness = Harness::new();
        let mut dialog = ManifestDialog::new("main/tool", &manifest());
        insta::assert_snapshot!(harness.draw(&mut dialog, 100, 30));
    }

    #[test]
    fn el_foco_empieza_en_el_contenido_y_las_flechas_desplazan() {
        let mut harness = Harness::new();
        let long: Vec<String> = (0..60).map(|i| format!("elemento {i}")).collect();
        let mut dialog = ManifestDialog::new("main/tool", &json!({ "lista": long }));
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(dialog.focus().focused(), Some(CONTENT));
        harness.press(&mut dialog, KeyCode::End);
        let text = harness.draw(&mut dialog, 100, 30);
        assert!(
            text.contains("elemento 59") && !text.contains("elemento 3\""),
            "{text}"
        );
        harness.press(&mut dialog, KeyCode::Home);
        assert!(harness.draw(&mut dialog, 100, 30).contains("elemento 0"));
    }

    #[test]
    fn esc_la_cruz_y_el_boton_cierran() {
        let mut harness = Harness::new();
        let mut dialog = ManifestDialog::new("main/tool", &manifest());
        harness.draw(&mut dialog, 100, 30);
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
        assert_eq!(
            harness.click(&mut dialog, CLOSE),
            Step::Close(Outcome::Closed)
        );
        let mut dialog = ManifestDialog::new("main/tool", &manifest());
        harness.draw(&mut dialog, 100, 30);
        harness.press(&mut dialog, KeyCode::Tab);
        assert_eq!(dialog.focus().focused(), Some(CLOSE));
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Enter),
            Step::Close(Outcome::Closed)
        );
    }
}
