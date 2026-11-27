//! Recursos de icono de un ejecutable PE (`RT_ICON` y `RT_GROUP_ICON`).
//!
//! Sustituye los iconos de una copia de Windows Terminal por los de MITO sin tocar el
//! original: `BeginUpdateResourceW`, `UpdateResourceW` y `EndUpdateResourceW`.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, EnumResourceLanguagesW, EnumResourceNamesW,
    LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW, UpdateResourceW,
};
use windows::core::{BOOL, PCWSTR};

use crate::registry::wide;

/// `RT_ICON`.
const RT_ICON: usize = 3;
/// `RT_GROUP_ICON`.
const RT_GROUP_ICON: usize = 14;
/// Identificador del primer `RT_ICON` nuevo; los siguientes son consecutivos.
pub const FIRST_ICON_ID: u16 = 0x7000;
/// Los nombres de recurso con valor mayor que esto son punteros a cadenas.
const MAX_INTEGER_NAME: usize = 0xFFFF;

/// Una imagen de un `.ico`: los seis primeros bytes de su entrada (ancho, alto, colores,
/// reservado, planos y bits por píxel) y sus datos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconImage {
    /// `width, height, colorcount, reserved` y después `planes` y `bitcount` (`u16` LE): los seis primeros campos de la entrada del `.ico`.
    pub header: [u8; 8],
    /// Bytes de la imagen (PNG o DIB).
    pub data: Vec<u8>,
}

/// Nombre de un recurso: un entero (`MAKEINTRESOURCE`) o una cadena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceName {
    /// Identificador numérico.
    Id(u16),
    /// Nombre textual.
    Text(String),
}

/// Fallos al sustituir los iconos.
#[derive(Debug, thiserror::Error)]
pub enum IconError {
    /// Error del sistema (`WinError`).
    #[error(transparent)]
    Io(#[from] io::Error),
    /// El ejecutable no tiene ningún grupo de iconos que sustituir.
    #[error("el ejecutable no contiene recursos de icono")]
    NoIconGroups,
}

fn os_error(error: &windows::core::Error) -> io::Error {
    // Un HRESULT `HRESULT_FROM_WIN32` conserva el código de Win32 en los 16 bits bajos.
    io::Error::from_raw_os_error(error.code().0 & 0xffff)
}

/// Un módulo cargado como datos, que se libera al soltarlo.
struct DataModule(HMODULE);

impl Drop for DataModule {
    fn drop(&mut self) {
        // SAFETY: `self.0` es un módulo devuelto por `LoadLibraryExW` que solo se
        // libera aquí, una vez.
        let _ = unsafe { FreeLibrary(self.0) };
    }
}

/// Convierte un nombre o tipo de recurso en el `PCWSTR` que esperan las funciones de
/// recursos (`MAKEINTRESOURCE` si es entero).
enum Resource {
    Id(u16),
    Text(Vec<u16>),
}

impl Resource {
    fn from_name(name: &ResourceName) -> Self {
        match name {
            ResourceName::Id(id) => Self::Id(*id),
            ResourceName::Text(text) => Self::Text(wide(text)),
        }
    }

