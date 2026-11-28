//! Diálogo «Abrir con» (`OpenWithDialog`): extensiones que abre cada entrada de una
//! aplicación.

use crossterm::event::KeyCode;
use mito_desktop::open_with::{COMMON, PRESETS, extensions};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;

use super::layout::{Content, columns};
use super::prompt::{Prompt, PromptButton};
use super::{Context, Dialog, Outcome, Step, drive_select, navigate, open_select};
use crate::input::Input;
use crate::model::describe;
use crate::theme;
use crate::widgets::modal::Size;
use crate::widgets::{
    Button, Column, Focus, Select, Table, TableRow, TextInput, Ui, Variant, WidgetEvent,
};

const CANCEL: &str = "open-with-cancel";
const CONFIRM: &str = "open-with-confirm";
const ENTRY: &str = "open-with-entry";
const TABLE: &str = "open-with-table";
const PRESET: &str = "open-with-preset";
const COMMON_ID: &str = "open-with-common";
const FIELD: &str = "open-with-extensions";
const ADD: &str = "open-with-add";
const REMOVE: &str = "open-with-remove";
/// Filas de la tabla de extensiones.
const TABLE_ROWS: u16 = 8;

/// «Abrir con»: extensiones registradas de cada entrada de aplicación, con presets y
/// sugerencias. Se cierra con [`Outcome::OpenWith`] (todas las entradas; lista vacía =
/// retirar) o, al cancelar o con Esc, con [`Outcome::Closed`].
#[derive(Debug)]
pub struct OpenWithDialog {
    prompt: Prompt,
    name: String,
    /// Etiqueta de cada entrada y sus extensiones.
    entries: Vec<(String, Vec<String>)>,
    current: usize,
    entry: Option<Select<String>>,
    table: Table,
    preset: Select<String>,
    common: Select<String>,
    field: TextInput,
    error: String,
}

impl OpenWithDialog {
    /// Diálogo para `name` con las `entries` (etiquetas de las entradas de aplicación) y
    /// lo ya registrado en `current` (etiqueta y extensiones).
    pub fn new(
        name: impl Into<String>,
        entries: Vec<String>,
        current: &[(String, Vec<String>)],
    ) -> Self {
        let entries: Vec<(String, Vec<String>)> = entries
            .into_iter()
            .map(|label| {
                let values = current
                    .iter()
                    .find(|(known, _)| *known == label)
                    .map(|(_, values)| values.clone())
                    .unwrap_or_default();
                (label, values)
            })
            .collect();
        // Entrada inicial: la primera con extensiones o, si no hay, la primera.
        let initial = entries
            .iter()
            .position(|(_, values)| !values.is_empty())
            .unwrap_or(0);
        let entry = (entries.len() > 1).then(|| {
            let options = entries
                .iter()
                .map(|(label, _)| (label.clone(), label.clone()))
                .collect();
            Select::new(ENTRY, options, Some(initial))
        });
        let presets = PRESETS
            .iter()
            .map(|(label, values)| (format!("{label} ({})", values.len()), values.join(" ")))
            .collect();
        let common = COMMON
            .iter()
            .map(|value| ((*value).to_owned(), (*value).to_owned()))
            .collect();
        let buttons = vec![
            PromptButton {
                id: CANCEL,
                label: "Cancelar",
                variant: Variant::Default,
            },
            PromptButton {
                id: CONFIRM,
                label: "Guardar",
                variant: Variant::Primary,
            },
        ];
        let mut table = Table::new(TABLE, vec![Column::new("", Constraint::Fill(1))]);
        table.header = false;
        let mut dialog = Self {
            prompt: Prompt::new(buttons, FIELD, Size::Cells(76), theme::BORDER),
            name: name.into(),
            entries,
            current: initial,
            entry,
            table,
            preset: Select::new(PRESET, presets, None),
            common: Select::new(COMMON_ID, common, None),
            field: TextInput::new(FIELD),
            error: String::new(),
        };
        dialog.refresh_table(0);
        dialog
    }

    fn values(&self) -> &[String] {
        self.entries
            .get(self.current)
            .map_or(&[][..], |(_, values)| values.as_slice())
    }

    /// Vuelve a pintar la tabla de la entrada actual con el cursor en `cursor`.
    fn refresh_table(&mut self, cursor: usize) {
        let rows: Vec<TableRow> = self
            .values()
            .iter()
            .map(|value| TableRow::new(value.clone(), vec![value.clone()]))
            .collect();
        let last = rows.len().saturating_sub(1);
        self.table.set_rows(rows);
        self.table.cursor = cursor.min(last);
    }

