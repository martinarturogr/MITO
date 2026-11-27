//! Datos de la página del catálogo (`load_catalog`, `filter_catalog` y
//! `on_data_table_row_highlighted` del original), sin dibujar nada.

use std::collections::BTreeSet;

use mito_catalog::pyjson::{py_str, py_truthy};
use mito_catalog::{Compatibility, Entry, rank_entries, read_disabled_buckets, set_bucket_enabled};
use mito_core::{Layout, Message};
use mito_engine::{Engine, Result};
use mito_i18n::Translator;
use serde_json::Value;

/// Catálogo cargado: entradas con su compatibilidad y buckets ocultos.
#[derive(Debug, Clone)]
pub struct Catalog {
    layout: Layout,
    entries: Vec<Entry<Compatibility>>,
    disabled: BTreeSet<String>,
}

/// Fila de la tabla del catálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    /// Clave `bucket/app`.
    pub key: String,
    /// `str(manifest.get('version', '?'))`.
    pub version: String,
    /// «Instalado», «Disponible» o «Todavía no soportado», ya traducido.
    pub availability: String,
    /// Licencia, ya resuelta (ver [`license_label`]).
    pub license: String,
    /// El manifiesto es `Compatible`.
    pub compatible: bool,
    /// La clave está instalada.
    pub installed: bool,
}

/// Resultado de filtrar el catálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// Filas ordenadas por relevancia.
    pub rows: Vec<CatalogRow>,
    /// Dos líneas: el recuento y el criterio de orden. Sin filas, la interfaz dibuja la
    /// primera en negrita azul (`#4da6ff`).
    pub summary: String,
    /// Aviso «Sin componentes…» que se añade bajo el resumen cuando no hay filas.
    pub empty_hint: Option<String>,
    /// Texto inicial del detalle: «Selecciona una aplicación.» o «Sin resultados…».
    pub details: String,
}

/// Detalle de la fila resaltada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    /// Texto exacto de `#details`.
    pub text: String,
    /// «Instalar» se habilita: compatible y no instalado (la interfaz añade `not busy`).
    pub can_install: bool,
}

impl Catalog {
    /// `load_catalog`: lee los snapshots, clasifica cada manifiesto y lee los buckets
    /// ocultos de `core/state/catalog-filters.json`.
    ///
    /// # Errors
    ///
    /// Un snapshot o el archivo de filtros ilegibles.
    pub fn load(engine: &Engine) -> Result<Self> {
        let entries = engine.catalog()?.search_entries()?;
        let disabled = read_disabled_buckets(engine.layout())?;
        Ok(Self {
            layout: engine.layout().clone(),
            entries,
            disabled,
        })
    }

    /// Nombres de las casillas de bucket: unión ordenada de los orígenes configurados y
    /// de los snapshots. Si el catálogo no se puede leer, no hay casillas.
    pub fn buckets(engine: &Engine) -> Vec<String> {
        engine
            .catalog()
            .and_then(|catalog| Ok(catalog.bucket_names()?))
            .map(|names| names.into_iter().collect())
            .unwrap_or_default()
    }

    /// Buckets ocultos en la búsqueda.
    pub fn disabled(&self) -> &BTreeSet<String> {
        &self.disabled
    }

    /// Muestra u oculta un bucket y guarda `catalog-filters.json` (lista ordenada).
    ///
    /// # Errors
    ///
    /// Los de lectura y escritura del archivo de filtros.
    pub fn set_enabled(&mut self, bucket: &str, enabled: bool) -> Result<()> {
        self.disabled = set_bucket_enabled(&self.layout, bucket, enabled)?;
        Ok(())
    }

