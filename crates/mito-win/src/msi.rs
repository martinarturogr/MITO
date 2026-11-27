//! Lectura de bases de datos de Windows Installer como datos (`Database` de `msi.py`).
//!
//! Solo se abre la base en modo lectura con `MsiOpenDatabaseW`: nunca se usan
//! `MsiOpenPackage`, `DoAction` ni `InstallProduct`. `msi.dll` se carga bajo demanda y
//! solo desde `System32`, como el `WinDLL('msi.dll', winmode=0x800)` del original, para
//! no añadirla a cada arranque de `mito.exe` ni buscarla junto al ejecutable.

use std::ffi::c_void;
use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, PCWSTR, s};

use crate::registry::wide;

/// `ERROR_NO_MORE_ITEMS`.
const NO_MORE_ITEMS: u32 = 259;
/// `ERROR_MORE_DATA`.
const MORE_DATA: u32 = 234;
/// Tamaño máximo de un valor de texto (como el original: reintenta si cabe en 64 Ki).
const MAX_STRING: u32 = 65_536;
/// Bloque de lectura de un flujo.
const CHUNK: usize = 1024 * 1024;

/// Error de lectura de una base MSI. Los textos son los del original, que no se
/// traducen.
#[derive(Debug, thiserror::Error)]
pub enum MsiError {
    /// `msi.dll` devolvió un código de error.
    #[error("MSI: Windows database error {0}")]
    Status(u32),
    /// No se pudo crear el registro del parámetro.
    #[error("MSI: cannot allocate record")]
    Allocation,
    /// La tabla supera el límite de filas.
    #[error("MSI: too many table entries")]
    TooManyEntries,
    /// No existe el flujo pedido en `_Streams`.
    #[error("MSI: missing embedded cabinet {0}")]
    MissingStream(String),
    /// El flujo supera el límite de bytes.
    #[error("MSI: cabinet too large")]
    TooLarge,
    /// `msi.dll` no se pudo cargar o le falta una función.
    #[error("MSI: msi.dll no disponible: {0}")]
    Unavailable(io::Error),
    /// Error al escribir el flujo.
    #[error(transparent)]
    Io(#[from] io::Error),
}

type OpenDatabase = unsafe extern "system" fn(PCWSTR, PCWSTR, *mut u32) -> u32;
type OpenView = unsafe extern "system" fn(u32, PCWSTR, *mut u32) -> u32;
type ViewExecute = unsafe extern "system" fn(u32, u32) -> u32;
type ViewFetch = unsafe extern "system" fn(u32, *mut u32) -> u32;
type CloseHandle = unsafe extern "system" fn(u32) -> u32;
type GetString = unsafe extern "system" fn(u32, u32, *mut u16, *mut u32) -> u32;
type ReadStream = unsafe extern "system" fn(u32, u32, *mut c_void, *mut u32) -> u32;
type CreateRecord = unsafe extern "system" fn(u32) -> u32;
type SetString = unsafe extern "system" fn(u32, u32, PCWSTR) -> u32;
type SetStream = unsafe extern "system" fn(u32, u32, PCWSTR) -> u32;
type Commit = unsafe extern "system" fn(u32) -> u32;

/// Funciones de `msi.dll` que se usan. La biblioteca queda cargada durante todo el
/// proceso (como en el original).
struct Api {
    open_database: OpenDatabase,
    open_view: OpenView,
    view_execute: ViewExecute,
    view_fetch: ViewFetch,
    close_handle: CloseHandle,
    get_string: GetString,
    read_stream: ReadStream,
    create_record: CreateRecord,
    set_string: SetString,
    set_stream: SetStream,
    commit: Commit,
}

/// Dirección de `name` en `module`, convertida al tipo de puntero `F`.
///
/// # Safety
///
/// `F` debe ser un puntero a función `extern "system"` con la firma exacta de `name`.
unsafe fn symbol<F: Copy>(module: HMODULE, name: PCSTR) -> io::Result<F> {
    // SAFETY: `module` es un módulo cargado y `name` una cadena ASCII terminada en NUL.
    let address = unsafe { GetProcAddress(module, name) }.ok_or_else(io::Error::last_os_error)?;
    debug_assert_eq!(size_of::<F>(), size_of_val(&address));
    // SAFETY: el llamador garantiza que `F` es el puntero a función con la firma de
    // `name`, del mismo tamaño que la dirección devuelta.
    Ok(unsafe { std::mem::transmute_copy::<_, F>(&address) })
}

impl Api {
    fn load() -> io::Result<Self> {
        let name = wide("msi.dll");
        // SAFETY: `name` es una cadena UTF-16 terminada en NUL que vive durante la
        // llamada; la búsqueda se limita a System32.
        let module =
            unsafe { LoadLibraryExW(PCWSTR(name.as_ptr()), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xffff))?;
        // SAFETY: cada tipo coincide con la firma documentada de la función de msi.dll.
        let api = unsafe {
            (|| -> io::Result<Self> {
                Ok(Self {
                    open_database: symbol(module, s!("MsiOpenDatabaseW"))?,
                    open_view: symbol(module, s!("MsiDatabaseOpenViewW"))?,
                    view_execute: symbol(module, s!("MsiViewExecute"))?,
                    view_fetch: symbol(module, s!("MsiViewFetch"))?,
                    close_handle: symbol(module, s!("MsiCloseHandle"))?,
                    get_string: symbol(module, s!("MsiRecordGetStringW"))?,
                    read_stream: symbol(module, s!("MsiRecordReadStream"))?,
                    create_record: symbol(module, s!("MsiCreateRecord"))?,
                    set_string: symbol(module, s!("MsiRecordSetStringW"))?,
                    set_stream: symbol(module, s!("MsiRecordSetStreamW"))?,
                    commit: symbol(module, s!("MsiDatabaseCommit"))?,
                })
            })()
        };
        if api.is_err() {
            // SAFETY: `module` se cargó arriba y no se usa después.
            let _ = unsafe { FreeLibrary(module) };
        }
        api
    }

