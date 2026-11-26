//! Traducciones de MITO. Las claves son plantillas en español y los argumentos se
//! interpolan después de traducir.
//!
//! Contrato: `docs/spec/modules/i18n.md`. Los errores y avisos de `mito-core` son un
//! [`Message`](mito_core::Message) con la plantilla española y sus argumentos sin
//! traducir; el borde de la aplicación (la TUI) los traduce con un [`Translator`]. La
//! CLI y el log muestran el texto español. Los 24 catálogos están incrustados en el
//! binario; no se cargan archivos en tiempo de ejecución.
//!
//! ```
//! use mito_core::Message;
//! use mito_i18n::Translator;
//!
//! let english = Translator::new("english");
//! let message = Message::new("Profile {p1} creado").arg("p1", "dev");
//! assert_eq!(english.tr(&message), "Profile dev created");
//! assert_eq!(message.to_string(), "Profile dev creado");
//! ```
#![forbid(unsafe_code)]

mod bundle;
#[cfg(test)]
mod tests;
mod translator;

pub use bundle::{Bundle, bundle, catalog_json, fields};
pub use mito_core::config::{DEFAULT_SETTING, LANGUAGE_CHOICES, LANGUAGES};
pub use translator::{Translator, language_for_langid, system_language};

/// Idioma de reserva: el de los idiomas desconocidos y el de los fallos de detección.
pub const DEFAULT_LANGUAGE: &str = "english";