    /// `filter_catalog`: filas para `term` sin los buckets ocultos. `installed` es el
    /// conjunto de ids instalados.
    pub fn filter(&self, term: &str, installed: &BTreeSet<String>, tr: &Translator) -> Listing {
        // Una clave sin `/` es imposible aquí (las crea `Catalog::all`); si ocurriera, el
        // original fallaría entero: aquí la lista queda vacía.
        let ranked =
            rank_entries(&self.entries, &term.to_lowercase(), &self.disabled).unwrap_or_default();
        let rows: Vec<CatalogRow> = ranked
            .into_iter()
            .map(|entry| {
                let installed = installed.contains(&entry.key);
                let compatible = entry.status.is_compatible();
                let availability = if installed {
                    "Instalado"
                } else if compatible {
                    "Disponible"
                } else {
                    "Todavía no soportado"
                };
                CatalogRow {
                    key: entry.key.clone(),
                    version: version_text(entry.manifest.raw()),
                    availability: tr.text(availability).to_owned(),
                    license: license_label(entry.manifest.raw(), tr),
                    compatible,
                    installed,
                }
            })
            .collect();
        let available = rows.iter().filter(|r| r.compatible && !r.installed).count();
        let summary = tr.tr(&Message::new(
            "{p0} componentes · {p2} disponibles\nOrden: palabras coincidentes, bucket/nombre y descripción. Coincidencias parciales al final.",
        )
        .arg("p0", rows.len())
        .arg("p2", available));
        let (empty_hint, details) = if rows.is_empty() {
            let hint = tr.tr(&Message::new(
                "Sin componentes. Pulsa el botón \"{refresh}\". Si no aparece ninguno comprueba tus fuentes en \"{configuration}\"",
            )
            .arg("refresh", tr.text("Actualizar catálogos"))
            .arg("configuration", tr.text("Configuración")));
            (
                Some(hint),
                tr.text("Sin resultados en los buckets seleccionados.")
                    .to_owned(),
            )
        } else {
            (None, tr.text("Selecciona una aplicación.").to_owned())
        };
        Listing {
            rows,
            summary,
            empty_hint,
            details,
        }
    }

    fn entry(&self, key: &str) -> Option<&Entry<Compatibility>> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    /// Texto de `#details` al resaltar `key` (`on_data_table_row_highlighted`). `None` si
    /// la clave no está en el catálogo.
    pub fn details(&self, key: &str, installed: bool, tr: &Translator) -> Option<Details> {
        let entry = self.entry(key)?;
        let raw = entry.manifest.raw();
        let description = raw.get("description").map(py_str).unwrap_or_default();
        let availability = if installed {
            tr.text("Instalado").to_owned()
        } else {
            match &entry.status {
                Compatibility::Compatible => tr.text("Compatible").to_owned(),
                Compatibility::Unsupported(reason) => tr.tr(reason),
            }
        };
        let mut text = format!(
            "{key} · {}\n{description}\n{availability}",
            version_text(raw)
        );
        for line in hook_lines(raw, tr) {
            text.push('\n');
            text.push_str(&line);
        }
        if let Some(notes) = raw.get("notes").filter(|notes| py_truthy(notes)) {
            let notes = match notes {
                Value::Array(items) => items.iter().map(py_str).collect::<Vec<_>>().join("\n"),
                other => py_str(other),
            };
            text.push_str(&tr.tr(&Message::new("\n\nNOTAS\n{p1}").arg("p1", notes)));
        }
        Some(Details {
            text,
            can_install: !installed && entry.status.is_compatible(),
        })
    }

    /// Manifiesto original de `key`, para el diálogo «Manifiesto».
    pub fn manifest(&self, key: &str) -> Option<&Value> {
        self.entry(key).map(|entry| entry.manifest.raw())
    }

    /// Versión de `key` que ha visto el usuario (`str(manifest.get('version', '?'))`).
    pub fn version(&self, key: &str) -> Option<String> {
        self.entry(key)
            .map(|entry| version_text(entry.manifest.raw()))
    }
}

fn version_text(manifest: &Value) -> String {
    manifest
        .get("version")
        .map_or_else(|| "?".to_owned(), py_str)
}

/// Texto de la columna «Licencia»: la cadena (o `identifier`/`url` si es un diccionario)
/// sin espacios de los lados o «No declarada». El identificador no se traduce.
pub fn license_label(manifest: &Value, tr: &Translator) -> String {
    let license = match manifest.get("license") {
        Some(Value::Object(map)) => [map.get("identifier"), map.get("url")]
            .into_iter()
            .flatten()
            .find(|value| py_truthy(value)),
        other => other,
    };
    match license {
        Some(Value::String(text)) if !text.trim().is_empty() => text.trim().to_owned(),
        _ => tr.text("No declarada").to_owned(),
    }
}

/// Valor de `field` en `architecture.64bit` o, si no es verdadero, en el primer nivel.
fn hook_field<'a>(manifest: &'a Value, field: &str) -> Option<&'a Value> {
    manifest
        .get("architecture")
        .and_then(|arch| arch.get("64bit"))
        .and_then(|arch| arch.get(field))
        .filter(|value| py_truthy(value))
        .or_else(|| manifest.get(field))
}