    fn get() -> Result<&'static Self, MsiError> {
        static API: OnceLock<Result<Api, i32>> = OnceLock::new();
        match API.get_or_init(|| Self::load().map_err(|e| e.raw_os_error().unwrap_or(0))) {
            Ok(api) => Ok(api),
            Err(code) => Err(MsiError::Unavailable(io::Error::from_raw_os_error(*code))),
        }
    }
}

fn check(status: u32) -> Result<(), MsiError> {
    if status == 0 {
        Ok(())
    } else {
        Err(MsiError::Status(status))
    }
}

/// Manejador de MSI que se cierra al soltarlo.
struct Handle {
    api: &'static Api,
    value: u32,
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.value != 0 {
            // SAFETY: `value` es un manejador abierto por msi.dll y se cierra una vez.
            unsafe { (self.api.close_handle)(self.value) };
        }
    }
}

/// Base de datos MSI abierta en modo lectura.
pub struct Database {
    api: &'static Api,
    handle: Handle,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("handle", &self.handle.value)
            .finish_non_exhaustive()
    }
}

impl Database {
    /// Abre `path` en modo lectura (`MSIDBOPEN_READONLY`).
    ///
    /// # Errors
    ///
    /// [`MsiError::Unavailable`] si no se puede cargar `msi.dll` y
    /// [`MsiError::Status`] con el código de Windows si la base no se puede abrir.
    pub fn open(path: &Path) -> Result<Self, MsiError> {
        let api = Api::get()?;
        let absolute = std::path::absolute(path)?;
        let path = wide(&absolute.to_string_lossy());
        let mut handle = 0;
        // SAFETY: `path` es UTF-16 terminado en NUL; un `szPersist` nulo equivale a
        // `MSIDBOPEN_READONLY`; `handle` es un entero local que la función escribe.
        check(unsafe {
            (api.open_database)(PCWSTR(path.as_ptr()), PCWSTR::null(), &raw mut handle)
        })?;
        Ok(Self {
            api,
            handle: Handle { api, value: handle },
        })
    }

