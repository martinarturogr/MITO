//! Modo de la consola de Windows.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
    GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
};

/// Eco de la entrada de la consola desactivado mientras vive (`Read-Host -AsSecureString`
/// del bootstrap original). Al soltarse restaura el modo anterior.
#[derive(Debug)]
pub struct HiddenInput {
    handle: HANDLE,
    mode: CONSOLE_MODE,
}

impl HiddenInput {
    /// Desactiva el eco de la entrada estándar. `None` si la entrada no es una consola
    /// (tubería o archivo): entonces no hay eco que ocultar.
    pub fn new() -> Option<Self> {
        // SAFETY: la función no recibe punteros; un fallo devuelve un error.
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) }.ok()?;
        let mut mode = CONSOLE_MODE(0);
        // SAFETY: `handle` es el handle estándar y `mode` un entero local válido.
        unsafe { GetConsoleMode(handle, &raw mut mode) }.ok()?;
        let hidden = CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT.0);
        // SAFETY: `handle` es una consola válida (el `GetConsoleMode` anterior funcionó).
        unsafe { SetConsoleMode(handle, hidden) }.ok()?;
        Some(Self { handle, mode })
    }
}

impl Drop for HiddenInput {
    fn drop(&mut self) {
        // SAFETY: `handle` es la consola cuyo modo se cambió en `new`; se restaura el
        // modo original. Un fallo aquí no tiene arreglo y se ignora.
        let _ = unsafe { SetConsoleMode(self.handle, self.mode) };
    }
}

/// Activa las secuencias de escape ANSI en la salida estándar. Devuelve `false` si la
/// salida no es una consola o no admite colores.
pub fn enable_virtual_terminal() -> bool {
    // SAFETY: la función no recibe punteros; un fallo devuelve un error.
    let Ok(handle) = (unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }) else {
        return false;
    };
    let mut mode = CONSOLE_MODE(0);
    // SAFETY: `handle` es el handle estándar y `mode` un entero local válido.
    if unsafe { GetConsoleMode(handle, &raw mut mode) }.is_err() {
        return false;
    }
    if mode.0 & ENABLE_VIRTUAL_TERMINAL_PROCESSING.0 != 0 {
        return true;
    }
    // SAFETY: `handle` es una consola válida (el `GetConsoleMode` anterior funcionó).
    unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_falla_aunque_la_salida_no_sea_una_consola() {
        // Bajo `cargo test` la salida suele ser una tubería: solo se comprueba que no entra
        // en pánico y que el resultado es coherente entre llamadas.
        assert_eq!(enable_virtual_terminal(), enable_virtual_terminal());
    }

    #[test]
    fn ocultar_la_entrada_sin_consola_no_falla() {
        // Bajo `cargo test` la entrada no suele ser una consola: `None`, o un guardián
        // que restaura el modo al soltarse.
        drop(HiddenInput::new());
    }
}
