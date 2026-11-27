//! Paquetes MSI extraídos como datos (`msi.py`): la base se lee con `msi.dll` en modo
//! lectura ([`mito_win::msi`]) y los CAB incrustados se descomprimen con el 7-Zip
//! privado. Nunca se ejecutan acciones de instalación.
//!
//! Contrato: `docs/spec/modules/msi.md`.

use std::path::Path;

use mito_core::Message;

use crate::error::{Error, Result};

/// Extrae los archivos de `archive` en `destination` (`extract_msi`). `log` recibe el
/// progreso (`MSI: CAB {p1}/{p2}`).
///
/// # Errors
///
/// `MSI: …` (sin traducir, como el original), los de 7-Zip, límites y E/S.
pub fn extract_msi(
    archive: &Path,
    destination: &Path,
    sevenzip: Option<&Path>,
    log: &dyn Fn(Message),
) -> Result<()> {
    let _ = (archive, destination, sevenzip, log);
    Err(Error::untranslated("MSI: extractor pendiente de F7"))
}
