//! Shell de Windows: carpetas conocidas, accesos directos `.lnk` y aviso de cambios de
//! asociaciones.
//!
//! El original escribía los `.lnk` con `WScript.Shell` desde PowerShell; aquí se usa
//! directamente `IShellLinkW` e `IPersistFile`, que es lo que hay debajo. Los campos son
//! los mismos: destino, argumentos, directorio de trabajo e icono.

use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs,
    FOLDERID_PublicDesktop, FOLDERID_RoamingAppData, IShellLinkW, KF_FLAG_DEFAULT,
    SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{GUID, Interface as _, PCWSTR};

use crate::registry::wide;
use crate::taskbar::ComApartment;

/// Longitud máxima de los campos de texto de un `.lnk` que se leen.
const FIELD: usize = 32_768;

fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xffff)
}

/// Carpetas conocidas que usa MITO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownFolder {
    /// Escritorio del usuario.
    Desktop,
    /// Escritorio público (todos los usuarios).
    PublicDesktop,
    /// Programas del menú Inicio del usuario.
    Programs,
    /// Programas del menú Inicio de todos los usuarios.
    CommonPrograms,
    /// `AppData\Roaming` del usuario (CSIDL 26).
    RoamingAppData,
    /// `AppData\Local` del usuario (CSIDL 28).
    LocalAppData,
}

impl KnownFolder {
    fn id(self) -> GUID {
        match self {
            Self::Desktop => FOLDERID_Desktop,
            Self::PublicDesktop => FOLDERID_PublicDesktop,
            Self::Programs => FOLDERID_Programs,
            Self::CommonPrograms => FOLDERID_CommonPrograms,
            Self::RoamingAppData => FOLDERID_RoamingAppData,
            Self::LocalAppData => FOLDERID_LocalAppData,
        }
    }
}

/// Ruta real de una carpeta conocida (`SHGetKnownFolderPath`), siempre la del usuario
/// del host aunque el entorno del proceso apunte a `home/`.
///
/// # Errors
///
/// El error del sistema.
pub fn known_folder(folder: KnownFolder) -> io::Result<PathBuf> {
    let id = folder.id();
    // SAFETY: `id` es un GUID local; sin token (usuario actual). La cadena devuelta se
    // libera con `CoTaskMemFree` tras copiarla.
    let raw = unsafe { SHGetKnownFolderPath(&raw const id, KF_FLAG_DEFAULT, None) }
        .map_err(|e| os_error(&e))?;
    // SAFETY: `raw` es una cadena UTF-16 terminada en NUL devuelta por la función.
    let text = unsafe { raw.to_string() };
    // SAFETY: `raw` se reservó con el asignador de COM y no se usa después.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    text.map(PathBuf::from)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
}

/// Campos de un acceso directo que escribe y lee MITO.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// `TargetPath`.
    pub target: PathBuf,
    /// `Arguments`, ya con las comillas de `list2cmdline`.
    pub arguments: String,
    /// `WorkingDirectory`.
    pub working_directory: Option<PathBuf>,
    /// `IconLocation` (ruta e índice).
    pub icon: Option<(PathBuf, i32)>,
}

fn create_shell_link() -> io::Result<IShellLinkW> {
    // SAFETY: COM está inicializado por el llamador (`ComApartment`); se pide la
    // interfaz `IShellLinkW` del objeto `ShellLink`.
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.map_err(|e| os_error(&e))
}

/// Escribe (o sustituye) el acceso directo `path` con los campos de `link`.
///
/// # Errors
///
/// El error del sistema de COM o del archivo.
pub fn write_link(path: &Path, link: &Link) -> io::Result<()> {
    let _com = ComApartment::new()?;
    let shell = create_shell_link()?;
    let target = wide(&link.target.display().to_string());
    let arguments = wide(&link.arguments);
    // SAFETY: `shell` es una interfaz válida y las cadenas son UTF-16 terminadas en NUL
    // que viven durante cada llamada; la interfaz copia los valores.
    unsafe {
        shell
            .SetPath(PCWSTR(target.as_ptr()))
            .map_err(|e| os_error(&e))?;
        shell
            .SetArguments(PCWSTR(arguments.as_ptr()))
            .map_err(|e| os_error(&e))?;
        if let Some(directory) = &link.working_directory {
            let directory = wide(&directory.display().to_string());
            shell
                .SetWorkingDirectory(PCWSTR(directory.as_ptr()))
                .map_err(|e| os_error(&e))?;
        }
        if let Some((icon, index)) = &link.icon {
            let icon = wide(&icon.display().to_string());
            shell
                .SetIconLocation(PCWSTR(icon.as_ptr()), *index)
                .map_err(|e| os_error(&e))?;
        }
    }
    let file: IPersistFile = shell.cast().map_err(|e| os_error(&e))?;
    let path = wide(&path.display().to_string());
    // SAFETY: `file` es la interfaz de persistencia del mismo objeto; `path` es UTF-16
    // terminado en NUL; `true` marca el archivo como el documento actual.
    unsafe { file.Save(PCWSTR(path.as_ptr()), true) }.map_err(|e| os_error(&e))
}

