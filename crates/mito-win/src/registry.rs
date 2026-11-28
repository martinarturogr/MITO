//! Registro de Windows: lectura de `HKLM` y claves de `HKCU` («Abrir con», protocolos).

use std::io;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE,
    REG_EXPAND_SZ, REG_NONE, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW,
    RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, PWSTR};

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

/// Rama del registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    CurrentUser,
    /// `HKEY_LOCAL_MACHINE` (solo lectura en MITO).
    LocalMachine,
}

impl Hive {
    fn handle(self) -> HKEY {
        match self {
            Self::CurrentUser => HKEY_CURRENT_USER,
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

/// Valor leído del registro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// `REG_SZ` o `REG_EXPAND_SZ` (sin expandir), sin el NUL final.
    String(String),
    /// `REG_NONE` (las entradas de `OpenWithProgids`).
    None,
    /// Cualquier otro tipo, con su código.
    Other(u32),
}

/// Valor del registro sin interpretar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawValue {
    /// Tipo del valor (`REG_SZ` = 1, `REG_EXPAND_SZ` = 2, `REG_BINARY` = 3…).
    pub kind: u32,
    /// Bytes del valor.
    pub data: Vec<u8>,
}

fn os_error(status: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(status.0).unwrap_or(i32::MAX))
}

fn check(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(os_error(status))
    }
}