    /// `add`: interpreta `text` y añade las extensiones que falten. `false` si no es
    /// válido (el error queda en pantalla).
    fn add(&mut self, text: &str, context: &Context) -> bool {
        match extensions(text) {
            Ok(found) => {
                self.error.clear();
                if let Some((_, values)) = self.entries.get_mut(self.current) {
                    for value in found {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                }
                let cursor = self.table.cursor;
                self.refresh_table(cursor);
                true
            }
            Err(error) => {
                self.error = describe(&error, context.tr);
                false
            }
        }
    }

    /// El campo (Intro o «Añadir»): añade y, si fue bien, lo vacía.
    fn add_field(&mut self, context: &Context) -> bool {
        let text = self.field.value.clone();
        let added = self.add(&text, context);
        if added {
            self.field.set_value("");
        }
        added
    }

    fn remove(&mut self) {
        let cursor = self.table.cursor;
        if let Some((_, values)) = self.entries.get_mut(self.current)
            && cursor < values.len()
        {
            values.remove(cursor);
            self.refresh_table(cursor);
        }
    }

    fn save(&mut self, context: &Context) -> Step {
        if !self.field.value.trim().is_empty() && !self.add_field(context) {
            return Step::Stay;
        }
        Step::Close(Outcome::OpenWith(self.entries.clone()))
    }

    /// Un selector cambió de valor: los de presets y habituales añaden y se vacían; el de
    /// entrada cambia de tabla.
    fn selected(&mut self, id: &str, context: &Context) {
        match id {
            PRESET | COMMON_ID => {
                let select = if id == PRESET {
                    &mut self.preset
                } else {
                    &mut self.common
                };
                let value = select.value().cloned();
                select.selected = None;
                if let Some(value) = value {
                    self.add(&value, context);
                }
            }
            ENTRY => {
                let label = self
                    .entry
                    .as_ref()
                    .and_then(|select| select.value().cloned());
                if let Some(index) = label
                    .and_then(|label| self.entries.iter().position(|(known, _)| *known == label))
                {
                    self.current = index;
                    self.error.clear();
                    self.refresh_table(0);
                }
            }
            _ => {}
        }
    }

    fn on_button(&mut self, id: &str, context: &Context) -> Option<Step> {
        match id {
            CANCEL => Some(Step::Close(Outcome::Closed)),
            CONFIRM => Some(self.save(context)),
            ADD => {
                self.add_field(context);
                Some(Step::Stay)
            }
            REMOVE => {
                self.remove();
                Some(Step::Stay)
            }
            _ => None,
        }
    }
}

impl Dialog for OpenWithDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let mut content = Content::new();
        content
            .text(ui.tr.text("Abrir con"), theme::title())
            .gap(1)
            .text(&self.name, theme::panel())
            .gap(1);
        if self.entry.is_some() {
            content.slot("entry", 1).gap(1);
        }
        content
            .text(ui.tr.text("Extensiones registradas"), theme::panel())
            .slot("table", TABLE_ROWS)
            .gap(1)
            .slot("selects", 1)
            .gap(1)
            .slot("field", 1)
            .gap(1)
            .slot("actions", 1)
            .gap(1)
            .text(
                ui.tr.text(
                    "Aparecerá en «Abrir con» del menú contextual de esos archivos y se abrirá con el entorno del profile. No cambia la aplicación predeterminada.",
                ),
                theme::panel(),
            );
        if !self.error.is_empty() {
            content.text(
                &self.error,
                Style::new().fg(theme::BADGE_ERROR).bg(theme::SIDEBAR),
            );
        }
        self.preset.prompt = ui.tr.text("Presets").to_owned();
        self.common.prompt = ui.tr.text("Habituales").to_owned();
        self.field.placeholder = ui.tr.text("Extensiones (por ejemplo: pdf epub)").to_owned();
        let (entry, table, preset, common, field) = (
            &self.entry,
            &self.table,
            &self.preset,
            &self.common,
            &self.field,
        );
        self.prompt
            .render(frame, screen, ui, &mut content, |frame, slots, ui| {
                if let Some(select) = entry {
                    select.render(frame, slots.get("entry"), ui);
                }
                table.render(frame, slots.get("table"), ui);
                let cells = columns(slots.get("selects"), 2, 1);
                preset.render(frame, cells[0], ui);
                common.render(frame, cells[1], ui);
                field.render(frame, slots.get("field"), ui);
                let cells = columns(slots.get("actions"), 2, 1);
                Button::new(ADD, ui.tr.text("Añadir")).render(frame, cells[0], ui);
                Button::new(REMOVE, ui.tr.text("Quitar")).render(frame, cells[1], ui);
            });
        if let Some(select) = &self.entry {
            select.render_overlay(frame, ui);
        }
        self.preset.render_overlay(frame, ui);
        self.common.render_overlay(frame, ui);
    }

    fn handle(&mut self, input: &Input, context: &mut Context) -> Step {
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        selects.push(&mut self.preset);
        selects.push(&mut self.common);
        if let Some(event) = open_select(&mut selects, &self.prompt.focus, input) {
            if let Some(WidgetEvent::Changed(id)) = event {
                self.selected(&id, context);
            }
            return Step::Stay;
        }
        if input.is(KeyCode::Esc) {
            return Step::Close(Outcome::Closed);
        }
        let focused = self.prompt.focus.focused().map(str::to_owned);
        let on_button = matches!(focused.as_deref(), Some(CANCEL | CONFIRM | ADD | REMOVE));
        if navigate(&mut self.prompt.focus, input, on_button) {
            return Step::Stay;
        }
        if let Input::Click { column, row } = input {
            let Some(id) = self.prompt.focus.hit(*column, *row) else {
                return Step::Stay;
            };
            self.prompt.focus.set(&id);
            let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
            selects.push(&mut self.preset);
            selects.push(&mut self.common);
            if let Some(event) = drive_select(&mut selects, &id, input) {
                if let Some(WidgetEvent::Changed(changed)) = event {
                    self.selected(&changed, context);
                }
                return Step::Stay;
            }
            match id.as_str() {
                FIELD => self.field.click(*column),
                TABLE => {
                    self.table.click(*column, *row);
                }
                other => return self.on_button(other, context).unwrap_or(Step::Stay),
            }
            return Step::Stay;
        }
        let Some(id) = focused else {
            return Step::Stay;
        };
        let mut selects: Vec<&mut Select<String>> = self.entry.iter_mut().collect();
        selects.push(&mut self.preset);
        selects.push(&mut self.common);
        if let Some(event) = drive_select(&mut selects, &id, input) {
            if let Some(WidgetEvent::Changed(changed)) = event {
                self.selected(&changed, context);
            }
            return Step::Stay;
        }
        match id.as_str() {
            FIELD => {
                if let Some(WidgetEvent::Submitted(_)) = self.field.handle(input) {
                    self.add_field(context);
                }
                Step::Stay
            }
            TABLE => {
                self.table.handle(input);
                Step::Stay
            }
            other if input.is_activation() => self.on_button(other, context).unwrap_or(Step::Stay),
            _ => Step::Stay,
        }
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.prompt.focus
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;

    fn one() -> OpenWithDialog {
        OpenWithDialog::new(
            "main/foliate",
            vec!["Foliate".into()],
            &[("Foliate".into(), vec![".docx".into()])],
        )
    }

    fn two() -> OpenWithDialog {
        OpenWithDialog::new(
            "main/office",
            vec!["Writer".into(), "Calc".into()],
            &[("Calc".into(), vec![".xlsx".into()])],
        )
    }

    fn saved(step: Step) -> Vec<(String, Vec<String>)> {
        match step {
            Step::Close(Outcome::OpenWith(entries)) => entries,
            other => panic!("debía guardar: {other:?}"),
        }
    }

    #[test]
    fn aspecto_con_una_entrada() {
        let harness = Harness::spanish();
        let mut dialog = one();
        insta::assert_snapshot!(harness.draw(&mut dialog, 90, 36));
    }

    #[test]
    fn aspecto_con_varias_entradas_y_error() {
        let mut harness = Harness::spanish();
        let mut dialog = two();
        harness.draw(&mut dialog, 90, 40);
        harness.type_text(&mut dialog, "a/b");
        harness.press(&mut dialog, KeyCode::Enter);
        insta::assert_snapshot!(harness.draw(&mut dialog, 90, 40));
    }

    #[test]
    fn la_entrada_inicial_es_la_primera_con_extensiones() {
        let harness = Harness::spanish();
        let mut dialog = two();
        let text = harness.draw(&mut dialog, 90, 40);
        assert!(text.contains(".xlsx"), "{text}");
        assert_eq!(dialog.current, 1);
        assert_eq!(dialog.focus().focused(), Some(FIELD));
    }

    #[test]
    fn lista_anade_desde_habituales_y_texto_y_quita_la_fila() {
        let mut harness = Harness::spanish();
        let mut dialog = one();
        harness.draw(&mut dialog, 90, 36);
        // «Habituales»: elige `.pdf` (la primera opción).
        harness.focus_on(&mut dialog, COMMON_ID);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(dialog.values(), [".docx", ".pdf"]);
        assert_eq!(dialog.common.selected, None, "el selector se vacía");
        harness.draw(&mut dialog, 90, 36);
        harness.focus_on(&mut dialog, FIELD);
        harness.type_text(&mut dialog, "mp3, AVI");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(dialog.values(), [".docx", ".pdf", ".mp3", ".avi"]);
        assert!(dialog.field.value.is_empty());
        // Quitar la primera fila.
        harness.focus_on(&mut dialog, REMOVE);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.draw(&mut dialog, 90, 36);
        assert_eq!(
            saved(harness.click(&mut dialog, CONFIRM)),
            [(
                "Foliate".to_owned(),
                vec![".pdf".into(), ".mp3".into(), ".avi".into()]
            )]
        );
    }

    #[test]
    fn un_preset_anade_sus_extensiones_sin_duplicados() {
        let mut harness = Harness::spanish();
        let mut dialog = OpenWithDialog::new("main/office", vec!["Office".into()], &[]);
        harness.draw(&mut dialog, 90, 36);
        let word = PRESETS
            .iter()
            .position(|(name, _)| *name == "Word")
            .unwrap();
        harness.focus_on(&mut dialog, PRESET);
        harness.press(&mut dialog, KeyCode::Enter);
        for _ in 0..word {
            harness.press(&mut dialog, KeyCode::Down);
        }
        harness.press(&mut dialog, KeyCode::Enter);
        let expected: Vec<String> = PRESETS[word].1.iter().map(|e| (*e).to_owned()).collect();
        assert_eq!(dialog.values(), expected.as_slice());
        // Elegirlo otra vez no duplica.
        harness.draw(&mut dialog, 90, 36);
        harness.press(&mut dialog, KeyCode::Enter);
        for _ in 0..word {
            harness.press(&mut dialog, KeyCode::Down);
        }
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(dialog.values(), expected.as_slice());
    }

    #[test]
    fn una_extension_no_valida_muestra_el_error_y_no_guarda() {
        let mut harness = Harness::spanish();
        let mut dialog = one();
        harness.draw(&mut dialog, 90, 36);
        harness.type_text(&mut dialog, "a/b");
        assert_eq!(harness.click(&mut dialog, CONFIRM), Step::Stay);
        let text = harness.draw(&mut dialog, 90, 36);
        assert!(text.contains("Extensión no válida: a/b"), "{text}");
        assert_eq!(dialog.values(), [".docx"]);
    }

    #[test]
    fn guardar_con_texto_pendiente_lo_anade_antes() {
        let mut harness = Harness::spanish();
        let mut dialog = one();
        harness.draw(&mut dialog, 90, 36);
        harness.type_text(&mut dialog, "epub");
        assert_eq!(
            saved(harness.click(&mut dialog, CONFIRM)),
            [("Foliate".to_owned(), vec![".docx".into(), ".epub".into()])]
        );
    }

    #[test]
    fn cambiar_de_entrada_cambia_la_tabla_y_guardar_devuelve_todas() {
        let mut harness = Harness::spanish();
        let mut dialog = two();
        harness.draw(&mut dialog, 90, 40);
        harness.focus_on(&mut dialog, ENTRY);
        harness.press(&mut dialog, KeyCode::Enter);
        harness.press(&mut dialog, KeyCode::Up);
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(dialog.current, 0);
        assert!(dialog.values().is_empty());
        harness.draw(&mut dialog, 90, 40);
        harness.focus_on(&mut dialog, FIELD);
        harness.type_text(&mut dialog, "txt");
        harness.press(&mut dialog, KeyCode::Enter);
        assert_eq!(
            saved(harness.click(&mut dialog, CONFIRM)),
            [
                ("Writer".to_owned(), vec![".txt".into()]),
                ("Calc".to_owned(), vec![".xlsx".into()])
            ]
        );
    }

    #[test]
    fn cancelar_y_esc_cierran_sin_guardar() {
        let mut harness = Harness::spanish();
        let mut dialog = one();
        harness.draw(&mut dialog, 90, 36);
        assert_eq!(
            harness.click(&mut dialog, CANCEL),
            Step::Close(Outcome::Closed)
        );
        assert_eq!(
            harness.press(&mut dialog, KeyCode::Esc),
            Step::Close(Outcome::Closed)
        );
    }
}
