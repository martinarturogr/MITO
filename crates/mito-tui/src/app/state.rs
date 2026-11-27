//! Estado de la aplicación: páginas, ocupado, temporizadores, avisos y log.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mito_core::Message;
use mito_core::Operation;
use mito_core::diagnostics::{self, LogLevel};
use mito_core::error_tracking::{self, Options};
use mito_engine::Engine;
use mito_i18n::Translator;

use super::catalog::CatalogState;
use super::dialogs::Open;
use super::home::HomeState;
use crate::widgets::{Focus, LogView, Notice, Select, Severity};
use crate::worker::Report;

/// Espera antes de mostrar el indicador de ocupado, el diálogo de progreso y el aviso
/// de paso largo.
pub(super) const DELAY: Duration = Duration::from_secs(2);

/// Función que copia texto al portapapeles (inyectable en las pruebas).
pub(super) type Clipboard = Box<dyn Fn(&str) -> io::Result<()> + Send>;

/// Función que abre una carpeta en el Explorador (inyectable en las pruebas).
pub(super) type Opener = Box<dyn Fn(&Path) -> mito_core::Result<()> + Send>;

/// Página activa (`ContentSwitcher#pages`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    /// «MIS COMPONENTES».
    Home,
    /// «BUSCAR E INSTALAR · CATÁLOGOS SCOOP».
    Catalog,
}

/// Qué ocurre cuando vence un temporizador.
#[derive(Debug)]
pub(super) enum TimerKind {
    /// Mostrar el indicador de ocupado si la secuencia sigue siendo la misma.
    Busy(u64),
    /// Abrir el diálogo de progreso de la operación con esa secuencia.
    Progress(u64),
    /// Abrir el aviso de paso largo con esa secuencia y ese mensaje.
    LongStep(u64, Message),
}

/// Temporizador pendiente (el `set_timer` de Textual).
#[derive(Debug)]
pub(super) struct Timer {
    /// Instante en que vence.
    pub due: Instant,
    /// Acción al vencer.
    pub kind: TimerKind,
}

/// Estado de ocupado y de la operación en curso.
#[derive(Debug, Default)]
pub(super) struct Busy {
    /// Hay una operación en curso (`busy`).
    pub active: bool,
    /// Secuencia del indicador: una operación antigua no puede mostrar el de una nueva.
    pub sequence: u64,
    /// El indicador `⏳ …` está visible.
    pub indicator: bool,
    /// Operación en curso (desde su primera fase activa).
    pub operation: Option<Operation>,
    /// Secuencia de la operación, para su diálogo de progreso.
    pub operation_sequence: u64,
    /// Secuencia del aviso de paso largo.
    pub long_sequence: u64,
    /// Al terminar hay que recargar el catálogo (`entries = []`).
    pub reload_catalog: bool,
}

/// La interfaz de texto de MITO (`YitoApp`).
pub struct App {
    pub(super) engine: Arc<Engine>,
    pub(super) tr: Translator,
    pub(super) page: Page,
    pub(super) focus: Focus,
    /// Foco que se aplica al terminar el fotograma (el widget aún no se había dibujado).
    pub(super) pending_focus: Option<String>,
    pub(super) profile: Select<String>,
    pub(super) home: HomeState,
    pub(super) catalog: CatalogState,
    pub(super) log: LogView,
    pub(super) notices: Vec<Notice>,
    pub(super) dialogs: Vec<Open>,
    pub(super) busy: Busy,
    pub(super) timers: Vec<Timer>,
    pub(super) now: Instant,
    pub(super) reports_tx: Sender<Report>,
    pub(super) reports_rx: Receiver<Report>,
    pub(super) worker: Option<JoinHandle<()>>,
    /// Comprobación de actualizaciones del arranque: corre aparte, sin bloquear la
    /// interfaz (worker `component-updates` del original).
    pub(super) startup: Option<(JoinHandle<()>, Receiver<Report>)>,
    /// Acción pedida mientras corre la comprobación del arranque; empieza al terminar
    /// esta (`wait_startup_updates` del original).
    pub(super) queued: Option<crate::worker::Action>,
    pub(super) clipboard: Clipboard,
    pub(super) opener: Opener,
    pub(super) identity: bool,
    pub(super) initial_profile: Option<String>,
    pub(super) quit: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("page", &self.page)
            .field("busy", &self.busy.active)
            .field("dialogs", &self.dialogs.len())
            .finish_non_exhaustive()
    }
}

