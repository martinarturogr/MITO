//! Ventanas de nivel superior e identidad de la barra de tareas (`IPropertyStore`).

use std::ffi::c_void;
use std::io;
use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemAlloc, CoUninitialize,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId,
};
use windows::core::{BOOL, GUID, PWSTR};

use crate::registry::wide;

/// `fmtid` de las propiedades `System.AppUserModel.*` (`pid` 2 `RelaunchCommand`, 3
/// `RelaunchIconResource`, 4 `RelaunchDisplayNameResource` y 5 `ID`).
pub const APP_USER_MODEL_FORMAT: u128 = 0x9F4C_2855_9F79_4B39_A8D0_E1D4_2DE1_D5F3;

/// Una ventana de nivel superior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Manejador (`HWND`).
    pub hwnd: isize,
    /// Clase de la ventana.
    pub class: String,
    /// Ruta completa del ejecutable del proceso propietario, si se pudo consultar.
    pub image: Option<PathBuf>,
}

fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xffff)
}

unsafe extern "system" fn collect(window: HWND, param: LPARAM) -> BOOL {
    // SAFETY: `param` es el puntero a un `Vec<HWND>` que `top_level_windows` pasa a
    // `EnumWindows` y que sigue vivo durante la enumeración, síncrona y de un solo hilo.
    let windows = unsafe { &mut *(param.0 as *mut Vec<isize>) };
    windows.push(window.0 as isize);
    true.into()
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 256];
    // SAFETY: `buffer` es un búfer local válido; la función solo escribe en él.
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    let length = usize::try_from(length).unwrap_or(0).min(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn process_image(hwnd: HWND) -> Option<PathBuf> {
    let mut pid = 0_u32;
    // SAFETY: `pid` es un entero local válido que la función solo escribe.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    // SAFETY: la llamada solo recibe valores; un fallo devuelve un error.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0_u16; 32_768];
    let mut size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `process` es un handle válido; `buffer` tiene `size` unidades y la función
    // actualiza `size` con la longitud escrita.
    let query = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut size,
        )
    };
    // SAFETY: `process` es el handle abierto arriba y se cierra una sola vez.
    let _ = unsafe { CloseHandle(process) };
    query.ok()?;
    let length = usize::try_from(size).ok()?.min(buffer.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..length])))
}

/// Ventanas de nivel superior cuya clase es exactamente `class`, con la ruta del
/// ejecutable de su proceso (`None` si no se puede consultar, p. ej. de otro usuario).
///
/// # Errors
///
/// El error del sistema si `EnumWindows` falla.
pub fn top_level_windows(class: &str) -> io::Result<Vec<WindowInfo>> {
    let mut handles: Vec<isize> = Vec::new();
    // SAFETY: `collect` respeta la firma y recibe un puntero a `handles`, que vive
    // durante esta llamada síncrona.
    unsafe { EnumWindows(Some(collect), LPARAM((&raw mut handles) as isize)) }
        .map_err(|e| os_error(&e))?;
    Ok(handles
        .into_iter()
        .filter_map(|handle| {
            let hwnd = HWND(handle as *mut c_void);
            (class_name(hwnd) == class).then(|| WindowInfo {
                hwnd: handle,
                class: class.to_owned(),
                image: process_image(hwnd),
            })
        })
        .collect())
}

/// Apartamento COM del hilo: se libera al soltarlo solo si esta llamada lo inicializó.
#[derive(Debug)]
pub struct ComApartment {
    initialized: bool,
}

