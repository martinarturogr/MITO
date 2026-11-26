//! Log local `logs/mito.log` (`diagnostics.py` del original).
//!
//! Contrato: `docs/spec/modules/diagnostics.md` y ADR 0007/0008. Cada registro es un
//! objeto JSON en una línea con el formato exacto de `json.dumps(ensure_ascii=True)`.
//! El archivo se rota a 1 MiB con tres copias (`.1` a `.3`), se protege con un lock
//! compartido entre procesos y, si la carpeta de la instancia no es escribible, se usa
//! `%TEMP%\MITO-logs\<hash-de-la-raíz>`.
//!
//! Hay dos formas de escribir: directamente con [`write()`](fn@write), o emitiendo eventos
//! `tracing` con un campo `operation` que recoge [`DiagnosticsLayer`]. Escribir el log
//! nunca falla hacia el llamador.

mod json;
mod layer;
mod scrub;

use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::layout::Layout;

pub use json::{iso_utc, python_dumps};
pub use layer::{DiagnosticsLayer, init};
pub use scrub::{MAX_CHARS, scrub};

/// Tamaño máximo de `mito.log` antes de rotar (1 MiB).
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Copias rotadas que se conservan (`mito.log.1` a `mito.log.3`).
pub const BACKUPS: u32 = 3;
/// Un registro mayor que esto pierde `exceptions` y gana `truncated: true`.
const MAX_RECORD_BYTES: usize = 128 * 1024;
/// Intentos y espera del lock de carpeta (unos 200 ms en total).
const LOCK_ATTEMPTS: u32 = 10;
const LOCK_WAIT: Duration = Duration::from_millis(20);

/// Nivel de un registro del log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// `info`.
    Info,
    /// `warning`.
    Warning,
    /// `error`.
    Error,
}

