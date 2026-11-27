//! `YitoTerminal.exe`: copia de `WindowsTerminal.exe` con el icono de MITO
//! (`terminal_branding.py`).
//!
//! El original nunca se modifica. La copia se reconstruye solo si cambian los hashes del
//! original o del icono (`.mito-branding.json`). Si no se puede reconstruir, el llamador
//! sigue con `WindowsTerminal.exe` (ADR 0010, punto 6).

use std::fs;
use std::path::{Path, PathBuf};

use mito_core::diagnostics::{self, LogLevel};
use mito_core::storage::{read_json, write_atomic, write_json};
use mito_core::{Layout, Message};
use mito_win::resources::{IconError, IconImage, replace_group_icons};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::digest::sha256_hex;
use crate::error::{Result, TerminalError};

/// Icono de MITO incluido en el binario, por si la instancia no lo trae.
const DEFAULT_ICON: &[u8] = include_bytes!("../../../assets/branding/mito.ico");

/// Iconos de `Images/` que Terminal muestra en sus ventanas.
const IMAGE_ICONS: [&str; 2] = ["terminal_contrast-black.ico", "terminal_contrast-white.ico"];

fn invalid(message: &'static str) -> TerminalError {
    TerminalError::Core(mito_core::Error::domain(Message::new(message)))
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    bytes
        .get(at..at + 2)
        .map_or(0, |pair| u16::from_le_bytes([pair[0], pair[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |quad| {
        u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]])
    })
}

/// Imágenes de un `.ico` (`_ico_frames`).
///
/// # Errors
///
/// `El ICO de MITO está truncado`, `El ICO de MITO no es válido` y `El ICO de MITO
/// contiene una imagen inválida`.
pub fn ico_images(bytes: &[u8]) -> Result<Vec<IconImage>> {
    if bytes.len() < 6 {
        return Err(invalid("El ICO de MITO está truncado"));
    }
    let count = usize::from(u16_at(bytes, 4));
    let directory = 6 + count * 16;
    if u16_at(bytes, 0) != 0 || u16_at(bytes, 2) != 1 || count == 0 || bytes.len() < directory {
        return Err(invalid("El ICO de MITO no es válido"));
    }
    let mut images = Vec::with_capacity(count);
    for index in 0..count {
        let entry = 6 + index * 16;
        let size = usize::try_from(u32_at(bytes, entry + 8)).unwrap_or(usize::MAX);
        let offset = usize::try_from(u32_at(bytes, entry + 12)).unwrap_or(usize::MAX);
        let end = offset.checked_add(size);
        if size < 1 || offset < directory || end.is_none_or(|end| end > bytes.len()) {
            return Err(invalid("El ICO de MITO contiene una imagen inválida"));
        }
        let mut header = [0_u8; 8];
        header.copy_from_slice(&bytes[entry..entry + 8]);
        images.push(IconImage {
            header,
            data: bytes[offset..offset + size].to_vec(),
        });
    }
    Ok(images)
}

/// Sustitutor de iconos: en producción, [`replace_group_icons`].
type Replacer<'a> = &'a dyn Fn(&Path, &[IconImage]) -> std::result::Result<(), IconError>;

fn icon_error(error: IconError) -> TerminalError {
    match error {
        IconError::Io(error) => TerminalError::Io(error),
        IconError::NoIconGroups => invalid("Windows Terminal no contiene recursos de icono"),
    }
}

/// Como [`ensure_branded_executable`] con el sustituidor de iconos inyectado.
///
/// # Errors
///
/// Los de [`ico_images`], `Windows Terminal no contiene recursos de icono` y los de E/S.
pub fn ensure_branded_with(layout: &Layout, replace: Replacer<'_>) -> Result<PathBuf> {
    let original = layout.windows_terminal_exe();
    let icon = layout.branding_icon();
    if !original.is_file() || !icon.is_file() {
        return Ok(original);
    }
    let branded = layout.branded_terminal_exe();
    let marker = layout.windows_terminal().join(".mito-branding.json");
    let read = |path: &Path| fs::read(path).map_err(|e| mito_core::Error::io(path, e));
    let icon_bytes = read(&icon)?;
    let identity = json!({
        "source_sha256": sha256_hex(&read(&original)?),
        "icon_sha256": sha256_hex(&icon_bytes),
    });
    if branded.is_file()
        && marker.is_file()
        && read_json::<Value>(&marker).is_ok_and(|current| current == identity)
    {
        return Ok(branded);
    }
    let images = ico_images(&icon_bytes)?;
    let folder = layout.windows_terminal();
    let temporary = folder.join(format!(".mito-terminal-{}.exe", Uuid::new_v4().simple()));
    let build = || -> Result<()> {
        fs::copy(&original, &temporary).map_err(|e| mito_core::Error::io(&temporary, e))?;
        replace(&temporary, &images).map_err(icon_error)?;
        fs::rename(&temporary, &branded).map_err(|e| mito_core::Error::io(&branded, e))?;
        write_json(&marker, &identity)?;
        Ok(())
    };
    let result = build();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(branded)
}

/// Deriva `YitoTerminal.exe` de `WindowsTerminal.exe` con los iconos de
/// `core/assets/branding/mito.ico`, solo si cambió alguno de los dos. Devuelve la ruta
/// de `YitoTerminal.exe`, o la del original si falta él o el icono.
///
/// # Errors
///
/// Los de [`ensure_branded_with`]. Con `YitoTerminal.exe` en uso, el renombrado falla y
/// se devuelve el error de E/S.
pub fn ensure_branded_executable(layout: &Layout) -> Result<PathBuf> {
    ensure_branded_with(layout, &replace_group_icons)
}

/// Escribe el icono incluido en el binario si la instancia no tiene `mito.ico`.
fn ensure_icon(layout: &Layout) -> Result<()> {
    let icon = layout.branding_icon();
    if !icon.is_file() {
        write_atomic(&icon, DEFAULT_ICON)?;
    }
    Ok(())
}

/// Sincroniza los iconos de `Images/` con `mito.ico` y deriva `YitoTerminal.exe`.
/// Devuelve `None` si falta `WindowsTerminal.exe`.
///
/// # Errors
///
/// Los de E/S y los de [`ensure_branded_executable`].
pub fn brand_window_icons(layout: &Layout) -> Result<Option<PathBuf>> {
    let folder = layout.windows_terminal();
    if !layout.windows_terminal_exe().is_file() {
        return Ok(None);
    }
    ensure_icon(layout)?;
    let icon = layout.branding_icon();
    let bytes = fs::read(&icon).map_err(|e| mito_core::Error::io(&icon, e))?;
    let images = folder.join("Images");
    fs::create_dir_all(&images).map_err(|e| mito_core::Error::io(&images, e))?;
    for name in IMAGE_ICONS {
        let target = images.join(name);
        if fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&target, &bytes).map_err(|e| mito_core::Error::io(&target, e))?;
        }
    }
    ensure_branded_executable(layout).map(Some)
}

/// [`brand_window_icons`] sin propagar el error: ante un fallo registra un aviso y
/// `command` seguirá usando `WindowsTerminal.exe` (ADR 0010).
pub fn brand_or_warn(layout: &Layout) -> Option<PathBuf> {
    match brand_window_icons(layout) {
        Ok(path) => path,
        Err(error) => {
            diagnostics::write(
                layout,
                LogLevel::Warning,
                "terminal.branding",
                &format!(
                    "No se pudo preparar YitoTerminal.exe; se usa WindowsTerminal.exe: {error}"
                ),
                &[],
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