/// Lee un buffer UTF-16 terminado en NUL.
fn text(buffer: &[u16]) -> String {
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

/// Lee los campos del acceso directo `path` (destino sin resolver, tal cual se guardó).
///
/// # Errors
///
/// El error del sistema si no es un `.lnk` legible.
pub fn read_link(path: &Path) -> io::Result<Link> {
    let _com = ComApartment::new()?;
    let shell = create_shell_link()?;
    let file: IPersistFile = shell.cast().map_err(|e| os_error(&e))?;
    let wide_path = wide(&path.display().to_string());
    // SAFETY: `file` es una interfaz válida y `wide_path` es UTF-16 terminado en NUL.
    unsafe { file.Load(PCWSTR(wide_path.as_ptr()), STGM_READ) }.map_err(|e| os_error(&e))?;
    let mut target = vec![0_u16; FIELD];
    let mut arguments = vec![0_u16; FIELD];
    let mut directory = vec![0_u16; FIELD];
    let mut icon = vec![0_u16; FIELD];
    let mut index = 0_i32;
    // SAFETY: cada búfer tiene `FIELD` unidades, que es el tamaño que se declara; la
    // interfaz escribe cadenas terminadas en NUL dentro de ellos.
    unsafe {
        shell
            .GetPath(&mut target, std::ptr::null_mut(), 0)
            .map_err(|e| os_error(&e))?;
        shell
            .GetArguments(&mut arguments)
            .map_err(|e| os_error(&e))?;
        shell
            .GetWorkingDirectory(&mut directory)
            .map_err(|e| os_error(&e))?;
        shell
            .GetIconLocation(&mut icon, &raw mut index)
            .map_err(|e| os_error(&e))?;
    }
    let directory = text(&directory);
    let icon = text(&icon);
    Ok(Link {
        target: PathBuf::from(text(&target)),
        arguments: text(&arguments),
        working_directory: (!directory.is_empty()).then(|| PathBuf::from(directory)),
        icon: (!icon.is_empty()).then(|| (PathBuf::from(icon), index)),
    })
}

/// Avisa al shell de que cambiaron las asociaciones de archivos («Abrir con»,
/// protocolos), como `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
pub fn notify_associations_changed() {
    // SAFETY: con `SHCNE_ASSOCCHANGED` los dos elementos deben ser nulos.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_carpetas_conocidas_existen() {
        for folder in [
            KnownFolder::Desktop,
            KnownFolder::PublicDesktop,
            KnownFolder::Programs,
            KnownFolder::CommonPrograms,
            KnownFolder::RoamingAppData,
            KnownFolder::LocalAppData,
        ] {
            let path = known_folder(folder).unwrap();
            assert!(
                path.is_absolute() && path.is_dir(),
                "{folder:?}: {}",
                path.display()
            );
        }
    }

    #[test]
    fn un_acceso_se_escribe_y_se_lee_igual() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("Prueba ñ.lnk");
        let link = Link {
            target: PathBuf::from(r"C:\Windows\System32\notepad.exe"),
            arguments: r#"run --profile test --app "extras/app" -- "two words""#.to_owned(),
            working_directory: Some(folder.path().to_path_buf()),
            icon: Some((PathBuf::from(r"C:\Windows\System32\shell32.dll"), 0)),
        };
        write_link(&path, &link).unwrap();
        assert!(path.is_file());
        assert_eq!(read_link(&path).unwrap(), link);
        // Sustituir conserva un único archivo con los campos nuevos.
        let other = Link {
            arguments: String::new(),
            working_directory: None,
            icon: None,
            ..link
        };
        write_link(&path, &other).unwrap();
        assert_eq!(read_link(&path).unwrap(), other);
        assert!(read_link(&folder.path().join("no-existe.lnk")).is_err());
        notify_associations_changed();
    }
}