impl App {
    /// Interfaz sobre `engine` con el traductor `tr` y el profile inicial pedido (si
    /// existe; si no, el primero). Usa el portapapeles de Windows y aplica la identidad
    /// de la barra de tareas al montar.
    pub fn new(engine: Arc<Engine>, profile: Option<String>, tr: Translator) -> Self {
        let (reports_tx, reports_rx) = channel();
        let catalog = CatalogState::new(&tr);
        Self {
            focus: Focus::new(),
            pending_focus: None,
            profile: Select::new("profile", Vec::new(), None),
            home: HomeState::default(),
            catalog,
            log: LogView::new("log"),
            notices: Vec::new(),
            dialogs: Vec::new(),
            busy: Busy::default(),
            timers: Vec::new(),
            now: Instant::now(),
            reports_tx,
            reports_rx,
            worker: None,
            startup: None,
            queued: None,
            clipboard: Box::new(mito_win::clipboard::set_text),
            opener: Box::new(mito_core::persistence::open_location),
            identity: true,
            initial_profile: profile,
            quit: false,
            page: Page::Home,
            engine,
            tr,
        }
    }

    /// Sustituye la función que copia al portapapeles (para pruebas).
    #[must_use]
    pub fn with_clipboard(
        mut self,
        clipboard: impl Fn(&str) -> io::Result<()> + Send + 'static,
    ) -> Self {
        self.clipboard = Box::new(clipboard);
        self
    }

    /// Sustituye la función que abre una carpeta en el Explorador (para pruebas).
    #[must_use]
    pub fn with_opener(
        mut self,
        opener: impl Fn(&Path) -> mito_core::Result<()> + Send + 'static,
    ) -> Self {
        self.opener = Box::new(opener);
        self
    }

    /// No aplica la identidad de la barra de tareas al montar (para pruebas).
    #[must_use]
    pub fn without_identity(mut self) -> Self {
        self.identity = false;
        self
    }

    /// ¿Se pidió salir?
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// ¿Hay una operación en curso?
    pub fn is_busy(&self) -> bool {
        self.busy.active
    }

    /// Profile seleccionado (`current_profile`).
    pub(super) fn current_profile(&self) -> Option<String> {
        self.profile.value().cloned()
    }

    /// Muestra un aviso (`notify`).
    pub(super) fn notify(&mut self, text: impl Into<String>, severity: Severity) {
        self.notices.push(Notice::new(text, severity));
    }

    /// Texto traducido de una cadena fija.
    pub(super) fn text(&self, key: &str) -> String {
        self.tr.text(key).to_owned()
    }

    /// `write_log` de un mensaje de la interfaz (no del hilo): lo escribe en
    /// `logs/mito.log` y en el panel de log.
    pub(super) fn write_log(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        let level = if error {
            LogLevel::Error
        } else {
            LogLevel::Info
        };
        diagnostics::write(self.engine.layout(), level, "ui", &text, &[]);
        self.log.push(text, error);
    }

    /// Registra un error en el log local (`capture` del original).
    pub(super) fn capture<E: std::error::Error + 'static>(&self, error: &E, operation: &str) {
        let _ =
            error_tracking::capture(self.engine.layout(), error, &Options::operation(operation));
    }

    /// Reconstruye las opciones del selector de profiles y elige `wanted` si existe; si
    /// no, el primero. No dispara ningún cambio de selección.
    pub(super) fn rebuild_profiles(&mut self, wanted: Option<&str>) {
        let names = self.engine.profiles();
        let selected = wanted
            .and_then(|wanted| names.iter().position(|name| name == wanted))
            .or_else(|| (!names.is_empty()).then_some(0));
        let options = names.iter().map(|n| (n.clone(), n.clone())).collect();
        self.profile = Select::new("profile", options, selected)
            .with_prompt(self.tr.text("Selecciona un profile"));
    }

    /// Aplica los ajustes de Windows Terminal del profile actual
    /// (`refresh_terminal_default`).
    pub(super) fn refresh_terminal_default(&mut self) {
        let profile = self.current_profile();
        if let Err(error) = mito_terminal::prepare(&self.engine, profile.as_deref()) {
            self.capture(&error, "terminal.settings");
            self.notify(error.to_string(), Severity::Error);
        }
    }
}
