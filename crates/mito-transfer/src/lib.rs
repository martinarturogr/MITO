//! Descargas HTTP/HTTPS, verificación de hashes y extracción validada de contenedores.
//!
//! Contratos: `docs/spec/modules/transfer.md`, `archives.md` y `network.md`.
#![forbid(unsafe_code)]

pub mod archives;
pub mod download;
pub mod error;
pub mod hash;
pub mod naming;
pub mod network;
mod stream;
pub mod web;

pub use archives::{ExtractOptions, Extraction, MAX_EXPANDED, extract, extract_zip};
pub use download::{DownloadOptions, MAX_DOWNLOAD, download};
pub use error::{Error, Result};
pub use hash::{ExpectedHash, HashAlgorithm, Hasher, download_hashes, parse_hash};
pub use naming::download_filename;
pub use web::{public_url, web_page, web_page_error};