    /// Ejecuta `sql` con un parámetro de texto opcional (`?`) y devuelve la vista.
    fn view(&self, sql: &str, parameter: Option<&str>) -> Result<Handle, MsiError> {
        let sql = wide(sql);
        let mut view = Handle {
            api: self.api,
            value: 0,
        };
        // SAFETY: base abierta; `sql` es UTF-16 terminado en NUL; `view.value` se escribe.
        check(unsafe {
            (self.api.open_view)(self.handle.value, PCWSTR(sql.as_ptr()), &raw mut view.value)
        })?;
        let record = match parameter {
            Some(parameter) => {
                // SAFETY: crear un registro de un campo no tiene precondiciones.
                let value = unsafe { (self.api.create_record)(1) };
                if value == 0 {
                    return Err(MsiError::Allocation);
                }
                let record = Handle {
                    api: self.api,
                    value,
                };
                let text = wide(parameter);
                // SAFETY: registro válido; `text` es UTF-16 terminado en NUL.
                check(unsafe { (self.api.set_string)(record.value, 1, PCWSTR(text.as_ptr())) })?;
                Some(record)
            }
            None => None,
        };
        let record_value = record.as_ref().map_or(0, |r| r.value);
        // SAFETY: vista abierta arriba; el registro (o 0) sigue vivo durante la llamada.
        check(unsafe { (self.api.view_execute)(view.value, record_value) })?;
        Ok(view)
    }

    /// Siguiente registro de la vista, o `None` al final.
    fn fetch(&self, view: &Handle) -> Result<Option<Handle>, MsiError> {
        let mut record = 0;
        // SAFETY: vista ejecutada; `record` es un entero local que la función escribe.
        let status = unsafe { (self.api.view_fetch)(view.value, &raw mut record) };
        if status == NO_MORE_ITEMS {
            return Ok(None);
        }
        check(status)?;
        Ok(Some(Handle {
            api: self.api,
            value: record,
        }))
    }

    /// Texto del campo `field` (desde 1) de `record`.
    fn string(&self, record: &Handle, field: u32) -> Result<String, MsiError> {
        let mut buffer = vec![0_u16; 1024];
        let mut size = 1024_u32;
        // SAFETY: `buffer` tiene `size` unidades; la función escribe como mucho `size`
        // (con el NUL) y actualiza `size` con la longitud sin NUL.
        let mut status = unsafe {
            (self.api.get_string)(record.value, field, buffer.as_mut_ptr(), &raw mut size)
        };
        if status == MORE_DATA && size < MAX_STRING {
            size += 1;
            buffer = vec![0; size as usize];
            // SAFETY: como arriba, con el búfer ampliado a `size` unidades.
            status = unsafe {
                (self.api.get_string)(record.value, field, buffer.as_mut_ptr(), &raw mut size)
            };
        }
        check(status)?;
        let length = (size as usize).min(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..length]))
    }

    /// Filas de `columns` de `table` como texto (`rows` del original). Más de
    /// `max_rows` filas es [`MsiError::TooManyEntries`].
    ///
    /// # Errors
    ///
    /// Los de msi.dll ([`MsiError::Status`]) y el límite de filas.
    pub fn rows(
        &self,
        table: &str,
        columns: &[&str],
        max_rows: usize,
    ) -> Result<Vec<Vec<String>>, MsiError> {
        let list: Vec<String> = columns.iter().map(|c| format!("`{c}`")).collect();
        let sql = format!("SELECT {} FROM `{table}`", list.join(", "));
        let view = self.view(&sql, None)?;
        let mut rows = Vec::new();
        while let Some(record) = self.fetch(&view)? {
            let mut row = Vec::with_capacity(columns.len());
            for field in 1..=columns.len() {
                let field = u32::try_from(field).map_err(|_| MsiError::TooManyEntries)?;
                row.push(self.string(&record, field)?);
            }
            rows.push(row);
            if rows.len() > max_rows {
                return Err(MsiError::TooManyEntries);
            }
        }
        Ok(rows)
    }

    /// Copia el flujo `name` de `_Streams` en `output` (`stream` del original). Más de
    /// `max_bytes` bytes es [`MsiError::TooLarge`]. Devuelve los bytes escritos.
    ///
    /// # Errors
    ///
    /// [`MsiError::MissingStream`] si no existe; los de msi.dll, el límite y la E/S.
    pub fn read_stream(
        &self,
        name: &str,
        output: &mut dyn Write,
        max_bytes: u64,
    ) -> Result<u64, MsiError> {
        let view = self.view("SELECT `Data` FROM `_Streams` WHERE `Name` = ?", Some(name))?;
        let record = self
            .fetch(&view)?
            .ok_or_else(|| MsiError::MissingStream(name.to_owned()))?;
        let mut buffer = vec![0_u8; CHUNK];
        let mut total = 0_u64;
        loop {
            let mut size = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            // SAFETY: `buffer` tiene `size` bytes; la función escribe como mucho `size`
            // y deja en `size` los bytes leídos.
            check(unsafe {
                (self.api.read_stream)(record.value, 1, buffer.as_mut_ptr().cast(), &raw mut size)
            })?;
            if size == 0 {
                break;
            }
            total += u64::from(size);
            if total > max_bytes {
                return Err(MsiError::TooLarge);
            }
            output.write_all(&buffer[..size as usize])?;
        }
        Ok(total)
    }
}

