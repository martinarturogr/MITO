//! Registro local de errores con contexto (`error_tracking.py`, solo la parte local).
//!
//! Contrato: `docs/spec/modules/error_tracking.md` y ADR 0007. MITO no envía informes
//! remotos: no hay `PostHog`, cola `logs/pending` ni `posthog-check`. Lo que se conserva
//! es lo que el original escribe siempre en el log local: un registro `error` con
//! `event_id`, `version`, `runtime`, la lista saneada de excepciones, el componente y
//! la fase. Además, un gancho de pánicos registra los errores no capturados de los
//! hilos (`report_threads`).
//!
//! El log local **no** deduplica: en el original la deduplicación (`_seen`) solo
//! limitaba los envíos remotos.

use std::any::type_name;
use std::error::Error as StdError;
use std::fmt;
use std::panic::{self, Location, PanicHookInfo};
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::diagnostics::{self, LogLevel, StepError};
use crate::layout::Layout;

/// Número máximo de excepciones encadenadas que se registran.
pub const MAX_EXCEPTIONS: usize = 5;

/// Interrupción del usuario (Ctrl+C), el equivalente a `KeyboardInterrupt`: no es un
/// fallo de MITO y [`capture`] no la registra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupted;

impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Interrumpido por el usuario")
    }
}

impl StdError for Interrupted {}

/// Opciones de [`capture`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options<'a> {
    /// `false` para errores no capturados (pánicos): `mechanism.handled`.
    pub handled: bool,
    /// Operación del registro (`error` por defecto, `cli.<comando>` desde la CLI).
    pub operation: &'a str,
    /// Componente afectado; el del error (de [`diagnostics::step`]) tiene prioridad.
    pub component: Option<&'a str>,
    /// Fase de la operación; la del error (de [`diagnostics::step`]) tiene prioridad.
    pub phase: Option<&'a str>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            handled: true,
            operation: "error",
            component: None,
            phase: None,
        }
    }
}

impl<'a> Options<'a> {
    /// Opciones con una operación (`handled = true`, sin componente ni fase).
    pub fn operation(operation: &'a str) -> Self {
        Self {
            operation,
            ..Self::default()
        }
    }
}

/// Origen de una captura: fichero y línea.
#[derive(Debug, Clone, Copy)]
struct Origin<'a> {
    file: &'a str,
    line: u32,
}

impl<'a> From<&'a Location<'_>> for Origin<'a> {
    fn from(location: &'a Location<'_>) -> Self {
        Self {
            file: location.file(),
            line: location.line(),
        }
    }
}

/// Nombre y módulo de un tipo a partir de `std::any::type_name` (sin genéricos).
fn split_type_name(full: &str) -> (String, String) {
    let base = full.split('<').next().unwrap_or(full);
    match base.rsplit_once("::") {
        Some((module, name)) => (name.to_owned(), module.to_owned()),
        None => (base.to_owned(), "builtins".to_owned()),
    }
}

