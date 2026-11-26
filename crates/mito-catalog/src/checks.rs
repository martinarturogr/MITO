//! Comprobaciones diarias persistentes (`update_checks.daily`) y filtros de búsqueda por
//! bucket (`catalog-filters.json`).
//!
//! Contrato: `docs/spec/modules/buckets.md`, secciones 5 y 6.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use mito_core::storage::{inside, read_json, write_json};
use mito_core::{Layout, Message, MutationLock};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::buckets::read_json_value;
use crate::error::{Result, bail};
use crate::pyjson::{DumpOptions, dumps, sha256_hex};

/// Lock que protege el registro de las comprobaciones diarias.
const CHECKS_LOCK: &str = "update-checks.lock";

/// Espera máxima del lock: componentes y MITO se comprueban en paralelo al abrir la UI y
/// el lock solo cubre la lectura y escritura del registro.
const CHECKS_WAIT: Duration = Duration::from_secs(15);

/// Ejecuta `operation` como máximo una vez por día local y por `key`, salvo `force`.
///
/// Devuelve el resultado de la operación o, si hoy ya se comprobó la misma clave, el
/// resultado guardado (`None` si la comprobación falló, sigue en curso o no produjo
/// resultado). Registra `core/state/<name>-check.json` con `{day, key, token, result}`
/// **antes** de la operación, de modo que un error o un cierre no se reintentan hasta el
/// día siguiente; la operación se ejecuta fuera del lock. La identidad guardada es el
/// SHA-256 de `json.dumps(key, sort_keys=True)` (separadores por defecto de Python).
///
/// # Errors
///
/// El error de `operation` (el registro queda con `result: null`), el lock ocupado más
/// de 15 s y los de E/S o JSON del registro.
pub fn daily<T>(
    layout: &Layout,
    name: &str,
    key: &Value,
    force: bool,
    operation: impl FnOnce() -> Result<T>,
) -> Result<Option<T>>
where
    T: Serialize + DeserializeOwned,
{
    let day = jiff::ZonedDateTime::now().date().to_string();
    daily_on(layout, name, key, force, &day, operation)
}

/// Como [`daily`] para un día concreto (`AAAA-MM-DD`), útil en pruebas.
///
/// # Errors
///
/// Los de [`daily`].
pub fn daily_on<T>(
    layout: &Layout,
    name: &str,
    key: &Value,
    force: bool,
    day: &str,
    operation: impl FnOnce() -> Result<T>,
) -> Result<Option<T>>
where
    T: Serialize + DeserializeOwned,
{
    let identity = sha256_hex(dumps(key, DumpOptions::DEFAULT_SORTED).as_bytes());
    let path = inside(layout.root(), &format!("core/state/{name}-check.json"))?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let record = json!({"day": day, "key": identity, "token": token, "result": null});
    {
        let _lock = MutationLock::acquire(layout, CHECKS_LOCK, CHECKS_WAIT)?;
        let previous = read_json::<Value>(&path).unwrap_or(Value::Null);
        if !force
            && previous.get("day").and_then(Value::as_str) == Some(day)
            && previous.get("key").and_then(Value::as_str) == Some(&identity)
        {
            let cached = previous
                .get("result")
                .filter(|result| !result.is_null())
                .and_then(|result| serde_json::from_value(result.clone()).ok());
            return Ok(cached);
        }
        // Se guarda antes de la red: los errores y cierres no se reintentan hoy.
        write_json(&path, &record)?;
    }
    let result = operation()?;
    {
        let _lock = MutationLock::acquire(layout, CHECKS_LOCK, CHECKS_WAIT)?;
        let current: Value = read_json(&path)?;
        if current.get("token").and_then(Value::as_str) == Some(&token) {
            let mut finished = record;
            finished["result"] =
                serde_json::to_value(&result).map_err(|source| mito_core::Error::Json {
                    path: path.clone(),
                    source,
                })?;
            write_json(&path, &finished)?;
        }
    }
    Ok(Some(result))
}