/// `MSIDBOPEN_CREATE`.
const CREATE: usize = 3;

/// Crea una base MSI nueva como datos, para fixtures de pruebas: tablas, filas y
/// flujos (CAB internos). No instala nada.
pub struct DatabaseBuilder {
    database: Database,
}

impl std::fmt::Debug for DatabaseBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseBuilder").finish_non_exhaustive()
    }
}

impl DatabaseBuilder {
    /// Crea (o sustituye) la base `path`.
    ///
    /// # Errors
    ///
    /// Los de msi.dll.
    pub fn create(path: &Path) -> Result<Self, MsiError> {
        let api = Api::get()?;
        let absolute = std::path::absolute(path)?;
        let path = wide(&absolute.to_string_lossy());
        let mut handle = 0;
        // SAFETY: `path` es UTF-16 terminado en NUL; `MSIDBOPEN_CREATE` es un valor
        // predefinido que la API acepta en lugar de una cadena; `handle` se escribe.
        check(unsafe {
            (api.open_database)(
                PCWSTR(path.as_ptr()),
                PCWSTR(CREATE as *const u16),
                &raw mut handle,
            )
        })?;
        Ok(Self {
            database: Database {
                api,
                handle: Handle { api, value: handle },
            },
        })
    }

    fn open_view(&self, sql: &str) -> Result<Handle, MsiError> {
        let database = &self.database;
        let api = database.api;
        let sql = wide(sql);
        let mut view = Handle { api, value: 0 };
        // SAFETY: base abierta; `sql` es UTF-16 terminado en NUL; `view.value` se escribe.
        check(unsafe {
            (api.open_view)(
                database.handle.value,
                PCWSTR(sql.as_ptr()),
                &raw mut view.value,
            )
        })?;
        Ok(view)
    }

    /// Ejecuta una sentencia SQL (`CREATE TABLE`, `INSERT`) con parámetros de texto.
    ///
    /// # Errors
    ///
    /// Los de msi.dll.
    pub fn execute(&self, sql: &str, parameters: &[&str]) -> Result<(), MsiError> {
        let view = self.open_view(sql)?;
        let record = self.record(parameters, None)?;
        let api = self.database.api;
        // SAFETY: vista y registro válidos durante la llamada.
        check(unsafe { (api.view_execute)(view.value, record.value) })
    }

    /// Añade el flujo `name` con el contenido del archivo `source` a `_Streams`.
    ///
    /// # Errors
    ///
    /// Los de msi.dll.
    pub fn add_stream(&self, name: &str, source: &Path) -> Result<(), MsiError> {
        let view = self.open_view("INSERT INTO `_Streams` (`Name`, `Data`) VALUES (?, ?)")?;
        let record = self.record(&[name], Some(source))?;
        let api = self.database.api;
        // SAFETY: vista y registro válidos durante la llamada.
        check(unsafe { (api.view_execute)(view.value, record.value) })
    }

