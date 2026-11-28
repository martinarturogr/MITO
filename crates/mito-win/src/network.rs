//! Estado de la red local según `WinINet` (`Network.local_network` del original).

use windows::Win32::Networking::WinInet::{INTERNET_CONNECTION, InternetGetConnectedState};

/// ¿Hay alguna conexión de red local (LAN, módem o proxy configurado)?
///
/// Es `InternetGetConnectedState`, como el original: no comprueba Internet, solo que el
/// equipo esté conectado a alguna red. Sirve para distinguir «Sin conexión de red» de
/// «Internet no accesible».
pub fn local_network() -> bool {
    let mut flags = INTERNET_CONNECTION(0);
    // SAFETY: `flags` es un entero local válido durante la llamada; el segundo argumento
    // está reservado y debe ser cero.
    unsafe { InternetGetConnectedState(&raw mut flags, None) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_consulta_no_falla() {
        // El resultado depende del equipo; solo se comprueba que es estable.
        assert_eq!(local_network(), local_network());
    }
}
