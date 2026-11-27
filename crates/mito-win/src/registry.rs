//! Lectura del registro de Windows.

use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::PCWSTR;

/// Convierte `text` en una cadena UTF-16 terminada en NUL.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Valor `DWORD` de `HKEY_LOCAL_MACHINE\<subkey>`, o `None` si la clave o el valor no
/// existen, no son un `DWORD` o no se pueden leer. Usa la vista de 64 bits.
pub fn local_machine_dword(subkey: &str, name: &str) -> Option<u32> {
    let (subkey, name) = (wide(subkey), wide(name));
    let mut data = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    // SAFETY: `subkey` y `name` son cadenas UTF-16 terminadas en NUL que viven durante
    // la llamada; `data` y `size` son enteros locales válidos y `size` declara el
    // tamaño real del búfer `data`, que la función solo escribe.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    status.is_ok().then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_un_dword_existente_y_no_inventa_los_ausentes() {
        assert!(
            local_machine_dword(
                r"SYSTEM\CurrentControlSet\Control\FileSystem",
                "NtfsDisable8dot3NameCreation"
            )
            .is_some()
        );
        assert_eq!(
            local_machine_dword(r"SYSTEM\CurrentControlSet\Control\FileSystem", "NoExiste"),
            None
        );
        assert_eq!(local_machine_dword(r"SOFTWARE\MitoNoExiste", "x"), None);
    }

    #[test]
    fn la_cadena_ancha_termina_en_nul() {
        assert_eq!(wide("ab"), [97, 98, 0]);
    }
}
