//! Diálogos modales de la interfaz.
//!
//! Cada diálogo es un `struct` con su estado y su propio [`Focus`]. La aplicación lo
//! dibuja con [`Dialog::render`], le pasa las entradas con [`Dialog::handle`] y los
//! tics de 250 ms con [`Dialog::tick`]; cuando devuelve [`Step::Close`] lo retira y usa
//! el [`Outcome`] como respuesta (la continuación que en Textual era el `await`).

mod component_info;
mod configuration;
mod delete_profile;
mod host_confirm;
mod host_discover;
mod host_info;
mod install_version;
mod integrity;
mod layout;
mod long_step;
mod manage_profile;
mod manifest;
mod open_application;
mod open_with;
mod operation;
mod prompt;
mod protocols;
mod remove_component;
mod remove_host;
mod shortcut;
mod unverified;

#[cfg(test)]
pub(crate) mod testing;

use crossterm::event::KeyCode;
use mito_engine::Engine;
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::input::Input;
use crate::widgets::{Focus, Notice, Select, Ui, WidgetEvent, WidgetId};
use crate::worker::Pick;

pub use component_info::ComponentInfoDialog;
pub use configuration::ConfigurationDialog;
pub use delete_profile::DeleteProfileDialog;
pub use host_confirm::HostDataConfirm;
pub use host_discover::HostDiscoverDialog;
pub use host_info::HostInfoDialog;
pub use install_version::InstallVersionDialog;
pub use integrity::DownloadIntegrityDialog;
pub use long_step::LongStepDialog;
pub use manage_profile::ManageProfileDialog;
pub use manifest::ManifestDialog;
pub use open_application::OpenApplicationDialog;
pub use open_with::OpenWithDialog;
pub use operation::{OperationDialog, human_size};
pub use protocols::{ProtocolBackend, ProtocolDialog, ThreadedSession};
pub use remove_component::RemoveComponentDialog;
pub use remove_host::RemoveHostDialog;
pub use shortcut::{ShortcutChoice, ShortcutDialog};
pub use unverified::UnverifiedDownloadsDialog;

/// Qué hacer tras procesar una entrada o un tic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// El diálogo sigue abierto.
    Stay,
    /// El diálogo se cierra con este resultado.
    Close(Outcome),
}

/// Resultado con el que se cierra un diálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Manifiesto, integridad, paso largo, progreso, o un diálogo de aplicación
    /// cancelado.
    Closed,
    /// Nueva versión, eliminar, descargas sin hash, borrar profile: `true` si se
    /// confirmó.
    Confirmed(bool),
    /// Info: «Liberar» la versión retenida.
    Release,
    /// Abrir aplicación: argumentos ya partidos y entrada elegida (si había).
    Open {
        /// Argumentos de la línea de órdenes.
        arguments: Vec<String>,
        /// Entrada de aplicación elegida, si el componente tiene varias.
        entry: Option<String>,
        /// Proceso `attached`: acoplar a una pestaña de la terminal de MITO.
        coupled: bool,
    },
    /// «Generar acceso directo»: letras, color y, para aplicaciones, modo y argumentos.
    Shortcut(ShortcutChoice),
    /// «Agregar aplicación del host»: el archivo elegido, «Examinar…» o cancelado.
    HostApplication(Pick),
    /// «Abrir con»: extensiones de cada entrada de aplicación (vacía = retirar).
    OpenWith(Vec<(String, Vec<String>)>),
    /// Configuración: `true` si se guardó.
    Saved(bool),
    /// Gestionar profile.
    Profile(ProfileChoice),
}

/// Acción elegida en «Gestionar profile».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileChoice {
    /// Se cerró el diálogo sin elegir.
    Cancel,
    /// Crear un profile con este nombre (ya sin espacios sobrantes).
    Create(String),
    /// Abrir la configuración del profile.
    Configuration,
    /// Eliminar el profile.
    Delete,
}

/// Lo que un diálogo necesita del exterior: traducción, avisos y la instancia.
pub struct Context<'a> {
    /// Traductor de la interfaz.
    pub tr: &'a Translator,
    /// Motor de la instancia.
    pub engine: &'a Engine,
    /// Avisos pendientes de mostrar (los `notify` del original).
    pub notices: &'a mut Vec<Notice>,
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("language", &self.tr.language())
            .field("root", &self.engine.root())
            .field("notices", &self.notices.len())
            .finish()
    }
}

