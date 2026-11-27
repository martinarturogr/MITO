//! Datos de Inicio y del catálogo, sin dependencia de ratatui.
//!
//! Funciones puras o que solo leen del motor: la interfaz las dibuja. Reproducen
//! `refresh_home`, `load_catalog`, `filter_catalog`, `on_data_table_row_highlighted` y
//! `license_label` de `ui.py` (`docs/spec/modules/ui.md`). Todo texto visible se
//! traduce con el [`mito_i18n::Translator`] recibido.

mod catalog_page;
mod errors;
#[cfg(test)]
pub(crate) mod fixture;
mod home_page;

pub use catalog_page::{Catalog, CatalogRow, Details, Listing, license_label};
pub use errors::{IntegrityInfo, describe};
pub use home_page::{
    Home, HomeRow, bulk_update_label, empty_text, home, row_label, summary, update_tooltip,
};
