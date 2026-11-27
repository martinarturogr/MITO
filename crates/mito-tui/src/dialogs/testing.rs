//! Utilidades de prueba de los diálogos: instancia temporal, dibujo y entradas.

use crossterm::event::KeyCode;
use mito_engine::Engine;
use mito_i18n::Translator;
use ratatui::buffer::Buffer;

use super::{Context, Dialog, Step};
use crate::input::Input;
use crate::widgets::{Notice, testing};

/// Instancia temporal con su motor, un traductor y los avisos recibidos.
pub struct Harness {
    pub dir: tempfile::TempDir,
    pub engine: Engine,
    pub tr: Translator,
    pub notices: Vec<Notice>,
}

impl Harness {
    /// Instancia vacía con la interfaz en inglés.
    pub fn new() -> Self {
        Self::with_language("english")
    }

    /// Instancia vacía con la interfaz en español (las claves del original).
    pub fn spanish() -> Self {
        Self::with_language("español")
    }

    fn with_language(language: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::new(dir.path());
        Self {
            dir,
            engine,
            tr: Translator::new(language),
            notices: Vec::new(),
        }
    }

    /// Contexto para `handle` y `tick`.
    pub fn context(&mut self) -> Context<'_> {
        Context {
            tr: &self.tr,
            engine: &self.engine,
            notices: &mut self.notices,
        }
    }

    /// Dibuja el diálogo en una pantalla de `width` x `height` y devuelve su búfer.
    pub fn buffer(&self, dialog: &mut dyn Dialog, width: u16, height: u16) -> Buffer {
        testing::render_buffer(width, height, |frame, area| {
            let mut ui = dialog.focus().frame(&self.tr);
            dialog.render(frame, area, &mut ui);
        })
    }

    /// Dibuja el diálogo y devuelve el texto de la pantalla.
    pub fn draw(&self, dialog: &mut dyn Dialog, width: u16, height: u16) -> String {
        testing::buffer_text(&self.buffer(dialog, width, height))
    }

    /// Envía una entrada al diálogo.
    pub fn send(&mut self, dialog: &mut dyn Dialog, input: &Input) -> Step {
        let mut context = self.context();
        dialog.handle(input, &mut context)
    }

    /// Envía una tecla sin modificadores.
    pub fn press(&mut self, dialog: &mut dyn Dialog, code: KeyCode) -> Step {
        self.send(dialog, &Input::key(code))
    }

    /// Escribe `text` carácter a carácter y devuelve el último paso.
    pub fn type_text(&mut self, dialog: &mut dyn Dialog, text: &str) -> Step {
        let mut step = Step::Stay;
        for c in text.chars() {
            step = self.send(dialog, &Input::character(c));
        }
        step
    }

    /// Envía un tic.
    pub fn tick(&mut self, dialog: &mut dyn Dialog) -> Step {
        let mut context = self.context();
        dialog.tick(&mut context)
    }

    /// Pulsa Tab hasta que `id` tenga el foco (como mucho una vuelta).
    pub fn focus_on(&mut self, dialog: &mut dyn Dialog, id: &str) {
        for _ in 0..512 {
            if dialog.focus().focused() == Some(id) {
                return;
            }
            self.press(dialog, KeyCode::Tab);
        }
        panic!("el foco no llega a {id}: {:?}", dialog.focus().order());
    }

    /// Clic en el centro del widget `id` según el último fotograma.
    pub fn click(&mut self, dialog: &mut dyn Dialog, id: &str) -> Step {
        let area = dialog
            .focus()
            .area_of(id)
            .unwrap_or_else(|| panic!("{id} no está dibujado"));
        self.send(
            dialog,
            &Input::Click {
                column: area.x + area.width / 2,
                row: area.y + area.height / 2,
            },
        )
    }
}