impl LogLevel {
    /// Texto del campo `level` del registro.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Carpetas de log por orden de preferencia: `<raíz>\logs` y
/// `%TEMP%\MITO-logs\<identidad>`, con `identidad` los 16 primeros caracteres
/// hexadecimales del SHA-256 de la raíz resuelta y en minúsculas.
pub fn locations(layout: &Layout) -> [PathBuf; 2] {
    let identity = hex_prefix(&Sha256::digest(
        scrub::resolved_text(layout.root())
            .to_lowercase()
            .as_bytes(),
    ));
    [
        layout.logs(),
        std::env::temp_dir().join("MITO-logs").join(identity),
    ]
}

/// Primeros 16 caracteres hexadecimales de un resumen.
fn hex_prefix(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    digest.iter().take(8).fold(String::new(), |mut out, byte| {
        // Escribir en un String no puede fallar.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Texto seguro para el log de un error: su mensaje saneado.
pub fn message(layout: &Layout, error: &dyn StdError) -> String {
    scrub(layout, &error.to_string())
}

/// Mensaje de un proceso hijo que terminó con error. Nunca incluye sus argumentos.
pub fn child_failed_message(exit_code: i32) -> String {
    format!("Child process failed with exit code {exit_code}")
}

/// Mensaje de un proceso hijo que superó su tiempo. Nunca incluye sus argumentos.
pub fn child_timeout_message(seconds: f64) -> String {
    // Python imprime los reales con `repr`: `5.0`, no `5`.
    if seconds.fract() == 0.0 && seconds.is_finite() {
        format!("Child process exceeded timeout of {seconds:.1} seconds")
    } else {
        format!("Child process exceeded timeout of {seconds} seconds")
    }
}

/// Datos del entorno de ejecución que acompañan a los errores: `rust` (versión mínima
/// de Rust con la que se compila), `bits`, `platform` y `windows` (`mayor.menor.build`).
pub fn environment() -> Value {
    let mut runtime = Map::new();
    runtime.insert("rust".into(), json!(env!("CARGO_PKG_RUST_VERSION")));
    runtime.insert("bits".into(), json!(usize::BITS));
    runtime.insert("platform".into(), json!("win32"));
    if let Some((major, minor, build)) = mito_win::system::windows_version() {
        runtime.insert("windows".into(), json!(format!("{major}.{minor}.{build}")));
    }
    Value::Object(runtime)
}

/// Escribe un registro en `logs/mito.log` (o en la ubicación alternativa).
///
/// `details` son etiquetas explícitas que se añaden como claves planas tras `message`,
/// en orden; nunca se vuelcan argumentos, entorno ni variables locales. Las cadenas se
/// sanean salvo en `exceptions` y `runtime`, que ya vienen saneados.
///
/// Devuelve la ruta del log escrito, o `None` si no se pudo escribir en ninguna
/// carpeta. **Nunca falla**: un error de log no debe sustituir al error original.
pub fn write(
    layout: &Layout,
    level: LogLevel,
    operation: &str,
    text: &str,
    details: &[(&str, Value)],
) -> Option<PathBuf> {
    let data = encode(layout, level, operation, text, details);
    append_to_first_writable(&locations(layout), MAX_BYTES, &data)
}

/// Construye la línea JSON (con `\n` final) de un registro.
fn encode(
    layout: &Layout,
    level: LogLevel,
    operation: &str,
    text: &str,
    details: &[(&str, Value)],
) -> Vec<u8> {
    let mut record = Map::new();
    record.insert("time".into(), json!(iso_utc(SystemTime::now())));
    record.insert("level".into(), json!(level.as_str()));
    record.insert("pid".into(), json!(std::process::id()));
    record.insert("operation".into(), json!(scrub(layout, operation)));
    record.insert("message".into(), json!(scrub(layout, text)));
    for (key, value) in details {
        if matches!(*key, "time" | "level" | "pid" | "operation" | "message") {
            continue;
        }
        let value = match value {
            Value::String(text) if !matches!(*key, "exceptions" | "runtime") => {
                Value::String(scrub(layout, text))
            }
            other => other.clone(),
        };
        record.insert((*key).to_owned(), value);
    }
    let mut data = line(&record);
    if data.len() > MAX_RECORD_BYTES {
        let mut reduced: Map<String, Value> = record
            .into_iter()
            .filter(|(key, _)| key != "exceptions")
            .collect();
        reduced.insert("truncated".into(), Value::Bool(true));
        data = line(&reduced);
    }
    data
}

fn line(record: &Map<String, Value>) -> Vec<u8> {
    let mut text = python_dumps(&Value::Object(record.clone()));
    text.push('\n');
    text.into_bytes()
}

/// Mensajes que se muestran una sola vez por proceso.
fn warned() -> MutexGuard<'static, HashSet<String>> {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    WARNED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn warn_once(key: &str, message: &str) {
    if warned().insert(key.to_owned()) {
        // Un fallo al avisar por stderr no debe afectar a nadie.
        let _ = writeln!(io::stderr(), "{message}");
    }
}

/// Añade `data` al primer `mito.log` escribible de `folders`, rotando si hace falta.
fn append_to_first_writable(folders: &[PathBuf], max_bytes: u64, data: &[u8]) -> Option<PathBuf> {
    for folder in folders {
        if let Ok(target) = append_in(folder, max_bytes, data) {
            if folders.first() != Some(folder) {
                warn_once(
                    &folder.to_string_lossy(),
                    &format!("MITO: log alternativo: {}", folder.display()),
                );
            }
            return Some(target);
        }
    }
    warn_once(
        "unwritable",
        "MITO: no se pudo escribir el log local ni el alternativo.",
    );
    None
}

fn append_in(folder: &Path, max_bytes: u64, data: &[u8]) -> io::Result<PathBuf> {
    guard(folder, || {
        let target = folder.join("mito.log");
        let size = fs::metadata(&target).map(|m| m.len());
        if size.is_ok_and(|size| size + data.len() as u64 > max_bytes) {
            rotate(folder, &target)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)?
            .write_all(data)?;
        Ok(target)
    })
}

/// `mito.log` pasa a `.1`, `.1` a `.2`, `.2` a `.3`, y se descarta el `.3` anterior.
fn rotate(folder: &Path, target: &Path) -> io::Result<()> {
    match fs::remove_file(folder.join(format!("mito.log.{BACKUPS}"))) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    for index in (1..BACKUPS).rev() {
        let previous = folder.join(format!("mito.log.{index}"));
        if previous.exists() {
            fs::rename(&previous, folder.join(format!("mito.log.{}", index + 1)))?;
        }
    }
    fs::rename(target, folder.join("mito.log.1"))
}

/// Exclusión mutua de la carpeta de log: un mutex de proceso y el lock de
/// `.diagnostics.lock` entre procesos (10 intentos de 20 ms).
fn guard<T>(folder: &Path, body: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    static PROCESS: Mutex<()> = Mutex::new(());
    let _process = PROCESS.lock().unwrap_or_else(PoisonError::into_inner);
    fs::create_dir_all(folder)?;
    let mut lock: File = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(folder.join(".diagnostics.lock"))?;
    if lock.metadata()?.len() == 0 {
        lock.write_all(&[0])?;
        lock.flush()?;
    }
    for attempt in 0..LOCK_ATTEMPTS {
        match lock.try_lock() {
            Ok(()) => {
                let result = body();
                let _ = lock.unlock();
                return result;
            }
            Err(TryLockError::WouldBlock) if attempt + 1 < LOCK_ATTEMPTS => {
                thread::sleep(LOCK_WAIT);
            }
            Err(TryLockError::WouldBlock) => break,
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "Diagnostic lock busy",
    ))
}

/// Error de una fase ejecutada con [`step`]: el error original más la fase y el
/// componente que estaban activos. Gana la primera fase (la más interna).
#[derive(Debug)]
pub struct StepError {
    phase: String,
    component: Option<String>,
    source: Box<dyn StdError + Send + Sync>,
}

impl StepError {
    /// Fase en la que falló.
    pub fn phase(&self) -> &str {
        &self.phase
    }

    /// Componente que se procesaba, si había uno.
    pub fn component(&self) -> Option<&str> {
        self.component.as_deref()
    }

    /// Error original, sin el contexto.
    pub fn into_source(self) -> Box<dyn StdError + Send + Sync> {
        self.source
    }
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}

impl StdError for StepError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.source)
    }
}

