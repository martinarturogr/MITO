//! Estado, actualización, dibujo y bucle de eventos de la interfaz (`YitoApp`).
//!
//! La aplicación sigue la arquitectura del ADR 0011: el estado solo cambia en los
//! puntos de entrada de [`App`] ([`handle`](App::handle) para el teclado y el ratón,
//! [`on_report`](App::on_report) para los informes del hilo de trabajo y
//! [`tick`](App::tick) para el reloj) y [`render`](App::render) lo dibuja. Todos reciben
//! el instante actual, de modo que las pruebas usan un reloj propio en lugar de esperas
//! reales. La terminal y el bucle de eventos viven en [`run`].
//!
//! Reparto de archivos:
//!
//! - `state`: el estado ([`App`]), los avisos, el log y los temporizadores.
//! - `update`: entradas de teclado, ratón y pegado.
//! - `reports`: informes del hilo, inicio y fin de las operaciones, tics.
//! - `dialogs`: pila de diálogos y el uso de su resultado.
//! - `render`: cabecera, cuerpo, panel de log y pie.
//! - `sidebar`, `home` y `catalog`: barra lateral y las dos páginas.
//! - `terminal`: modo crudo, restauración y bucle de eventos.

mod catalog;
mod dialogs;
mod home;
mod host;
mod render;
mod reports;
mod sidebar;
mod state;
mod terminal;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_f8;
mod update;

pub use state::App;
pub use terminal::{TuiError, run};