/// Tipo de un error de la cadena. El primero se conoce estáticamente; los demás se
/// reconocen si son de un tipo habitual de MITO.
fn describe(node: &(dyn StdError + 'static), known: Option<&str>) -> (String, String) {
    if let Some(name) = known {
        return split_type_name(name);
    }
    if node.is::<crate::Error>() {
        return split_type_name(type_name::<crate::Error>());
    }
    if node.is::<std::io::Error>() {
        return split_type_name(type_name::<std::io::Error>());
    }
    if node.is::<serde_json::Error>() {
        return split_type_name(type_name::<serde_json::Error>());
    }
    if node.is::<Interrupted>() {
        return split_type_name(type_name::<Interrupted>());
    }
    ("Error".to_owned(), "unknown".to_owned())
}

/// Frame de pila: solo fichero y línea, nunca argumentos ni variables.
///
/// Un fichero del propio workspace (`crates/...`) se guarda con su ruta relativa y
/// `in_app = true`; cualquier otro, solo con su nombre base (no filtra rutas del
/// usuario) y `in_app = false`.
fn frame(origin: Origin<'_>) -> Value {
    let normalized = origin.file.replace('\\', "/");
    let (filename, in_app) = if normalized.starts_with("crates/") {
        (normalized, true)
    } else {
        let base = Path::new(origin.file)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        (base, false)
    };
    json!({
        "platform": "custom",
        "lang": "rust",
        "filename": filename,
        "function": null,
        "lineno": origin.line,
        "in_app": in_app,
    })
}

/// Contexto de fase que `step` adjuntó al error, el más interno (la primera fase gana).
fn step_context(error: &(dyn StdError + 'static)) -> (Option<String>, Option<String>) {
    let (mut phase, mut component) = (None, None);
    let mut current = Some(error);
    while let Some(node) = current {
        if let Some(step) = node.downcast_ref::<StepError>() {
            phase = Some(step.phase().to_owned());
            component = step.component().map(str::to_owned);
        }
        current = node.source();
    }
    (phase, component)
}

/// Lista saneada de excepciones del error y su cadena de causas (`source()`), la más
/// externa primero y como máximo [`MAX_EXCEPTIONS`].
///
/// Cada entrada lleva `type`, `value` (el mensaje saneado), `module`, `mechanism` y
/// `stacktrace`. Solo la primera entrada lleva el frame del lugar de captura; el
/// resto, `frames` vacío. Los envoltorios de [`diagnostics::step`] no cuentan como
/// excepciones.
#[track_caller]
pub fn exception_list<E: StdError + 'static>(
    layout: &Layout,
    error: &E,
    handled: bool,
) -> Vec<Value> {
    let origin = Origin::from(Location::caller());
    build_exceptions(layout, error, Some(type_name::<E>()), handled, Some(origin))
}

fn build_exceptions(
    layout: &Layout,
    error: &(dyn StdError + 'static),
    top_type: Option<&str>,
    handled: bool,
    origin: Option<Origin<'_>>,
) -> Vec<Value> {
    let mut result = Vec::new();
    let mut hops = 0_u32;
    let mut current = Some(error);
    let mut root = true;
    while let Some(node) = current {
        // Una cadena real es un árbol de propietarios; el límite de saltos evita bucles.
        if result.len() >= MAX_EXCEPTIONS || hops > 64 {
            break;
        }
        hops += 1;
        current = node.source();
        let known = if root { top_type } else { None };
        root = false;
        if node.is::<StepError>() {
            continue;
        }
        let (name, module) = describe(node, known);
        // Solo la primera excepción registrada lleva el frame del lugar de captura.
        let frames = match origin {
            Some(origin) if result.is_empty() => vec![frame(origin)],
            _ => Vec::new(),
        };
        result.push(json!({
            "type": name,
            "value": diagnostics::message(layout, node),
            "module": module,
            "mechanism": {"handled": handled, "synthetic": false},
            "stacktrace": {"type": "raw", "frames": frames},
        }));
    }
    result
}

/// Registra `error` en el log local como un registro `error`. Devuelve `true` si se
/// escribió.
///
/// No registra una [`Interrupted`]. Nunca falla. El componente y la fase del error
/// (de [`diagnostics::step`]) tienen prioridad sobre los de `options`.
#[track_caller]
pub fn capture<E: StdError + 'static>(layout: &Layout, error: &E, options: &Options<'_>) -> bool {
    let origin = Origin::from(Location::caller());
    capture_at(layout, error, Some(type_name::<E>()), options, Some(origin))
}

fn capture_at(
    layout: &Layout,
    error: &(dyn StdError + 'static),
    top_type: Option<&str>,
    options: &Options<'_>,
    origin: Option<Origin<'_>>,
) -> bool {
    if error.is::<Interrupted>() {
        return false;
    }
    let (error_phase, error_component) = step_context(error);
    let component = error_component.or_else(|| options.component.map(str::to_owned));
    let phase = error_phase.or_else(|| options.phase.map(str::to_owned));
    let label = |value: Option<String>| {
        value.map_or(Value::Null, |text| {
            Value::String(diagnostics::scrub(layout, &text))
        })
    };
    let mut details: Map<String, Value> = Map::new();
    details.insert("event_id".into(), json!(uuid::Uuid::new_v4().to_string()));
    details.insert("version".into(), json!(env!("CARGO_PKG_VERSION")));
    details.insert("runtime".into(), diagnostics::environment());
    details.insert(
        "exceptions".into(),
        Value::Array(build_exceptions(
            layout,
            error,
            top_type,
            options.handled,
            origin,
        )),
    );
    details.insert("component".into(), label(component));
    details.insert("phase".into(), label(phase));
    let pairs: Vec<(&str, Value)> = details
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    diagnostics::write(
        layout,
        LogLevel::Error,
        options.operation,
        &diagnostics::message(layout, error),
        &pairs,
    )
    .is_some()
}

/// Error sintético de un pánico.
#[derive(Debug)]
struct Panic(String);

impl fmt::Display for Panic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for Panic {}

fn record_panic(layout: &Layout, info: &PanicHookInfo<'_>) {
    let payload = info.payload();
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic sin mensaje".to_owned());
    let options = Options {
        handled: false,
        operation: "thread.unhandled",
        component: None,
        phase: None,
    };
    let origin = info.location().map(Origin::from);
    capture_at(
        layout,
        &Panic(text),
        Some("std::panic::Panic"),
        &options,
        origin,
    );
}

/// Instala un gancho de pánicos que registra cada pánico en el log local
/// (`operation = "thread.unhandled"`, `handled = false`) y después invoca el gancho
/// anterior. Equivale a `report_threads` del original.
pub fn install_panic_hook(layout: Layout) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        record_panic(&layout, info);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::message::Message;
    use std::fs;
    use std::io;

    #[derive(Debug, thiserror::Error)]
    #[error("also secret")]
    struct Runtime(#[source] io::Error);

    #[derive(Debug, thiserror::Error)]
    #[error("nivel {0}")]
    struct Level(u8, #[source] Option<Box<Level>>);

    fn instance() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        (dir, layout)
    }

    fn private_error() -> io::Error {
        io::Error::other("password=private C:/Users/private/file.txt")
    }

    fn last_record(layout: &Layout) -> Value {
        let text = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
        serde_json::from_str(text.lines().last().unwrap()).unwrap()
    }

    #[test]
    fn la_cadena_de_causas_sale_de_mas_externa_a_mas_interna() {
        let (_dir, layout) = instance();
        let error = Runtime(io::Error::other("secret"));
        let items = exception_list(&layout, &error, false);
        let types: Vec<_> = items.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["Runtime", "Error"]);
        assert_eq!(items[0]["module"], "mito_core::error_tracking::tests");
        assert!(items[1]["module"].as_str().unwrap().ends_with("io::error"));
        assert_eq!(
            items[0]["mechanism"],
            json!({"handled": false, "synthetic": false})
        );
        assert_eq!(items[0]["value"], "also secret");
        assert_eq!(items[1]["value"], "secret");
        assert_eq!(items[1]["stacktrace"]["frames"], json!([]));
    }

    #[test]
    fn una_interrupcion_no_se_registra() {
        let (_dir, layout) = instance();
        assert!(!capture(&layout, &Interrupted, &Options::default()));
        assert!(!layout.logs().join("mito.log").exists());
    }

    #[test]
    fn solo_se_registran_cinco_excepciones() {
        let (_dir, layout) = instance();
        let chain = (0..8).fold(None, |inner, n| Some(Box::new(Level(n, inner))));
        let items = exception_list(&layout, &*chain.unwrap(), true);
        assert_eq!(items.len(), MAX_EXCEPTIONS);
        assert_eq!(items[0]["value"], "nivel 7");
        assert_eq!(items[4]["value"], "nivel 3");
    }

    #[test]
    fn la_traza_es_privada_y_el_ultimo_frame_es_el_lugar_de_captura() {
        let (dir, layout) = instance();
        assert!(capture(&layout, &private_error(), &Options::default()));
        let text = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
        assert!(!text.contains("private"), "{text}");
        assert!(!text.contains(&*dir.path().to_string_lossy()));
        let record = last_record(&layout);
        let exception = &record["exceptions"][0];
        assert_eq!(exception["type"], "Error");
        assert!(exception["module"].as_str().unwrap().ends_with("io::error"));
        let frame = &exception["stacktrace"]["frames"][0];
        assert_eq!(frame["filename"], "crates/mito-core/src/error_tracking.rs");
        assert_eq!(frame["lang"], "rust");
        assert_eq!(frame["in_app"], true);
        assert!(frame["lineno"].as_u64().unwrap() > 0);
    }

    #[test]
    fn un_fichero_ajeno_al_workspace_solo_conserva_el_nombre_base() {
        let value = frame(Origin {
            file: "C:\\Users\\persona\\.cargo\\registry\\src\\x\\lib.rs",
            line: 3,
        });
        assert_eq!(value["filename"], "lib.rs");
        assert_eq!(value["in_app"], false);
        let own = frame(Origin {
            file: "crates\\mito-core\\src\\a.rs",
            line: 1,
        });
        assert_eq!(own["filename"], "crates/mito-core/src/a.rs");
    }

    #[test]
    fn el_registro_local_existe_sin_cola_pendiente() {
        let (_dir, layout) = instance();
        assert!(capture(
            &layout,
            &private_error(),
            &Options::operation("install")
        ));
        let record = last_record(&layout);
        assert_eq!(record["operation"], "install");
        assert_eq!(record["level"], "error");
        assert_eq!(record["exceptions"][0]["type"], "Error");
        assert_eq!(record["component"], Value::Null);
        assert_eq!(record["phase"], Value::Null);
        assert_eq!(record["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(record["runtime"]["platform"], "win32");
        assert_eq!(record["event_id"].as_str().unwrap().len(), 36);
        assert!(!layout.logs().join("pending").exists());
        let keys: Vec<_> = record.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "time",
                "level",
                "pid",
                "operation",
                "message",
                "event_id",
                "version",
                "runtime",
                "exceptions",
                "component",
                "phase"
            ]
        );
    }

    #[test]
    fn el_valor_de_la_excepcion_coincide_con_el_mensaje_del_registro() {
        let (_dir, layout) = instance();
        capture(&layout, &private_error(), &Options::default());
        let record = last_record(&layout);
        assert_eq!(record["exceptions"][0]["value"], record["message"]);
        assert_eq!(record["message"], "password=[redacted] <USER>/file.txt");
    }

    #[test]
    fn la_cli_registra_con_la_operacion_cli_comando() {
        let (_dir, layout) = instance();
        let error = Error::domain(Message::new("failed"));
        assert!(capture(
            &layout,
            &error,
            &Options::operation("cli.installed")
        ));
        let record = last_record(&layout);
        assert_eq!(record["operation"], "cli.installed");
        assert_eq!(record["message"], "failed");
        assert_eq!(record["exceptions"][0]["type"], "Error");
        assert_eq!(record["exceptions"][0]["module"], "mito_core::error");
    }

    #[test]
    fn el_contexto_de_step_llega_al_registro_y_prevalece_sobre_las_opciones() {
        let (_dir, layout) = instance();
        let error = diagnostics::step(&layout, "install", "extract", Some("main/example"), || {
            Err::<(), _>(io::Error::other(r#"Cannot extract: password="two words""#))
        })
        .unwrap_err();
        let options = Options {
            handled: true,
            operation: "ui.install",
            component: Some("otro"),
            phase: Some("otra"),
        };
        assert!(capture(&layout, &error, &options));
        let text = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
        assert!(!text.contains("two words"));
        let record = last_record(&layout);
        assert_eq!(record["phase"], "extract");
        assert_eq!(record["component"], "main/example");
        assert!(
            record["message"]
                .as_str()
                .unwrap()
                .contains("Cannot extract")
        );
        assert_eq!(record["exceptions"].as_array().unwrap().len(), 1);
        assert_eq!(record["exceptions"][0]["type"], "Error");
    }

    #[test]
    fn la_primera_fase_gana_en_steps_anidados() {
        let (_dir, layout) = instance();
        let error = diagnostics::step(&layout, "install", "externa", Some("a"), || {
            diagnostics::step(&layout, "install", "interna", Some("b"), || {
                Err::<(), _>(io::Error::other("fallo"))
            })
        })
        .unwrap_err();
        capture(&layout, &error, &Options::default());
        let record = last_record(&layout);
        assert_eq!(record["phase"], "interna");
        assert_eq!(record["component"], "b");
    }

    #[test]
    fn sin_contexto_en_el_error_se_usan_las_opciones_saneadas() {
        let (_dir, layout) = instance();
        let options = Options {
            handled: true,
            operation: "x",
            component: Some("https://u:p@h/x?t=1"),
            phase: Some("fase"),
        };
        capture(&layout, &io::Error::other("e"), &options);
        let record = last_record(&layout);
        assert_eq!(record["component"], "https://h/x?[redacted]");
        assert_eq!(record["phase"], "fase");
    }
}
