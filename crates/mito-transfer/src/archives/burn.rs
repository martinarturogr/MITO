//! Bundles `WiX` 3 Burn leídos como datos (`burn.py`): se localizan los contenedores de
//! la sección `.wixburn` y sus CAB se descomprimen con el 7-Zip privado. El bundle
//! nunca se ejecuta.
//!
//! Contrato: `docs/spec/modules/burn.md`.

use std::path::Path;

use mito_core::Message;

use crate::error::{Error, Result};

/// Extrae el contenido de `archive` en `destination` (`extract_burn`). `log` recibe el
/// progreso.
///
/// # Errors
///
/// `Burn: …` (sin traducir, como el original), los de 7-Zip, límites y E/S.
pub fn extract_burn(
    archive: &Path,
    destination: &Path,
    sevenzip: Option<&Path>,
    log: &dyn Fn(Message),
) -> Result<()> {
    let _ = (archive, destination, sevenzip, log);
    Err(Error::untranslated("Burn: extractor pendiente de F7"))
}