/// Ejecuta `body` como una fase de `operation`: escribe de inmediato un registro `info`
/// con la fase como mensaje y, si `body` falla, devuelve el error con la fase y el
/// componente adjuntos (solo etiquetas explícitas, nunca argumentos ni variables).
///
/// # Errors
///
/// El error de `body` envuelto en [`StepError`].
pub fn step<T, E>(
    layout: &Layout,
    operation: &str,
    phase: &str,
    component: Option<&str>,
    body: impl FnOnce() -> Result<T, E>,
) -> Result<T, StepError>
where
    E: StdError + Send + Sync + 'static,
{
    let scrubbed = component.map(|c| Value::String(scrub(layout, c)));
    write(
        layout,
        LogLevel::Info,
        operation,
        phase,
        &[("component", scrubbed.unwrap_or(Value::Null))],
    );
    body().map_err(|source| StepError {
        phase: phase.to_owned(),
        component: component.map(str::to_owned),
        source: Box::new(source),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn lines(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn write_plain(layout: &Layout, text: &str) -> Option<PathBuf> {
        write(layout, LogLevel::Error, "test", text, &[])
    }

    #[test]
    fn la_rotacion_conserva_tres_copias_y_lineas_json_validas() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let folders = [layout.logs(), root.path().join("alt")];
        for index in 0..12 {
            let data = encode(
                &layout,
                LogLevel::Error,
                "test",
                &format!("{index}{}", "x".repeat(250)),
                &[],
            );
            append_to_first_writable(&folders, 600, &data).unwrap();
        }
        let files: Vec<_> = fs::read_dir(layout.logs())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("mito.log")
            })
            .collect();
        assert_eq!(files.len(), 4);
        for path in &files {
            assert!(fs::metadata(path).unwrap().len() <= 600);
            for record in lines(path) {
                assert_eq!(record["operation"], "test");
            }
        }
        let last = lines(&layout.logs().join("mito.log"));
        assert!(
            last.last().unwrap()["message"]
                .as_str()
                .unwrap()
                .starts_with("11")
        );
    }

    #[test]
    fn escritores_concurrentes_no_intercalan_registros() {
        let root = tempfile::tempdir().unwrap();
        let layout = Arc::new(Layout::new(root.path()));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let layout = Arc::clone(&layout);
                thread::spawn(move || {
                    for _ in 0..20 {
                        write(&layout, LogLevel::Error, "thread", "entry", &[]);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let records = lines(&layout.logs().join("mito.log"));
        assert_eq!(records.len(), 80);
        assert!(records.iter().all(|r| r["message"] == "entry"));
    }

    #[test]
    fn la_ubicacion_alternativa_y_el_fallo_total_no_lanzan() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let blocked = root.path().join("blocked");
        fs::write(&blocked, "file, not directory").unwrap();
        let fallback = root.path().join("fallback");
        let data = encode(&layout, LogLevel::Error, "test", "message", &[]);
        assert_eq!(
            append_to_first_writable(&[blocked.clone(), fallback.clone()], MAX_BYTES, &data),
            Some(fallback.join("mito.log"))
        );
        assert_eq!(
            append_to_first_writable(&[blocked.clone(), blocked], MAX_BYTES, &data),
            None
        );
    }

    #[test]
    fn la_ubicacion_alternativa_usa_el_hash_de_la_raiz_en_minusculas() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let [primary, alternative] = locations(&layout);
        assert_eq!(primary, root.path().join("logs"));
        let resolved = dunce::canonicalize(root.path()).unwrap();
        let expected = hex_prefix(&Sha256::digest(
            resolved.to_string_lossy().to_lowercase().as_bytes(),
        ));
        assert_eq!(expected.len(), 16);
        assert_eq!(
            alternative,
            std::env::temp_dir().join("MITO-logs").join(&expected)
        );
        let upper = Layout::new(root.path().to_string_lossy().to_uppercase());
        assert_eq!(locations(&upper)[1], alternative);
    }

    #[test]
    fn hash_de_referencia() {
        // sha256("c:\\mito")[:16] calculado con sha256sum (equivale a hashlib).
        assert_eq!(hex_prefix(&Sha256::digest(b"c:\\mito")), "41298808652d1a4d");
    }

    #[test]
    fn el_formato_de_linea_es_el_del_original() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let path = write(
            &layout,
            LogLevel::Warning,
            "download.hash",
            "Cannot extract: password=SECRET ñ",
            &[
                ("component", Value::Null),
                ("phase", json!("extract")),
                ("pending", json!(true)),
            ],
        )
        .unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.ends_with("}\n"));
        let prefix = r#"{"time": ""#;
        assert!(text.starts_with(prefix), "{text}");
        let pid = std::process::id();
        let after_time = &text[text.find(r#"", "level""#).unwrap()..];
        assert_eq!(
            after_time,
            format!(
                r#"", "level": "warning", "pid": {pid}, "operation": "download.hash", "message": "Cannot extract: password=[redacted] \u00f1", "component": null, "phase": "extract", "pending": true}}"#
            ) + "\n"
        );
    }

    #[test]
    fn los_detalles_se_sanean_salvo_exceptions_y_runtime() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let path = write(
            &layout,
            LogLevel::Info,
            "t",
            "m",
            &[
                ("host", json!("https://u:p@example.com/x?token=1")),
                ("runtime", json!("C:/Users/Someone/x")),
                ("pid", json!(0)),
            ],
        )
        .unwrap();
        let record = &lines(&path)[0];
        assert_eq!(record["host"], "https://example.com/x?[redacted]");
        assert_eq!(record["runtime"], "C:/Users/Someone/x");
        assert_ne!(record["pid"], 0);
    }

    #[test]
    fn un_registro_enorme_pierde_exceptions_y_gana_truncated() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let huge = json!([{"value": "x".repeat(200_000)}]);
        let path = write(
            &layout,
            LogLevel::Error,
            "t",
            "m",
            &[("exceptions", huge), ("component", Value::Null)],
        )
        .unwrap();
        let record = &lines(&path)[0];
        assert!(record.get("exceptions").is_none());
        assert_eq!(record["truncated"], true);
        let keys: Vec<_> = record.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "time",
                "level",
                "pid",
                "operation",
                "message",
                "component",
                "truncated"
            ]
        );
    }

    #[test]
    fn el_mensaje_de_un_proceso_hijo_no_incluye_argumentos() {
        assert_eq!(
            child_failed_message(7),
            "Child process failed with exit code 7"
        );
        assert_eq!(
            child_timeout_message(5.0),
            "Child process exceeded timeout of 5.0 seconds"
        );
        assert_eq!(
            child_timeout_message(2.5),
            "Child process exceeded timeout of 2.5 seconds"
        );
    }

    #[test]
    fn el_mensaje_de_un_error_se_sanea() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let error = io::Error::other("fixture password=SECRET");
        assert_eq!(message(&layout, &error), "fixture password=[redacted]");
    }

    #[test]
    fn step_registra_la_fase_y_etiqueta_el_error() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let ok: Result<u8, StepError> = step(&layout, "install", "download", None, || {
            Ok::<_, io::Error>(1)
        });
        assert_eq!(ok.unwrap(), 1);
        let failed = step(&layout, "install", "extract", Some("main/example"), || {
            Err::<(), _>(io::Error::other("Cannot extract"))
        })
        .unwrap_err();
        assert_eq!(failed.phase(), "extract");
        assert_eq!(failed.component(), Some("main/example"));
        assert_eq!(failed.to_string(), "Cannot extract");
        let records = lines(&layout.logs().join("mito.log"));
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["message"], "download");
        assert_eq!(records[0]["component"], Value::Null);
        assert_eq!(records[1]["message"], "extract");
        assert_eq!(records[1]["component"], "main/example");
        assert_eq!(records[1]["level"], "info");
        assert!(write_plain(&layout, "x").is_some());
    }

    #[test]
    fn el_entorno_describe_la_plataforma() {
        let runtime = environment();
        assert_eq!(runtime["platform"], "win32");
        assert_eq!(runtime["bits"], 64);
        assert_eq!(runtime["windows"].as_str().unwrap().split('.').count(), 3);
    }
}
