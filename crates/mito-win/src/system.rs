//! Datos del sistema Windows: idioma de la interfaz y versión del sistema.

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

/// `LANGID` del idioma de interfaz del usuario (`GetUserDefaultUILanguage`).
///
/// Es el idioma en que el usuario ve Windows, no el formato regional.
pub fn ui_language_id() -> u16 {
    // SAFETY: la función no recibe punteros ni estado y siempre devuelve un LANGID.
    unsafe { GetUserDefaultUILanguage() }
}

/// Versión de Windows como `(mayor, menor, compilación)`, o `None` si falla.
///
/// Usa `RtlGetVersion`, que no depende del manifiesto de la aplicación (a diferencia
/// de `GetVersionEx`, que miente en ejecutables sin manifiesto).
pub fn windows_version() -> Option<(u32, u32, u32)> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `info` es una estructura válida y con su tamaño inicializado; la función
    // solo escribe dentro de ella durante la llamada.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    status
        .is_ok()
        .then_some((info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_version_de_windows_es_plausible() {
        let (major, _, build) = windows_version().unwrap();
        assert!(major >= 10);
        assert!(build > 0);
    }

    #[test]
    fn el_idioma_de_interfaz_no_es_cero() {
        assert_ne!(ui_language_id(), 0);
    }
}