    fn record(&self, strings: &[&str], stream: Option<&Path>) -> Result<Handle, MsiError> {
        let api = self.database.api;
        let fields = strings.len() + usize::from(stream.is_some());
        let count = u32::try_from(fields.max(1)).map_err(|_| MsiError::Allocation)?;
        // SAFETY: crear un registro no tiene precondiciones.
        let value = unsafe { (api.create_record)(count) };
        if value == 0 {
            return Err(MsiError::Allocation);
        }
        let record = Handle { api, value };
        for (index, text) in strings.iter().enumerate() {
            let field = u32::try_from(index + 1).map_err(|_| MsiError::Allocation)?;
            let text = wide(text);
            // SAFETY: registro válido; `text` es UTF-16 terminado en NUL.
            check(unsafe { (api.set_string)(record.value, field, PCWSTR(text.as_ptr())) })?;
        }
        if let Some(source) = stream {
            let field = u32::try_from(strings.len() + 1).map_err(|_| MsiError::Allocation)?;
            let path = wide(&std::path::absolute(source)?.to_string_lossy());
            // SAFETY: registro válido; `path` es UTF-16 terminado en NUL.
            check(unsafe { (api.set_stream)(record.value, field, PCWSTR(path.as_ptr())) })?;
        }
        Ok(record)
    }

    /// Guarda los cambios y cierra la base.
    ///
    /// # Errors
    ///
    /// Los de msi.dll.
    pub fn commit(self) -> Result<(), MsiError> {
        let database = &self.database;
        // SAFETY: base abierta en modo creación.
        check(unsafe { (database.api.commit)(database.handle.value) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_api_se_carga_desde_system32() {
        assert!(Api::get().is_ok());
    }

    #[test]
    fn una_base_creada_se_lee_con_sus_filas_y_flujos() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("prueba.msi");
        let cabinet = folder.path().join("datos.cab");
        let payload: Vec<u8> = (0..3_000_000_u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&cabinet, &payload).unwrap();
        let builder = DatabaseBuilder::create(&path).unwrap();
        builder
            .execute(
                "CREATE TABLE `Directory` (`Directory` CHAR(72) NOT NULL, `Directory_Parent` CHAR(72), `DefaultDir` CHAR(255) NOT NULL PRIMARY KEY `Directory`)",
                &[],
            )
            .unwrap();
        let insert = "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES (?, ?, ?)";
        builder
            .execute(insert, &["TARGETDIR", "", "SourceDir"])
            .unwrap();
        builder
            .execute(insert, &["APP", "TARGETDIR", "app|Aplicación ñ"])
            .unwrap();
        builder.add_stream("datos.cab", &cabinet).unwrap();
        builder.commit().unwrap();

        let database = Database::open(&path).unwrap();
        let mut rows = database
            .rows(
                "Directory",
                &["Directory", "Directory_Parent", "DefaultDir"],
                10,
            )
            .unwrap();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                vec![
                    "APP".to_owned(),
                    "TARGETDIR".to_owned(),
                    "app|Aplicación ñ".to_owned()
                ],
                vec![
                    "TARGETDIR".to_owned(),
                    String::new(),
                    "SourceDir".to_owned()
                ],
            ]
        );
        assert!(matches!(
            database.rows("Directory", &["Directory"], 1),
            Err(MsiError::TooManyEntries)
        ));
        let mut copy = Vec::new();
        assert_eq!(
            database
                .read_stream("datos.cab", &mut copy, u64::MAX)
                .unwrap(),
            3_000_000
        );
        assert_eq!(copy, payload);
        assert!(matches!(
            database.read_stream("datos.cab", &mut Vec::new(), 1000),
            Err(MsiError::TooLarge)
        ));
        assert!(matches!(
            database.read_stream("otro.cab", &mut Vec::new(), u64::MAX),
            Err(MsiError::MissingStream(name)) if name == "otro.cab"
        ));
    }

    #[test]
    fn un_archivo_que_no_es_msi_da_un_error_de_windows() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("falso.msi");
        std::fs::write(&path, b"no soy una base MSI").unwrap();
        let error = Database::open(&path).unwrap_err();
        assert!(
            matches!(error, MsiError::Status(code) if code != 0),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .starts_with("MSI: Windows database error ")
        );
    }
}
