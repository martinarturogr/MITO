//! Catálogo de manifiestos Scoop: snapshots de buckets, resolución, búsqueda y clasificación.
//!
//! Contratos: `docs/spec/modules/catalog.md`, `search.md` y `buckets.md`. Este crate no lee
//! `config.properties` ni hace red por sí mismo (ADR 0008, punto 10): recibe los orígenes
//! de los buckets ya resueltos y una implementación de `Fetcher` para las descargas.
#![forbid(unsafe_code)]

pub mod archive;
pub mod buckets;
pub mod checks;
pub mod compat;
pub mod error;
pub mod manifest;
pub mod pyjson;
mod pypath;
pub mod resources;
pub mod search;

pub use buckets::{BucketSources, Catalog, Fetcher, read_json_value};
pub use checks::{
    daily, daily_on, read_disabled_buckets, set_bucket_enabled, write_disabled_buckets,
};
pub use compat::{COMPATIBLE, Compatibility, compatibility};
pub use error::{Error, FetchError, Result};
pub use manifest::{Manifest, ManifestView, OneOrMany, array, fingerprint, installer_definition};
pub use resources::required_buckets;
pub use search::{Entry, rank_entries, tokens};
