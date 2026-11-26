//! Capa de `tracing` que escribe los eventos en `logs/mito.log`.
//!
//! Las librerías de MITO no conocen el log: emiten eventos `tracing` con un campo
//! `operation` y detalles, por ejemplo
//! `tracing::warn!(operation = "download.hash", host = %host, "Hash mismatch")`.
//! [`DiagnosticsLayer`] convierte cada evento de nivel `INFO` o superior en una línea
//! del log con el mismo formato que [`write`](fn@super::write): el mensaje del evento es
//! `message` y el resto de campos son detalles planos, en orden.

use serde_json::{Number, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::util::{SubscriberInitExt as _, TryInitError};
use tracing_subscriber::{Layer, registry};

use super::{LogLevel, write};
use crate::layout::Layout;

/// Capa que escribe los eventos `tracing` en el log local de una instancia.
#[derive(Debug, Clone)]
pub struct DiagnosticsLayer {
    layout: Layout,
}

impl DiagnosticsLayer {
    /// Crea la capa para la instancia `layout`.
    pub fn new(layout: Layout) -> Self {
        Self { layout }
    }
}

/// Instala la capa como suscriptor global de `tracing`.
///
/// # Errors
///
/// [`TryInitError`] si ya hay un suscriptor global.
pub fn init(layout: Layout) -> Result<(), TryInitError> {
    registry().with(DiagnosticsLayer::new(layout)).try_init()
}

/// Recoge los campos de un evento.
#[derive(Default)]
struct Fields {
    operation: Option<String>,
    message: String,
    details: Vec<(String, Value)>,
}

impl Fields {
    fn put(&mut self, field: &Field, value: Value) {
        match field.name() {
            "operation" => {
                self.operation = Some(match value {
                    Value::String(text) => text,
                    other => other.to_string(),
                });
            }
            "message" => {
                self.message = match value {
                    Value::String(text) => text,
                    other => other.to_string(),
                };
            }
            // Campos internos del puente con el crate `log`.
            name if name.starts_with("log.") => {}
            name => self.details.push((name.to_owned(), value)),
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::String(value.to_owned()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::Bool(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::Number(value.into()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::Number(value.into()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let value =
            Number::from_f64(value).map_or_else(|| Value::String(value.to_string()), Value::Number);
        self.put(field, value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.put(field, Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::String(format!("{value:?}")));
    }
}

fn level_of(level: Level) -> Option<LogLevel> {
    match level {
        Level::ERROR => Some(LogLevel::Error),
        Level::WARN => Some(LogLevel::Warning),
        Level::INFO => Some(LogLevel::Info),
        // El log local solo conoce info, warning y error.
        Level::DEBUG | Level::TRACE => None,
    }
}

impl<S: Subscriber> Layer<S> for DiagnosticsLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let Some(level) = level_of(*event.metadata().level()) else {
            return;
        };
        let mut fields = Fields::default();
        event.record(&mut fields);
        let operation = fields
            .operation
            .unwrap_or_else(|| event.metadata().target().to_owned());
        let details: Vec<(&str, Value)> = fields
            .details
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect();
        write(&self.layout, level, &operation, &fields.message, &details);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn records(layout: &Layout) -> Vec<Value> {
        fs::read_to_string(layout.logs().join("mito.log"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn with_layer(layout: &Layout, body: impl FnOnce()) {
        let subscriber = registry().with(DiagnosticsLayer::new(layout.clone()));
        tracing::subscriber::with_default(subscriber, body);
    }

    #[test]
    fn un_evento_de_hash_se_convierte_en_una_linea_del_log() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let host = "https://u:p@example.com/x";
        with_layer(&layout, || {
            tracing::warn!(operation = "download.hash", host = %host, "Hash mismatch");
        });
        let text = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
        let record = &records(&layout)[0];
        assert_eq!(record["level"], "warning");
        assert_eq!(record["operation"], "download.hash");
        assert_eq!(record["message"], "Hash mismatch");
        assert_eq!(record["host"], "https://example.com/x");
        assert!(
            text.contains(r#""operation": "download.hash", "message": "Hash mismatch", "host": "#)
        );
    }

    #[test]
    fn los_detalles_conservan_tipos_y_orden() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        with_layer(&layout, || {
            tracing::error!(
                operation = "install",
                size = 5_u64,
                ok = false,
                delta = -3_i64,
                ratio = 0.5_f64,
                "Falló"
            );
            tracing::info!("sin operación");
            tracing::debug!(operation = "ignorado", "no se escribe");
            tracing::trace!("tampoco");
        });
        let all = records(&layout);
        assert_eq!(all.len(), 2);
        let keys: Vec<_> = all[0].as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "time",
                "level",
                "pid",
                "operation",
                "message",
                "size",
                "ok",
                "delta",
                "ratio"
            ]
        );
        assert_eq!(all[0]["level"], "error");
        assert_eq!(all[0]["size"], 5);
        assert_eq!(all[0]["ok"], false);
        assert_eq!(all[0]["delta"], -3);
        assert_eq!(all[0]["ratio"], 0.5);
        assert_eq!(all[1]["level"], "info");
        assert!(all[1]["operation"].as_str().unwrap().contains("layer"));
        assert_eq!(all[1]["message"], "sin operación");
    }

    #[test]
    fn el_mensaje_con_argumentos_y_los_errores_se_formatean() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let error = std::io::Error::other("fallo password=SECRET");
        with_layer(&layout, || {
            tracing::warn!(
                operation = "x",
                error = &error as &dyn std::error::Error,
                "Fallo {}",
                7
            );
        });
        let record = &records(&layout)[0];
        assert_eq!(record["message"], "Fallo 7");
        assert_eq!(record["error"], "fallo password=[redacted]");
    }
}