/// Ruta de `core/state/catalog-filters.json`.
pub fn filters_path(layout: &Layout) -> PathBuf {
    layout.state().join("catalog-filters.json")
}

/// Buckets ocultos en la búsqueda de la UI. Sin archivo, ninguno.
///
/// # Errors
///
/// `Filtros de catálogo no válidos: …` si el archivo no es una lista de nombres (el
/// original lanza una excepción al abrir la UI) y los de E/S o JSON.
pub fn read_disabled_buckets(layout: &Layout) -> Result<BTreeSet<String>> {
    let path = filters_path(layout);
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let invalid =
        || bail(Message::new("Filtros de catálogo no válidos: {p1}").arg("p1", path.display()));
    let Value::Array(items) = read_json_value(&path)? else {
        return invalid();
    };
    let mut names = BTreeSet::new();
    for item in items {
        let Value::String(name) = item else {
            return invalid();
        };
        names.insert(name);
    }
    Ok(names)
}

/// Guarda la lista ordenada de buckets ocultos. Como el original, sin lock de mutación.
///
/// # Errors
///
/// E/S del núcleo.
pub fn write_disabled_buckets(layout: &Layout, disabled: &BTreeSet<String>) -> Result<()> {
    let names: Vec<&String> = disabled.iter().collect();
    write_json(&filters_path(layout), &names)?;
    Ok(())
}

