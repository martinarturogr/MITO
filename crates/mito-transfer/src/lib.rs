//! Descargas HTTP/HTTPS, verificación de hashes y extracción validada de contenedores.
//!
//! Contratos: `docs/spec/modules/transfer.md`, `archives.md` y `network.md`.
#![forbid(unsafe_code)]

pub mod hash;
pub mod naming;

pub use hash::{ExpectedHash, HashAlgorithm, download_hashes, parse_hash};
pub use naming::download_filename;
