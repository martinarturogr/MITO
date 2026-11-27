//! Utilidades de prueba de la aplicación: instancia temporal, reloj propio y dibujo.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::state::App;
use crate::input::Input;
use crate::model::fixture::{Fx, fx, spanish};
use crate::widgets::testing::buffer_text;

/// Textos recibidos por el portapapeles falso.
pub type Copies = Arc<Mutex<Vec<String>>>;

/// La aplicación sobre una instancia temporal, con su reloj.
pub struct Rig {
    pub f: Fx,
    pub app: App,
    pub now: Instant,
    pub copies: Copies,
    pub opened: Arc<Mutex<Vec<PathBuf>>>,
    pub width: u16,
    pub height: u16,
}

impl Rig {
    /// Interfaz montada y en reposo (comprobación de actualizaciones terminada) sobre una
    /// instancia preparada antes con `setup`.
    pub fn with(setup: impl FnOnce(&Fx)) -> Self {
        Self::build(setup, None)
    }

    /// Como [`with`](Self::with), con un profile inicial pedido.
    pub fn build(setup: impl FnOnce(&Fx), profile: Option<&str>) -> Self {
        let f = fx();
        setup(&f);
        let copies: Copies = Arc::default();
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let app = {
            let copies = Arc::clone(&copies);
            let opened = Arc::clone(&opened);
            App::new(Arc::clone(&f.engine), profile.map(str::to_owned), spanish())
                .with_clipboard(move |text| {
                    copies.lock().unwrap().push(text.to_owned());
                    Ok(())
                })
                .with_opener(move |path| {
                    opened.lock().unwrap().push(path.to_path_buf());
                    Ok(())
                })
                .without_identity()
        };
        let mut rig = Self {
            f,
            app,
            now: Instant::now(),
            copies,
            opened,
            width: 120,
            height: 40,
        };
        rig.app.mount(rig.now);
        rig.settle();
        rig
    }

    /// Hace que el portapapeles falle.
    pub fn failing_clipboard(&mut self) {
        self.app.clipboard = Box::new(|_| Err(io::Error::other("sin portapapeles")));
    }

    /// Avanza el reloj y da un tic.
    pub fn advance(&mut self, by: Duration) {
        self.now += by;
        self.app.tick(self.now);
    }

    /// Procesa informes del hilo hasta que `done` se cumpla (como mucho 20 s).
    pub fn pump_until(&mut self, done: impl Fn(&App) -> bool) {
        let limit = Instant::now() + Duration::from_secs(20);
        while !done(&self.app) {
            assert!(Instant::now() < limit, "tiempo agotado esperando al hilo");
            self.app.poll_reports(self.now);
            if !done(&self.app) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Espera a que termine la operación en curso.
    pub fn settle(&mut self) {
        self.pump_until(|app| !app.busy.active && app.startup.is_none());
    }

    pub fn input(&mut self, input: &Input) {
        self.app.handle(input, self.now);
    }

    pub fn press(&mut self, code: KeyCode) {
        self.input(&Input::key(code));
    }

    pub fn ctrl(&mut self, c: char) {
        self.input(&Input::ctrl(c));
    }

    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.input(&Input::character(c));
        }
    }

    fn terminal(&self) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(self.width, self.height)).expect("terminal de prueba")
    }

    /// Dibuja la pantalla y devuelve su búfer.
    pub fn buffer(&mut self) -> Buffer {
        let mut terminal = self.terminal();
        terminal
            .draw(|frame| self.app.render(frame))
            .expect("dibujo");
        terminal.backend().buffer().clone()
    }

    /// Dibuja la pantalla y devuelve su texto.
    pub fn draw(&mut self) -> String {
        buffer_text(&self.buffer())
    }

    /// Foco de la pantalla de arriba: el del diálogo abierto o el principal.
    fn top_area(&mut self, id: &str) -> Option<Rect> {
        match self.app.dialogs.last_mut() {
            Some(open) => open.slot.dialog().focus().area_of(id),
            None => self.app.focus.area_of(id),
        }
    }

    /// ¿Está `id` registrado (visible y habilitado) en la pantalla de arriba?
    pub fn has(&mut self, id: &str) -> bool {
        self.draw();
        match self.app.dialogs.last_mut() {
            Some(open) => open.slot.dialog().focus().is_registered(id),
            None => self.app.focus.is_registered(id),
        }
    }

    /// Dibuja y hace clic en el centro de `id`.
    pub fn click(&mut self, id: &str) {
        self.draw();
        let area = self
            .top_area(id)
            .unwrap_or_else(|| panic!("{id} no está dibujado"));
        self.input(&Input::Click {
            column: area.x + area.width / 2,
            row: area.y + area.height / 2,
        });
    }

    /// Pulsa Tab hasta que `id` tenga el foco de la pantalla principal.
    pub fn focus_on(&mut self, id: &str) {
        self.draw();
        for _ in 0..200 {
            if self.app.focus.focused() == Some(id) {
                return;
            }
            self.press(KeyCode::Tab);
        }
        panic!("el foco no llega a {id}: {:?}", self.app.focus.order());
    }

    /// Líneas del panel de log.
    pub fn log_lines(&self) -> Vec<String> {
        self.app
            .log
            .entries
            .iter()
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// Textos de los avisos visibles.
    pub fn notices(&self) -> Vec<String> {
        self.app.notices.iter().map(|n| n.text.clone()).collect()
    }

    /// Número de diálogos abiertos.
    pub fn dialogs(&self) -> usize {
        self.app.dialogs.len()
    }

    /// ¿Dice el log `text` exactamente en alguna línea?
    pub fn logged(&self, text: &str) -> bool {
        self.log_lines().iter().any(|line| line == text)
    }
}
