//! La terminal real: modo crudo, restauración y bucle de eventos.

use std::io::{self, IsTerminal, Stdout};
use std::panic;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use mito_engine::Engine;
use mito_i18n::Translator;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::state::App;
use crate::input::Input;

/// Espera máxima de un evento antes de volver a mirar los informes del hilo.
const POLL: Duration = Duration::from_millis(50);
/// Periodo de los tics (relojes, avisos, cierres diferidos).
const TICK: Duration = Duration::from_millis(250);

/// Error de la interfaz de texto.
#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    /// La entrada o la salida no son una terminal interactiva.
    #[error("la interfaz necesita una terminal interactiva")]
    NotATerminal,
    /// Fallo al preparar, dibujar o leer la terminal.
    #[error("error de la terminal: {0}")]
    Terminal(#[from] io::Error),
    /// Fallo del motor al arrancar.
    #[error(transparent)]
    Engine(#[from] mito_engine::Error),
}

/// Deja la terminal como estaba (idempotente: se puede llamar varias veces).
fn restore() {
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

/// Guarda RAII: al soltarla restaura la terminal, también si hay un error o un pánico.
struct Guard;

impl Guard {
    /// Modo crudo, pantalla alternativa, ratón y pegado, y un gancho de pánico que
    /// restaura la terminal antes de encadenar el anterior.
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        Ok(guard)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

/// Abre la interfaz de texto sobre `engine` y no vuelve hasta que el usuario sale.
///
/// `profile` es el profile que se selecciona al arrancar si existe; si no, el primero.
///
/// # Errors
///
/// [`TuiError::NotATerminal`] si no hay una terminal interactiva, y los errores de la
/// terminal o del motor al arrancar.
pub fn run(engine: Engine, profile: Option<String>) -> Result<(), TuiError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(TuiError::NotATerminal);
    }
    let engine = Arc::new(engine);
    let tr = Translator::from_layout(engine.layout()).map_err(mito_engine::Error::from)?;
    let mut app = App::new(engine, profile, tr);
    let _guard = Guard::enter()?;
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut last_tick = Instant::now();
    app.mount(last_tick);
    while !app.should_quit() {
        terminal.draw(|frame| app.render(frame))?;
        if event::poll(POLL)? {
            // Se vacía la cola antes de volver a dibujar (el movimiento del ratón
            // genera muchos eventos).
            loop {
                if let Some(input) = Input::from_event(event::read()?) {
                    app.handle(&input, Instant::now());
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        let now = Instant::now();
        app.poll_reports(now);
        if now.duration_since(last_tick) >= TICK {
            last_tick = now;
            app.tick(now);
        }
    }
    Ok(())
}
