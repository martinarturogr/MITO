//! Modo de la consola de Windows.

use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
    STD_OUTPUT_HANDLE, SetConsoleMode,
};

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
}