/// Un diálogo modal.
pub trait Dialog {
    /// Dibuja el diálogo sobre la pantalla completa `screen`.
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui);

    /// Procesa una entrada; el diálogo usa su propio [`Focus`].
    fn handle(&mut self, input: &Input, context: &mut Context) -> Step;

    /// Tic periódico (cada 250 ms) para relojes y cierres diferidos.
    fn tick(&mut self, _context: &mut Context) -> Step {
        Step::Stay
    }

    /// Foco y zonas del diálogo.
    fn focus(&mut self) -> &mut Focus;
}

/// Tab y Mayús+Tab recorren el foco; con `arrows`, ← y → también (para filas de
/// botones). Devuelve `true` si la entrada se consumió.
pub(crate) fn navigate(focus: &mut Focus, input: &Input, arrows: bool) -> bool {
    if input.is_tab() || (arrows && input.is(KeyCode::Right)) {
        focus.next();
        true
    } else if input.is_back_tab() || (arrows && input.is(KeyCode::Left)) {
        focus.previous();
        true
    } else {
        false
    }
}

/// Id sobre el que actúa una entrada: el del clic (que además lo enfoca) o el enfocado
/// con Intro o Espacio.
pub(crate) fn pressed(focus: &mut Focus, input: &Input) -> Option<WidgetId> {
    match input {
        Input::Click { column, row } => {
            let id = focus.hit(*column, *row)?;
            focus.set(&id);
            Some(id)
        }
        _ if input.is_activation() => focus.focused().map(str::to_owned),
        _ => None,
    }
}

/// Si hay un selector con la lista abierta, le entrega la entrada. Devuelve `Some` (con el
/// evento que produjo, si lo hubo) cuando la consumió; `None` si ninguno estaba abierto o
/// la entrada lo cierra (Tab, o un clic fuera de él) y hay que seguir procesándola.
#[allow(
    clippy::option_option,
    reason = "`Some` = la entrada se consumió; el interior, el evento del selector"
)]
pub(crate) fn open_select(
    selects: &mut [&mut Select<String>],
    focus: &Focus,
    input: &Input,
) -> Option<Option<WidgetEvent>> {
    let select = selects.iter_mut().find(|select| select.open)?;
    let hit = match input {
        Input::Click { column, row } => focus.hit(*column, *row),
        _ => None,
    };
    let closing = input.is_tab()
        || input.is_back_tab()
        || (matches!(input, Input::Click { .. }) && hit.as_deref() != Some(select.id.as_str()));
    if closing {
        select.close();
        return None;
    }
    Some(if let Input::Click { column, row } = input {
        select.click(*column, *row)
    } else {
        select.handle(input)
    })
}

/// Entrega la entrada al selector `id` (clic o tecla). `None` si ninguno tiene ese id; si
/// no, `Some` con el evento que produjo (si lo hubo).
#[allow(
    clippy::option_option,
    reason = "`None` = no hay ese selector; el interior, el evento que produjo"
)]
pub(crate) fn drive_select(
    selects: &mut [&mut Select<String>],
    id: &str,
    input: &Input,
) -> Option<Option<WidgetEvent>> {
    let select = selects.iter_mut().find(|select| select.id == id)?;
    Some(if let Input::Click { column, row } = input {
        select.click(*column, *row)
    } else {
        select.handle(input)
    })
}

/// Opciones `detached`/`attached` del selector «Proceso» (etiquetas literales).
pub(crate) fn process_select(id: &str) -> Select<String> {
    Select::new(
        id,
        vec![
            ("detached".to_owned(), "detached".to_owned()),
            ("attached".to_owned(), "attached".to_owned()),
        ],
        Some(0),
    )
}

/// ¿Está elegido `attached` en el selector «Proceso»?
pub(crate) fn is_attached(select: &Select<String>) -> bool {
    select.value().is_some_and(|value| value == "attached")
}

/// Texto de un error del núcleo para el usuario: su mensaje traducido o, si no lo tiene
/// (E/S, JSON), su descripción.
pub(crate) fn describe_core(error: &mito_core::Error, tr: &Translator) -> String {
    error
        .message()
        .map_or_else(|| error.to_string(), |message| tr.tr(&message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navegar_y_pulsar_sin_widgets_no_hace_nada() {
        let mut focus = Focus::new();
        assert!(navigate(&mut focus, &Input::key(KeyCode::Tab), false));
        assert!(!navigate(&mut focus, &Input::key(KeyCode::Right), false));
        assert!(navigate(&mut focus, &Input::key(KeyCode::Right), true));
        assert_eq!(pressed(&mut focus, &Input::key(KeyCode::Enter)), None);
        assert_eq!(
            pressed(&mut focus, &Input::Click { column: 1, row: 1 }),
            None
        );
    }
}
