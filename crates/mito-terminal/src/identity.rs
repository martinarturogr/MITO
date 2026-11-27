//! Identidad de la barra de tareas de las ventanas de la instancia
//! (`terminal_identity.py`).
//!
//! Cada ventana de Windows Terminal de la instancia recibe `System.AppUserModel.ID` por
//! ruta de instancia y las propiedades de relanzamiento (icono y comando de MITO). Un
//! fallo nunca impide arrancar: [`apply_safely`] solo lo registra.

use std::io;
use std::path::Path;

use mito_core::Layout;
use mito_core::diagnostics::{self, LogLevel};
use mito_win::taskbar::{self, ComApartment, WindowInfo};
use serde_json::json;

use crate::commandline::list2cmdline;
use crate::digest::sha256_hex;

/// Clase de las ventanas de Windows Terminal.
pub const WINDOW_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";

/// `pid` de `System.AppUserModel.RelaunchCommand`.
pub const RELAUNCH_COMMAND: u32 = 2;
/// `pid` de `System.AppUserModel.RelaunchIconResource`.
pub const RELAUNCH_ICON: u32 = 3;
/// `pid` de `System.AppUserModel.RelaunchDisplayNameResource`.
pub const RELAUNCH_DISPLAY_NAME: u32 = 4;
/// `pid` de `System.AppUserModel.ID`.
pub const APP_ID: u32 = 5;

/// Los 24 primeros hex del SHA-256 de la ruta resuelta de la raíz en minúsculas.
fn tag(root: &Path) -> String {
    let resolved = dunce::canonicalize(root)
        .or_else(|_| std::path::absolute(root))
        .unwrap_or_else(|_| root.to_path_buf());
    sha256_hex(resolved.display().to_string().to_lowercase().as_bytes())[..24].to_owned()
}

/// Propiedades de identidad, en el orden obligatorio: las de relanzamiento van antes que
/// el ID explícito. Son siempre cuatro `(pid, valor)`.
pub fn identity_properties(layout: &Layout) -> Vec<(u32, String)> {
    let command = list2cmdline(&[layout.mito_exe().display().to_string(), "start".to_owned()]);
    vec![
        (RELAUNCH_COMMAND, command),
        (RELAUNCH_DISPLAY_NAME, "MITO".to_owned()),
        (
            RELAUNCH_ICON,
            format!("{},0", layout.branding_icon().display()),
        ),
        (APP_ID, format!("MITO.Instance.{}", tag(layout.root()))),
    ]
}

/// De las ventanas dadas, las que son de Windows Terminal y de un proceso cuyo ejecutable
/// es exactamente el `YitoTerminal.exe` o el `WindowsTerminal.exe` de esta instancia.
pub fn select_windows(layout: &Layout, windows: &[WindowInfo]) -> Vec<isize> {
    let allowed = [
        layout
            .branded_terminal_exe()
            .display()
            .to_string()
            .to_lowercase(),
        layout
            .windows_terminal_exe()
            .display()
            .to_string()
            .to_lowercase(),
    ];
    windows
        .iter()
        .filter(|window| window.class == WINDOW_CLASS)
        .filter(|window| {
            window
                .image
                .as_ref()
                .is_some_and(|image| allowed.contains(&image.display().to_string().to_lowercase()))
        })
        .map(|window| window.hwnd)
        .collect()
}

/// Ventanas de Windows Terminal de esta instancia.
///
/// # Errors
///
/// El error del sistema si no se pueden enumerar las ventanas.
pub fn terminal_windows(layout: &Layout) -> io::Result<Vec<isize>> {
    let windows = taskbar::top_level_windows(WINDOW_CLASS)?;
    Ok(select_windows(layout, &windows))
}

/// Aplica la identidad a las ventanas de la instancia y devuelve cuántas eran. Sin
/// `mito.ico` no hace nada y devuelve `0`.
///
/// # Errors
///
/// El error del sistema de COM, de la enumeración o del almacén de propiedades de una
/// ventana. Las ventanas anteriores conservan sus propiedades (no hay marcha atrás).
pub fn apply(layout: &Layout) -> io::Result<usize> {
    if !layout.branding_icon().is_file() {
        return Ok(0);
    }
    let _com = ComApartment::new()?;
    let windows = terminal_windows(layout)?;
    let properties = identity_properties(layout);
    for window in &windows {
        taskbar::set_app_user_model_properties(*window, &properties)?;
    }
    Ok(windows.len())
}

/// Como [`apply`], sin propagar los errores del sistema: registra `terminal.identity`
/// con nivel `info` y `windows=<n>`, o con nivel `warning` y el error.
pub fn apply_safely(layout: &Layout) {
    match apply(layout) {
        Ok(count) => {
            diagnostics::write(
                layout,
                LogLevel::Info,
                "terminal.identity",
                "Taskbar identity applied",
                &[("windows", json!(count))],
            );
        }
        Err(error) => {
            diagnostics::write(
                layout,
                LogLevel::Warning,
                "terminal.identity",
                &error.to_string(),
                &[],
            );
        }
    }
}

#[cfg(test)]
mod tests;
