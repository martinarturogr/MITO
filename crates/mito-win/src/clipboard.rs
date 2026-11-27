//! Portapapeles de Windows con texto Unicode (`CF_UNICODETEXT`).
//!
//! Sustituye al `Set-Clipboard` de PowerShell que usaba el original para «Copiar log»:
//! el texto no pasa por ningún proceso externo.

use std::io;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Intentos de abrir el portapapeles: otro proceso puede tenerlo abierto un instante.
const ATTEMPTS: u32 = 10;
/// Pausa entre intentos.
const RETRY: Duration = Duration::from_millis(20);

fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xffff)
}

/// Portapapeles abierto por este hilo; se cierra al soltarlo.
struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open() -> io::Result<Self> {
        let mut last = None;
        for _ in 0..ATTEMPTS {
            // SAFETY: sin ventana propietaria; la apertura se empareja con `Drop`.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(error) => last = Some(error),
            }
            thread::sleep(RETRY);
        }
        Err(last.map_or_else(io::Error::last_os_error, |e| os_error(&e)))
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: empareja la llamada exitosa a `OpenClipboard` de `open`.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Bloque de memoria global que se libera al soltarlo, salvo que el sistema lo adopte.
struct GlobalMemory(HGLOBAL);

impl GlobalMemory {
    /// Copia `text` (UTF-16 terminado en NUL) en un bloque movible.
    fn with_text(text: &[u16]) -> io::Result<Self> {
        let bytes = std::mem::size_of_val(text);
        // SAFETY: reservar memoria no tiene precondiciones; el resultado se comprueba.
        let memory = Self(unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }.map_err(|e| os_error(&e))?);
        // SAFETY: `memory.0` es un bloque movible recién reservado.
        let pointer = unsafe { GlobalLock(memory.0) }.cast::<u16>();
        if pointer.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: el bloque tiene `bytes` bytes, es decir `text.len()` unidades `u16`, y
        // no se solapa con `text`.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), pointer, text.len()) };
        // SAFETY: desbloquea el `GlobalLock` anterior. Devuelve un «error» con código 0
        // cuando el contador llega a cero, que es lo esperado.
        let _ = unsafe { GlobalUnlock(memory.0) };
        Ok(memory)
    }

    /// Entrega el bloque al sistema: ya no se libera aquí.
    fn release(self) -> HGLOBAL {
        let memory = self.0;
        std::mem::forget(self);
        memory
    }
}

impl Drop for GlobalMemory {
    fn drop(&mut self) {
        // SAFETY: el bloque es propio (no se entregó al portapapeles) y se libera una vez.
        let _ = unsafe { GlobalFree(Some(self.0)) };
    }
}

/// Sustituye el contenido del portapapeles por `text`.
///
/// # Errors
///
/// El error del sistema si no se puede abrir el portapapeles (tras varios intentos),
/// reservar la memoria o publicar el texto.
pub fn set_text(text: &str) -> io::Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let memory = GlobalMemory::with_text(&wide)?;
    let _open = OpenClipboardGuard::open()?;
    // SAFETY: el portapapeles está abierto por este hilo.
    unsafe { EmptyClipboard() }.map_err(|e| os_error(&e))?;
    // SAFETY: el portapapeles está abierto y vacío; `memory` es un bloque movible con
    // texto UTF-16 terminado en NUL. Si la llamada tiene éxito el sistema adopta el bloque.
    unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0.0))) }
        .map_err(|e| os_error(&e))?;
    memory.release();
    Ok(())
}

/// Texto del portapapeles, o `None` si no contiene texto.
///
/// # Errors
///
/// El error del sistema si no se puede abrir el portapapeles.
pub fn text() -> io::Result<Option<String>> {
    let _open = OpenClipboardGuard::open()?;
    // SAFETY: el portapapeles está abierto; el manejador pertenece al sistema y solo se
    // lee mientras sigue abierto.
    let Ok(handle) = (unsafe { GetClipboardData(u32::from(CF_UNICODETEXT.0)) }) else {
        return Ok(None);
    };
    let memory = HGLOBAL(handle.0);
    // SAFETY: `memory` es el bloque de texto del portapapeles, válido mientras esté abierto.
    let pointer = unsafe { GlobalLock(memory) }.cast::<u16>();
    if pointer.is_null() {
        return Ok(None);
    }
    let mut length = 0;
    // SAFETY: `CF_UNICODETEXT` garantiza una cadena terminada en NUL dentro del bloque.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: las `length` unidades anteriores al NUL están dentro del bloque bloqueado.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
    // SAFETY: desbloquea el `GlobalLock` anterior.
    let _ = unsafe { GlobalUnlock(memory) };
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "sustituye el portapapeles del usuario; se prueba a mano"]
    fn el_texto_copiado_se_lee_igual() {
        let text = "Línea 1\nñandú – «log» 🚀";
        set_text(text).unwrap();
        assert_eq!(super::text().unwrap().as_deref(), Some(text));
        set_text("").unwrap();
        assert_eq!(super::text().unwrap().as_deref(), Some(""));
    }

    #[test]
    fn la_memoria_no_entregada_se_libera() {
        // Reservar y soltar sin entregar al sistema no debe fallar ni filtrar memoria.
        let memory = GlobalMemory::with_text(&[0x61, 0]).unwrap();
        drop(memory);
    }
}
