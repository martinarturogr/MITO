//! Núcleo de MITO: rutas confinadas a la instancia, JSON atómico, exclusión de
//! mutaciones, `config.properties` y log local rotado.
//!
//! Este crate define los tipos que comparten todos los demás: [`Error`] (el
//! `YitoError` del original), [`Message`] (texto traducible), [`Operation`] (progreso
//! y cancelación) y [`Layout`] (carpetas de la instancia).
#![forbid(unsafe_code)]

pub mod error;
pub mod layout;
pub mod lock;
pub mod message;
pub mod operation;
pub mod storage;

pub use error::{Error, Result};
pub use layout::Layout;
pub use lock::MutationLock;
pub use message::Message;
pub use operation::Operation;