/// Avisos de los hooks que MITO no ejecuta como Scoop (F7), en el orden del original.
fn hook_lines(manifest: &Value, tr: &Translator) -> Vec<String> {
    const RUNS: &str = "{hook}: se ejecuta con PowerShell durante la instalación.";
    let mut lines = Vec::new();
    if hook_field(manifest, "pre_install").is_some_and(py_truthy) {
        lines.push(
            tr.text("pre_install: se ejecuta con PowerShell durante la instalación.")
                .to_owned(),
        );
    }
    if let Some(Value::Object(installer)) = hook_field(manifest, "installer") {
        for hook in ["script", "file"] {
            if installer.get(hook).is_some_and(py_truthy) {
                lines.push(tr.tr(&Message::new(RUNS).arg("hook", format!("installer.{hook}"))));
            }
        }
    }
    if hook_field(manifest, "post_install").is_some_and(py_truthy) {
        lines.push(
            tr.tr(&Message::new("Paso omitido: MITO no ejecuta {p1}.").arg("p1", "post_install")),
        );
    }
    lines
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::model::fixture::{Fx, fx};

    fn tr() -> Translator {
        crate::model::fixture::spanish()
    }

    fn apps() -> Value {
        json!({
            "tool": {"version": "1.0", "description": "Una herramienta",
                     "license": "MIT", "url": "https://example.org/tool.exe"},
            "java": {"version": 21, "description": "Java development kit",
                     "license": {"identifier": "GPL-2.0", "url": "https://x.org/l"},
                     "url": "https://example.org/java.exe"},
            "legacy": {"version": "0.1", "description": "Algo antiguo", "psmodule": "x",
                       "url": "https://example.org/legacy.exe"}
        })
    }

    fn loaded() -> (Fx, Catalog) {
        let f = fx();
        f.catalog(apps());
        f.bucket(
            "extras",
            &json!({"editor": {"version": "3", "description": "Editor de texto",
                                              "url": "https://example.org/editor.exe"}}),
        );
        let catalog = Catalog::load(&f.engine).unwrap();
        (f, catalog)
    }

    fn keys(listing: &Listing) -> Vec<&str> {
        listing.rows.iter().map(|row| row.key.as_str()).collect()
    }

    #[test]
    fn sin_termino_todo_ordenado_por_clave_con_su_disponibilidad() {
        let (_f, catalog) = loaded();
        let installed = BTreeSet::from(["main/tool".to_owned()]);
        let listing = catalog.filter("", &installed, &tr());
        assert_eq!(
            keys(&listing),
            ["extras/editor", "main/java", "main/legacy", "main/tool"]
        );
        let availability: Vec<&str> = listing
            .rows
            .iter()
            .map(|r| r.availability.as_str())
            .collect();
        assert_eq!(
            availability,
            [
                "Disponible",
                "Disponible",
                "Todavía no soportado",
                "Instalado"
            ]
        );
        assert_eq!(listing.rows[1].version, "21");
        assert_eq!(listing.rows[1].license, "GPL-2.0");
        assert_eq!(listing.rows[0].license, "No declarada");
        assert_eq!(listing.details, "Selecciona una aplicación.");
        assert_eq!(listing.empty_hint, None);
        assert_eq!(
            listing.summary,
            "4 componentes · 2 disponibles\nOrden: palabras coincidentes, bucket/nombre y descripción. Coincidencias parciales al final."
        );
    }

    #[test]
    fn el_termino_filtra_y_ordena_por_relevancia() {
        let (_f, catalog) = loaded();
        let listing = catalog.filter("JAVA", &BTreeSet::new(), &tr());
        assert_eq!(keys(&listing), ["main/java"]);
        let listing = catalog.filter("herramienta", &BTreeSet::new(), &tr());
        assert_eq!(keys(&listing), ["main/tool"]);
    }

    #[test]
    fn sin_resultados_hay_aviso_y_detalle_vacio() {
        let (_f, catalog) = loaded();
        let listing = catalog.filter("zzzz", &BTreeSet::new(), &tr());
        assert!(listing.rows.is_empty());
        assert_eq!(
            listing.details,
            "Sin resultados en los buckets seleccionados."
        );
        assert_eq!(
            listing.summary,
            "0 componentes · 0 disponibles\nOrden: palabras coincidentes, bucket/nombre y descripción. Coincidencias parciales al final."
        );
        assert_eq!(
            listing.empty_hint.as_deref(),
            Some(
                "Sin componentes. Pulsa el botón \"Actualizar catálogos\". Si no aparece ninguno comprueba tus fuentes en \"Configuración\""
            )
        );
    }

    #[test]
    fn un_bucket_oculto_se_guarda_como_lista_ordenada_y_se_excluye() {
        let (f, mut catalog) = loaded();
        catalog.set_enabled("main", false).unwrap();
        catalog.set_enabled("extras", false).unwrap();
        catalog.set_enabled("extras", true).unwrap();
        catalog.set_enabled("zeta", false).unwrap();
        let path = f.engine.layout().state().join("catalog-filters.json");
        let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved, json!(["main", "zeta"]));
        assert_eq!(catalog.disabled().len(), 2);
        let listing = catalog.filter("", &BTreeSet::new(), &tr());
        assert_eq!(keys(&listing), ["extras/editor"]);
        // Una carga posterior lee el mismo archivo.
        let again = Catalog::load(&f.engine).unwrap();
        assert_eq!(again.disabled(), catalog.disabled());
    }

    #[test]
    fn los_buckets_son_la_union_ordenada_de_origenes_y_snapshots() {
        let (f, _catalog) = loaded();
        fs::write(
            f.engine.layout().config_file(),
            "bucket.zeta=C:/origen\nbucket.main=C:/otro\n",
        )
        .unwrap();
        assert_eq!(Catalog::buckets(&f.engine), ["extras", "main", "zeta"]);
    }

    #[test]
    fn el_detalle_muestra_la_disponibilidad_o_el_motivo() {
        let (_f, catalog) = loaded();
        let tool = catalog.details("main/tool", false, &tr()).unwrap();
        assert_eq!(tool.text, "main/tool · 1.0\nUna herramienta\nCompatible");
        assert!(tool.can_install);
        let installed = catalog.details("main/tool", true, &tr()).unwrap();
        assert_eq!(
            installed.text,
            "main/tool · 1.0\nUna herramienta\nInstalado"
        );
        assert!(!installed.can_install);
        let legacy = catalog.details("main/legacy", false, &tr()).unwrap();
        assert_eq!(
            legacy.text,
            "main/legacy · 0.1\nAlgo antiguo\nPasos no soportados: psmodule"
        );
        assert!(!legacy.can_install);
        assert!(catalog.details("main/nada", false, &tr()).is_none());
    }

    #[test]
    fn el_detalle_incluye_hooks_y_notas_en_orden() {
        let f = fx();
        f.catalog(json!({"hooks": {
            "version": "2", "description": "Con hooks",
            "url": "https://example.org/h.exe",
            "architecture": {"64bit": {"pre_install": "echo hola"}},
            "installer": {"script": "x", "file": "y"},
            "post_install": ["a"],
            "notes": ["Primera", "Segunda"]
        }}));
        let catalog = Catalog::load(&f.engine).unwrap();
        let details = catalog.details("main/hooks", false, &tr()).unwrap();
        // La línea de disponibilidad depende de la compatibilidad; los avisos no.
        assert!(details.text.starts_with("main/hooks · 2\nCon hooks\n"));
        let tail = [
            "pre_install: se ejecuta con PowerShell durante la instalación.",
            "installer.script: se ejecuta con PowerShell durante la instalación.",
            "installer.file: se ejecuta con PowerShell durante la instalación.",
            "Paso omitido: MITO no ejecuta post_install.",
            "",
            "NOTAS",
            "Primera",
            "Segunda",
        ]
        .join("\n");
        assert!(
            details.text.ends_with(&format!("\n{tail}")),
            "{}",
            details.text
        );
    }

    #[test]
    fn las_notas_de_texto_se_muestran_tal_cual() {
        let f = fx();
        f.catalog(json!({"n": {"version": "1", "description": "d",
                               "url": "https://example.org/n.exe", "notes": "Una nota"}}));
        let catalog = Catalog::load(&f.engine).unwrap();
        let details = catalog.details("main/n", false, &tr()).unwrap();
        assert_eq!(details.text, "main/n · 1\nd\nCompatible\n\nNOTAS\nUna nota");
    }

    #[test]
    fn manifiesto_y_version_vistos() {
        let (_f, catalog) = loaded();
        assert_eq!(catalog.version("main/java").as_deref(), Some("21"));
        assert_eq!(catalog.version("main/nada"), None);
        assert_eq!(catalog.manifest("main/tool").unwrap()["license"], "MIT");
        assert!(catalog.manifest("main/nada").is_none());
    }

    #[test]
    fn la_licencia_es_cadena_diccionario_o_no_declarada() {
        let tr = tr();
        assert_eq!(license_label(&json!({"license": "  MIT  "}), &tr), "MIT");
        assert_eq!(
            license_label(
                &json!({"license": {"identifier": "Apache-2.0", "url": "u"}}),
                &tr
            ),
            "Apache-2.0"
        );
        assert_eq!(
            license_label(&json!({"license": {"url": "https://l"}}), &tr),
            "https://l"
        );
        assert_eq!(license_label(&json!({"license": {}}), &tr), "No declarada");
        assert_eq!(
            license_label(&json!({"license": "   "}), &tr),
            "No declarada"
        );
        assert_eq!(license_label(&json!({"license": 3}), &tr), "No declarada");
        assert_eq!(license_label(&json!({}), &tr), "No declarada");
    }
}