impl ComApartment {
    /// Inicializa COM en modo STA. Si el hilo ya lo tenía con otro modo
    /// (`RPC_E_CHANGED_MODE`) se sigue adelante sin liberar nada.
    ///
    /// # Errors
    ///
    /// El error del sistema para cualquier otro HRESULT negativo.
    pub fn new() -> io::Result<Self> {
        // SAFETY: no recibe punteros; la inicialización se empareja con `Drop`.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self { initialized: false });
        }
        result.ok().map_err(|e| os_error(&e))?;
        Ok(Self { initialized: true })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: empareja la llamada exitosa a `CoInitializeEx` de `new`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Escribe en la ventana las propiedades `System.AppUserModel.*` `(pid, valor)` como
/// `VT_LPWSTR`, en el orden dado (las de relanzamiento deben ir antes que el ID).
///
/// # Errors
///
/// El error del sistema si la ventana no tiene almacén de propiedades o `SetValue`
/// falla. El almacén se libera siempre.
pub fn set_app_user_model_properties(
    window: isize,
    properties: &[(u32, String)],
) -> io::Result<()> {
    let hwnd = HWND(window as *mut c_void);
    // SAFETY: `hwnd` es un valor de ventana; si no es válido la función devuelve un
    // HRESULT de error. La interfaz devuelta se libera con `Drop`.
    let store: IPropertyStore =
        unsafe { SHGetPropertyStoreForWindow(hwnd) }.map_err(|e| os_error(&e))?;
    for (pid, value) in properties {
        let key = PROPERTYKEY {
            fmtid: GUID::from_u128(APP_USER_MODEL_FORMAT),
            pid: *pid,
        };
        let variant = lpwstr_variant(value)?;
        // SAFETY: `key` y `variant` son valores locales válidos durante la llamada;
        // `SetValue` copia el valor y no toma posesión de `variant`.
        unsafe { store.SetValue(&raw const key, &raw const variant) }.map_err(|e| os_error(&e))?;
    }
    Ok(())
}

/// `PROPVARIANT` de tipo `VT_LPWSTR` con la cadena en memoria de `CoTaskMemAlloc`, como
/// `InitPropVariantFromString`.
///
/// `PROPVARIANT` implementa `Drop` con `PropVariantClear`, que libera `pwszVal` con
/// `CoTaskMemFree`: apuntarlo a memoria de Rust corrompe el heap al soltarlo
/// (`STATUS_HEAP_CORRUPTION`, 0xC0000374).
fn lpwstr_variant(value: &str) -> io::Result<PROPVARIANT> {
    let text = wide(value);
    let bytes = text.len() * size_of::<u16>();
    // SAFETY: reservar memoria no tiene precondiciones; el resultado se comprueba.
    let buffer = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
    if buffer.is_null() {
        return Err(io::Error::from(io::ErrorKind::OutOfMemory));
    }
    // SAFETY: `buffer` tiene espacio para `text.len()` elementos `u16` recién reservados
    // y no se solapa con `text`.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len()) };
    Ok(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    pwszVal: PWSTR(buffer),
                },
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_enumeracion_devuelve_solo_la_clase_pedida() {
        let none = top_level_windows("MITO_CLASE_QUE_NO_EXISTE").unwrap();
        assert_eq!(none.len(), 0);
        let tray = top_level_windows("Shell_TrayWnd").unwrap();
        assert!(tray.iter().all(|w| w.class == "Shell_TrayWnd"));
    }

    #[test]
    fn el_variante_de_texto_se_libera_con_el_asignador_de_com() {
        // Con la cadena en memoria de Rust, soltar el variante corrompía el heap y el
        // proceso terminaba con 0xC0000374.
        for value in ["MITO.Instance.prueba", "", "ñandú – icono"] {
            let variant = lpwstr_variant(value).unwrap();
            assert_eq!(variant.vt(), VT_LPWSTR);
            // SAFETY: en el test, el variante es `VT_LPWSTR` con una cadena terminada en NUL.
            let text = unsafe { variant.Anonymous.Anonymous.Anonymous.pwszVal.to_string() };
            assert_eq!(text.unwrap(), value);
            drop(variant);
        }
    }

    #[test]
    fn com_se_inicializa_y_se_puede_repetir() {
        let first = ComApartment::new().unwrap();
        let second = ComApartment::new().unwrap();
        drop(second);
        drop(first);
    }

    #[test]
    #[ignore = "necesita una ventana real de Windows Terminal; se prueba a mano"]
    fn una_ventana_real_acepta_las_propiedades() {
        let windows = top_level_windows("CASCADIA_HOSTING_WINDOW_CLASS").unwrap();
        let window = windows.first().expect("abre Windows Terminal antes");
        let _com = ComApartment::new().unwrap();
        set_app_user_model_properties(window.hwnd, &[(5, "MITO.Instance.prueba".into())]).unwrap();
    }
}