    fn pointer(&self) -> PCWSTR {
        match self {
            Self::Id(id) => PCWSTR(usize::from(*id) as *const u16),
            Self::Text(text) => PCWSTR(text.as_ptr()),
        }
    }
}

/// Lee el nombre que recibe un `callback` de enumeración.
///
/// # Safety
///
/// `name` debe ser un entero de `MAKEINTRESOURCE` o un puntero válido a una cadena
/// terminada en NUL.
unsafe fn read_name(name: PCWSTR) -> ResourceName {
    let value = name.0 as usize;
    if value <= MAX_INTEGER_NAME {
        // El valor cabe en 16 bits por la comprobación anterior.
        return ResourceName::Id(u16::try_from(value).unwrap_or(0));
    }
    // SAFETY: por contrato, `name` apunta a una cadena terminada en NUL.
    ResourceName::Text(unsafe { name.to_string() }.unwrap_or_default())
}

unsafe extern "system" fn collect_names(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    param: isize,
) -> BOOL {
    // SAFETY: `param` es el puntero a un `Vec<ResourceName>` que `group_names` pasa a
    // `EnumResourceNamesW` y que sigue vivo mientras dura la enumeración; la
    // enumeración es síncrona y de un solo hilo.
    let names = unsafe { &mut *(param as *mut Vec<ResourceName>) };
    // SAFETY: Windows entrega un entero de `MAKEINTRESOURCE` o una cadena válida.
    names.push(unsafe { read_name(name) });
    true.into()
}

unsafe extern "system" fn collect_languages(
    _module: HMODULE,
    _kind: PCWSTR,
    _name: PCWSTR,
    language: u16,
    param: isize,
) -> BOOL {
    // SAFETY: `param` es el puntero a un `Vec<u16>` que `group_languages` pasa a
    // `EnumResourceLanguagesW` y que sigue vivo durante la enumeración síncrona.
    let languages = unsafe { &mut *(param as *mut Vec<u16>) };
    languages.push(language);
    true.into()
}

fn group_icon_type() -> PCWSTR {
    PCWSTR(RT_GROUP_ICON as *const u16)
}

/// Pares `(nombre, idioma)` de los grupos de iconos de `executable`.
///
/// # Errors
///
/// El error del sistema si no se puede cargar el ejecutable.
pub fn group_icons(executable: &Path) -> io::Result<Vec<(ResourceName, u16)>> {
    let path = wide(&executable.display().to_string());
    // SAFETY: `path` es una cadena UTF-16 terminada en NUL que vive durante la llamada.
    let module = unsafe {
        LoadLibraryExW(
            PCWSTR(path.as_ptr()),
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    }
    .map_err(|e| os_error(&e))?;
    let module = DataModule(module);
    let mut names: Vec<ResourceName> = Vec::new();
    // SAFETY: `module` es un módulo cargado; `collect_names` respeta la firma y recibe
    // un puntero a `names`, que vive durante esta llamada síncrona.
    unsafe {
        let _ = EnumResourceNamesW(
            Some(module.0),
            group_icon_type(),
            Some(collect_names),
            (&raw mut names) as isize,
        );
    }
    let mut pairs = Vec::new();
    for name in names {
        let resource = Resource::from_name(&name);
        let mut languages: Vec<u16> = Vec::new();
        // SAFETY: igual que arriba, con `collect_languages` y un `Vec<u16>` local; el
        // nombre apunta a un entero o a un búfer propio que sigue vivo.
        let result = unsafe {
            EnumResourceLanguagesW(
                Some(module.0),
                group_icon_type(),
                resource.pointer(),
                Some(collect_languages),
                (&raw mut languages) as isize,
            )
        };
        result.map_err(|e| os_error(&e))?;
        pairs.extend(
            languages
                .into_iter()
                .map(|language| (name.clone(), language)),
        );
    }
    Ok(pairs)
}

/// Datos del grupo (`GRPICONDIR` más una `GRPICONDIRENTRY` de 14 bytes por imagen).
fn group_data(images: &[IconImage]) -> Option<Vec<u8>> {
    let mut data = Vec::with_capacity(6 + images.len() * 14);
    data.extend_from_slice(&0_u16.to_le_bytes());
    data.extend_from_slice(&1_u16.to_le_bytes());
    data.extend_from_slice(&u16::try_from(images.len()).ok()?.to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        data.extend_from_slice(&image.header);
        data.extend_from_slice(&u32::try_from(image.data.len()).ok()?.to_le_bytes());
        let id = FIRST_ICON_ID.checked_add(u16::try_from(index).ok()?)?;
        data.extend_from_slice(&id.to_le_bytes());
    }
    Some(data)
}

/// Sustituye los grupos de iconos de `executable` por `images`.
///
/// Cada `(nombre, idioma)` del grupo original recibe el mismo conjunto de imágenes,
/// como `RT_ICON` con id `0x7000 + índice`. Los `RT_ICON` originales no se borran
/// (quedan sin referencia). Si algo falla, los cambios se descartan.
///
/// # Errors
///
/// [`IconError::NoIconGroups`] si no hay grupos; [`IconError::Io`] con el error del
/// sistema en cualquier llamada de recursos.
pub fn replace_group_icons(executable: &Path, images: &[IconImage]) -> Result<(), IconError> {
    let groups = group_icons(executable)?;
    if groups.is_empty() {
        return Err(IconError::NoIconGroups);
    }
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "icono demasiado grande");
    let group = group_data(images).ok_or_else(invalid)?;
    let path = wide(&executable.display().to_string());
    // SAFETY: `path` es una cadena UTF-16 terminada en NUL que vive durante la llamada.
    let handle = unsafe { BeginUpdateResourceW(PCWSTR(path.as_ptr()), false) }
        .map_err(|e| IconError::Io(os_error(&e)))?;
    let result = (|| -> io::Result<()> {
        for (name, language) in &groups {
            for (index, image) in images.iter().enumerate() {
                let id = FIRST_ICON_ID
                    .checked_add(u16::try_from(index).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                let size = u32::try_from(image.data.len()).map_err(|_| invalid())?;
                // SAFETY: `handle` viene de `BeginUpdateResourceW`; `image.data` es un
                // búfer válido de `size` bytes que la función copia durante la llamada.
                unsafe {
                    UpdateResourceW(
                        handle,
                        PCWSTR(RT_ICON as *const u16),
                        PCWSTR(usize::from(id) as *const u16),
                        *language,
                        Some(image.data.as_ptr().cast()),
                        size,
                    )
                }
                .map_err(|e| os_error(&e))?;
            }
            let resource = Resource::from_name(name);
            let size = u32::try_from(group.len()).map_err(|_| invalid())?;
            // SAFETY: igual que arriba; `resource` mantiene vivo el nombre textual y
            // `group` es un búfer válido de `size` bytes.
            unsafe {
                UpdateResourceW(
                    handle,
                    group_icon_type(),
                    resource.pointer(),
                    *language,
                    Some(group.as_ptr().cast()),
                    size,
                )
            }
            .map_err(|e| os_error(&e))?;
        }
        Ok(())
    })();
    // SAFETY: `handle` es el de `BeginUpdateResourceW` y se cierra una sola vez; con
    // `fdiscard` verdadero se descartan los cambios si hubo un error.
    let end = unsafe { EndUpdateResourceW(handle, result.is_err()) };
    result?;
    end.map_err(|e| IconError::Io(os_error(&e)))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn image(width: u8, size: usize) -> IconImage {
        IconImage {
            header: [width, width, 0, 0, 1, 0, 32, 0],
            data: vec![7; size],
        }
    }

    #[test]
    fn el_grupo_lleva_cabecera_y_entradas_de_14_bytes_con_ids_consecutivos() {
        let data = group_data(&[image(16, 3), image(32, 5)]).unwrap();
        assert_eq!(data.len(), 6 + 2 * 14);
        assert_eq!(data[..6], [0, 0, 1, 0, 2, 0]);
        assert_eq!(data[6..14], [16, 16, 0, 0, 1, 0, 32, 0]);
        assert_eq!(data[14..18], 3_u32.to_le_bytes());
        assert_eq!(data[18..20], 0x7000_u16.to_le_bytes());
        assert_eq!(data[32..34], 0x7001_u16.to_le_bytes());
    }

    #[test]
    fn sustituye_los_iconos_de_una_copia_que_sigue_funcionando() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copia.exe");
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        fs::copy(Path::new(&system).join(r"System32\notepad.exe"), &copy).unwrap();
        let original = fs::read(&copy).unwrap();
        let before = group_icons(&copy).unwrap();
        assert_ne!(before.len(), 0);
        replace_group_icons(&copy, &[image(16, 40), image(32, 80)]).unwrap();
        assert_eq!(group_icons(&copy).unwrap(), before);
        assert_ne!(fs::read(&copy).unwrap(), original);
        // Una segunda sustitución sobre los recursos ya cambiados también funciona.
        replace_group_icons(&copy, &[image(48, 24)]).unwrap();
    }

    #[test]
    fn un_archivo_que_no_es_un_pe_falla_sin_modificarlo() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("falso.exe");
        fs::write(&fake, b"no soy un ejecutable").unwrap();
        assert!(replace_group_icons(&fake, &[image(16, 4)]).is_err());
        assert_eq!(fs::read(&fake).unwrap(), b"no soy un ejecutable");
        assert!(group_icons(&dir.path().join("falta.exe")).is_err());
    }
}