/// Marca un bucket como visible (`enabled`) u oculto y guarda el filtro. Devuelve el
/// conjunto resultante.
///
/// # Errors
///
/// Los de [`read_disabled_buckets`] y [`write_disabled_buckets`].
pub fn set_bucket_enabled(
    layout: &Layout,
    bucket: &str,
    enabled: bool,
) -> Result<BTreeSet<String>> {
    let mut disabled = read_disabled_buckets(layout)?;
    if enabled {
        disabled.remove(bucket);
    } else {
        disabled.insert(bucket.to_owned());
    }
    write_disabled_buckets(layout, &disabled)?;
    Ok(disabled)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use super::*;
    use crate::error::Error;

    fn layout() -> (tempfile::TempDir, Layout) {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        (root, layout)
    }

    fn record(layout: &Layout, name: &str) -> Value {
        let path = layout.state().join(format!("{name}-check.json"));
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn la_identidad_es_el_sha256_del_json_de_python_con_separadores_por_defecto() {
        // Valor calculado con `hashlib.sha256(json.dumps(clave, sort_keys=True).encode())`.
        let key = json!({
            "sources": {"main": "C:/x", "caf\u{e9}": "https://e.org"},
            "inventory": {"main/tool": {"key": "main/tool", "version": "1.0"}}
        });
        assert_eq!(
            dumps(&key, DumpOptions::DEFAULT_SORTED),
            r#"{"inventory": {"main/tool": {"key": "main/tool", "version": "1.0"}}, "sources": {"caf\u00e9": "https://e.org", "main": "C:/x"}}"#
        );
        let (_root, layout) = layout();
        daily_on(
            &layout,
            "component-updates",
            &key,
            false,
            "2026-09-13",
            || Ok(2_u32),
        )
        .unwrap();
        let saved = record(&layout, "component-updates");
        assert_eq!(
            saved["key"],
            "8b383c776ea363279fa3b059dc477ea59d4b97a467aa4294a47c15d493889c1b"
        );
        assert_eq!(saved["day"], "2026-09-13");
        assert_eq!(saved["result"], 2);
        assert_eq!(saved["token"].as_str().map(str::len), Some(32));
    }

    #[test]
    fn exito_cacheado_el_mismo_dia_y_se_repite_al_cambiar_dia_o_clave() {
        let (_root, layout) = layout();
        let calls = Cell::new(0);
        let run = |key: &str, day: &str, force: bool| {
            daily_on(&layout, "check", &json!(key), force, day, || {
                calls.set(calls.get() + 1);
                Ok(calls.get())
            })
            .unwrap()
        };
        assert_eq!(run("source", "2026-01-01", false), Some(1));
        assert_eq!(run("source", "2026-01-01", false), Some(1));
        assert_eq!(calls.get(), 1);
        assert_eq!(run("source", "2026-01-02", false), Some(2));
        assert_eq!(run("another source", "2026-01-02", false), Some(3));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn la_comprobacion_manual_se_salta_y_reemplaza_la_cache() {
        let (_root, layout) = layout();
        let key = json!("k");
        let first = daily_on(&layout, "check", &key, false, "2026-01-01", || Ok(1_u32)).unwrap();
        let forced = daily_on(&layout, "check", &key, true, "2026-01-01", || Ok(2_u32)).unwrap();
        let cached = daily_on(&layout, "check", &key, false, "2026-01-01", || Ok(3_u32)).unwrap();
        assert_eq!((first, forced, cached), (Some(1), Some(2), Some(2)));
    }

    #[test]
    fn un_fallo_no_se_repite_el_mismo_dia() {
        let (_root, layout) = layout();
        let key = json!("k");
        let failure = daily_on::<u32>(&layout, "check", &key, false, "2026-01-01", || {
            Err(Error::domain(Message::new("sin red")))
        });
        assert_eq!(failure.unwrap_err().to_string(), "sin red");
        assert_eq!(record(&layout, "check")["result"], Value::Null);
        let again = daily_on(
            &layout,
            "check",
            &key,
            false,
            "2026-01-01",
            || -> Result<u32> { unreachable!("no debe repetirse") },
        )
        .unwrap();
        assert_eq!(again, None);
    }

    #[test]
    fn una_llamada_anidada_en_curso_devuelve_none_y_la_externa_termina() {
        let (_root, layout) = layout();
        let key = json!("k");
        let outer = daily_on(&layout, "check", &key, false, "2026-01-01", || {
            let inner = daily_on(
                &layout,
                "check",
                &key,
                false,
                "2026-01-01",
                || -> Result<u32> { unreachable!("la operación externa está en curso") },
            )
            .unwrap();
            assert_eq!(inner, None);
            Ok(42_u32)
        })
        .unwrap();
        assert_eq!(outer, Some(42));
        assert_eq!(record(&layout, "check")["result"], 42);
    }

    #[test]
    fn si_otra_consulta_reemplaza_el_registro_no_se_sobrescribe() {
        let (_root, layout) = layout();
        let key = json!("k");
        let result = daily_on(&layout, "check", &key, false, "2026-01-01", || {
            // Una comprobación forzada concurrente toma el relevo con otro token.
            daily_on(&layout, "check", &key, true, "2026-01-01", || Ok(7_u32)).unwrap();
            Ok(1_u32)
        })
        .unwrap();
        assert_eq!(result, Some(1));
        assert_eq!(record(&layout, "check")["result"], 7);
    }

    #[test]
    fn filtros_de_buckets() {
        let (_root, layout) = layout();
        assert_eq!(read_disabled_buckets(&layout).unwrap(), BTreeSet::new());
        set_bucket_enabled(&layout, "extras", false).unwrap();
        set_bucket_enabled(&layout, "a", false).unwrap();
        let text = fs::read_to_string(filters_path(&layout)).unwrap();
        assert_eq!(text, "[\n  \"a\",\n  \"extras\"\n]\n");
        let disabled = set_bucket_enabled(&layout, "a", true).unwrap();
        assert_eq!(disabled, BTreeSet::from(["extras".to_owned()]));
        assert_eq!(
            fs::read_to_string(filters_path(&layout)).unwrap(),
            "[\n  \"extras\"\n]\n"
        );
        for invalid in [r#"{"extras": true}"#, "[1]", "no json"] {
            fs::write(filters_path(&layout), invalid).unwrap();
            assert!(read_disabled_buckets(&layout).is_err(), "{invalid}");
        }
    }
}