/// Clave abierta del registro; se cierra al soltarla.
#[derive(Debug)]
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: `self.0` es una clave abierta por `RegOpenKeyExW`/`RegCreateKeyExW` que
        // se cierra una sola vez.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// Abre `path` (vista de 64 bits). `None` si no existe.
    ///
    /// # Errors
    ///
    /// El error del sistema si existe pero no se puede abrir.
    pub fn open(hive: Hive, path: &str, write: bool) -> io::Result<Option<Self>> {
        let path = wide(path);
        let access = if write {
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY
        } else {
            KEY_READ | KEY_WOW64_64KEY
        };
        let mut key = HKEY::default();
        // SAFETY: `path` es UTF-16 terminado en NUL; `key` es un manejador local que la
        // función escribe.
        let status = unsafe {
            RegOpenKeyExW(
                hive.handle(),
                PCWSTR(path.as_ptr()),
                None,
                access,
                &raw mut key,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        Ok(Some(Self(key)))
    }

    /// Abre o crea `path` con acceso de lectura y escritura (vista de 64 bits).
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn create(hive: Hive, path: &str) -> io::Result<Self> {
        let path = wide(path);
        let mut key = HKEY::default();
        // SAFETY: `path` es UTF-16 terminado en NUL; `key` es un manejador local que la
        // función escribe; el resto de punteros son nulos opcionales.
        check(unsafe {
            RegCreateKeyExW(
                hive.handle(),
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                None,
                &raw mut key,
                None,
            )
        })?;
        Ok(Self(key))
    }

    /// Valor `name` (`""` es el valor por defecto). `None` si no existe.
    ///
    /// # Errors
    ///
    /// El error del sistema si no se puede leer.
    pub fn get(&self, name: &str) -> io::Result<Option<RegValue>> {
        let name = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0_u32;
        // SAFETY: consulta de tamaño: `name` es UTF-16 terminado en NUL y `kind`/`size`
        // son enteros locales.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                None,
                Some(&raw mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        if kind == REG_NONE {
            return Ok(Some(RegValue::None));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Ok(Some(RegValue::Other(kind.0)));
        }
        let mut buffer = vec![0_u16; (size as usize).div_ceil(2) + 1];
        let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: `buffer` tiene `bytes` bytes; la función escribe como mucho `bytes`.
        check(unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                Some(buffer.as_mut_ptr().cast()),
                Some(&raw mut bytes),
            )
        })?;
        let mut length = (bytes as usize / 2).min(buffer.len());
        while length > 0 && buffer[length - 1] == 0 {
            length -= 1;
        }
        Ok(Some(RegValue::String(String::from_utf16_lossy(
            &buffer[..length],
        ))))
    }

    /// Texto de `name` si es `REG_SZ`/`REG_EXPAND_SZ`.
    ///
    /// # Errors
    ///
    /// El error del sistema si no se puede leer.
    pub fn get_string(&self, name: &str) -> io::Result<Option<String>> {
        Ok(match self.get(name)? {
            Some(RegValue::String(text)) => Some(text),
            _ => None,
        })
    }

    /// Valor `name` sin interpretar: su tipo (`REG_*`) y sus bytes. `None` si no existe.
    ///
    /// Lo usa el respaldo de las claves de protocolo, que debe restaurar cualquier tipo
    /// (binarios, `DWORD`…) tal cual.
    ///
    /// # Errors
    ///
    /// El error del sistema si no se puede leer.
    pub fn get_raw(&self, name: &str) -> io::Result<Option<RawValue>> {
        let name = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0_u32;
        // SAFETY: consulta de tamaño: `name` es UTF-16 terminado en NUL y `kind`/`size`
        // son enteros locales.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                None,
                Some(&raw mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        // El valor puede crecer entre las dos llamadas: se reintenta con el tamaño nuevo.
        let mut data = vec![0_u8; size as usize + 2];
        loop {
            let mut bytes = u32::try_from(data.len()).unwrap_or(u32::MAX);
            // SAFETY: `data` tiene `bytes` bytes; la función escribe como mucho `bytes`.
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    PCWSTR(name.as_ptr()),
                    None,
                    Some(&raw mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&raw mut bytes),
                )
            };
            if status == ERROR_MORE_DATA {
                data = vec![0_u8; (bytes as usize).max(data.len()) * 2];
                continue;
            }
            check(status)?;
            data.truncate((bytes as usize).min(data.len()));
            return Ok(Some(RawValue {
                kind: kind.0,
                data,
            }));
        }
    }

    /// Escribe `name` con el tipo `kind` y los bytes `data` tal cual.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn set_raw(&self, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
        self.set(name, REG_VALUE_TYPE(kind), Some(data))
    }

    fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: Option<&[u8]>) -> io::Result<()> {
        let name = wide(name);
        // SAFETY: `name` es UTF-16 terminado en NUL y `data` (si hay) vive durante la llamada.
        check(unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, kind, data) })
    }

    /// Escribe `name` como `REG_SZ`.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn set_string(&self, name: &str, value: &str) -> io::Result<()> {
        let data: Vec<u8> = wide(value)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        self.set(name, REG_SZ, Some(&data))
    }

    /// Escribe `name` como `REG_NONE` vacío.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn set_none(&self, name: &str) -> io::Result<()> {
        self.set(name, REG_NONE, None)
    }

    /// Borra el valor `name`; no existir no es un error.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn delete_value(&self, name: &str) -> io::Result<()> {
        let name = wide(name);
        // SAFETY: `name` es UTF-16 terminado en NUL.
        let status = unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }

    /// Borra la subclave `name` con todo su contenido; no existir no es un error.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn delete_tree(&self, name: &str) -> io::Result<()> {
        let name = wide(name);
        // SAFETY: `name` es UTF-16 terminado en NUL.
        let status = unsafe { RegDeleteTreeW(self.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)?;
        // `RegDeleteTreeW` vacía la subclave; se borra la propia clave.
        // SAFETY: como arriba.
        let status = unsafe { RegDeleteKeyW(self.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }

    /// Nombres de las subclaves.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn subkeys(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let mut buffer = vec![0_u16; 256];
        for index in 0.. {
            let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            // SAFETY: `buffer` tiene `length` unidades; la función escribe el nombre y
            // deja en `length` su longitud sin NUL.
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            check(status)?;
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        Ok(names)
    }

    /// Nombres de los valores (el valor por defecto aparece como `""`).
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn value_names(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let mut buffer = vec![0_u16; 16_384];
        for index in 0.. {
            let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            // SAFETY: `buffer` tiene `length` unidades; la función escribe el nombre y
            // deja en `length` su longitud sin NUL. No se piden los datos.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            check(status)?;
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        Ok(names)
    }
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
    fn crea_escribe_lee_enumera_y_borra_en_hkcu() {
        let path = format!(r"Software\MITO-tests\registro-{}", std::process::id());
        let key = Key::create(Hive::CurrentUser, &path).unwrap();
        key.set_string("", "valor por defecto ñ").unwrap();
        key.set_string("Nombre", "Aplicación").unwrap();
        key.set_none("MITO.app.1").unwrap();
        let child = Key::create(Hive::CurrentUser, &format!(r"{path}\shell\open\command")).unwrap();
        child.set_string("", r#""C:\MITO\app.exe" "%1""#).unwrap();
        drop(child);

        let read = Key::open(Hive::CurrentUser, &path, false).unwrap().unwrap();
        assert_eq!(
            read.get_string("").unwrap().as_deref(),
            Some("valor por defecto ñ")
        );
        assert_eq!(
            read.get("Nombre").unwrap(),
            Some(RegValue::String("Aplicación".into()))
        );
        assert_eq!(read.get("MITO.app.1").unwrap(), Some(RegValue::None));
        assert_eq!(read.get("NoExiste").unwrap(), None);
        let mut values = read.value_names().unwrap();
        values.sort();
        assert_eq!(values, ["", "MITO.app.1", "Nombre"]);
        assert_eq!(read.subkeys().unwrap(), ["shell"]);
        let command = Key::open(
            Hive::CurrentUser,
            &format!(r"{path}\shell\open\command"),
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            command.get_string("").unwrap().as_deref(),
            Some(r#""C:\MITO\app.exe" "%1""#)
        );
        drop(command);
        drop(read);

        key.delete_value("Nombre").unwrap();
        key.delete_value("Nombre").unwrap();
        assert_eq!(key.get("Nombre").unwrap(), None);
        key.delete_tree("shell").unwrap();
        key.delete_tree("shell").unwrap();
        assert_eq!(key.subkeys().unwrap(), Vec::<String>::new());
        drop(key);

        let parent = Key::open(Hive::CurrentUser, r"Software\MITO-tests", true)
            .unwrap()
            .unwrap();
        parent
            .delete_tree(&format!("registro-{}", std::process::id()))
            .unwrap();
        assert!(
            Key::open(Hive::CurrentUser, &path, false)
                .unwrap()
                .is_none()
        );
        assert!(
            Key::open(
                Hive::LocalMachine,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion",
                false
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn los_valores_en_bruto_conservan_tipo_y_bytes() {
        let name = format!("raw-{}", std::process::id());
        let path = format!(r"Software\MITO-tests\{name}");
        let key = Key::create(Hive::CurrentUser, &path).unwrap();
        key.set_raw("Blob", 3, &[0, 1, 255]).unwrap();
        key.set_raw("Numero", 4, &7_u32.to_le_bytes()).unwrap();
        key.set_string("Texto", "ñ").unwrap();
        assert_eq!(
            key.get_raw("Blob").unwrap(),
            Some(RawValue {
                kind: 3,
                data: vec![0, 1, 255]
            })
        );
        assert_eq!(
            key.get_raw("Numero").unwrap(),
            Some(RawValue {
                kind: 4,
                data: 7_u32.to_le_bytes().to_vec()
            })
        );
        let text = key.get_raw("Texto").unwrap().unwrap();
        assert_eq!(text.kind, 1);
        assert_eq!(text.data, [0xf1, 0, 0, 0]);
        assert_eq!(key.get_raw("NoExiste").unwrap(), None);
        drop(key);
        let parent = Key::open(Hive::CurrentUser, r"Software\MITO-tests", true)
            .unwrap()
            .unwrap();
        parent.delete_tree(&name).unwrap();
    }

    #[test]
    fn la_cadena_ancha_termina_en_nul() {
        assert_eq!(wide("ab"), [97, 98, 0]);
    }
}
