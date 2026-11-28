//! Datos de la página Inicio (`refresh_home` del original), sin dibujar nada.

use std::collections::{BTreeMap, BTreeSet};

use std::path::PathBuf;

use mito_catalog::{COMPATIBLE, Manifest};
use mito_core::Message;
use mito_desktop::host_data::Kind;
use mito_desktop::{host_apps, host_data, host_exposure, open_with};
use mito_engine::{Candidate, Engine, Result};
use mito_i18n::Translator;

use crate::integration::Integration;

/// Una fila de componente instalado.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "cada booleano es el estado de un botón de la fila"
)]
pub struct HomeRow {
    /// Id de la instalación (`bucket/app`).
    pub installation: String,
    /// Clave del componente.
    pub key: String,
    /// Versión instalada, como texto.
    pub version: String,
    /// Versión retenida: «Actualizar» no la cambia hasta liberarla.
    pub pinned: bool,
    /// Activo en el profile seleccionado.
    pub active: bool,
    /// Se puede abrir como aplicación (`can_open`).
    pub can_open: bool,
    /// Versión posterior del catálogo (`available_update`), compatible o no.
    pub update: Option<Candidate>,
    /// Hay extensiones registradas en «Abrir con» para este componente y profile
    /// (`Extensiones ✓`). Lo calcula [`host_state`].
    pub open_with: bool,
    /// El `AppData` de MITO está expuesto en el host (`Host · ✓`). Lo calcula
    /// [`host_state`].
    pub exposed: bool,
}

/// Una aplicación del host registrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    /// Identidad registrada.
    pub identity: String,
    /// Ruta del ejecutable.
    pub path: PathBuf,
    /// El archivo sigue existiendo.
    pub available: bool,
    /// Hay enlaces de datos (`Data · ✓`).
    pub data: bool,
    /// Hay enlaces de caché (`Cache · ✓`).
    pub cache: bool,
}

impl HomeRow {
    /// ¿Hay una actualización instalable (estado `Compatible`)? Si no, el botón
    /// «Actualizar» se dibuja deshabilitado.
    pub fn update_is_compatible(&self) -> bool {
        self.update
            .as_ref()
            .is_some_and(|candidate| candidate.status == COMPATIBLE)
    }
}

/// Contenido de Inicio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    /// Una fila por componente instalado, en el orden del inventario.
    pub rows: Vec<HomeRow>,
    /// Componentes activos en el profile (`len(active)` del original: los ids de
    /// `profile.apps`, estén o no instalados).
    pub active: usize,
    /// Instalaciones con actualización `Compatible`, en el orden de las filas.
    pub pending: Vec<String>,
    /// Aplicaciones del host registradas (vacío hasta llamar a [`host_state`]).
    pub host: Vec<HostRow>,
}

/// Calcula Inicio para `profile` (sin profile nada está activo) con el `catalog` actual.
///
/// # Errors
///
/// Los de lectura del inventario y del profile y los de `available_update`.
pub fn home(
    engine: &Engine,
    profile: Option<&str>,
    catalog: &BTreeMap<String, Manifest>,
) -> Result<Home> {
    let active: BTreeSet<String> = match profile.filter(|name| !name.is_empty()) {
        Some(name) => engine.profile(name)?.apps.into_iter().collect(),
        None => BTreeSet::new(),
    };
    let mut rows = Vec::new();
    let mut pending = Vec::new();
    for (installation, record) in &engine.installed()? {
        let update = engine.available_update(record, Some(catalog))?;
        let row = HomeRow {
            installation: installation.to_owned(),
            key: record.key.clone(),
            version: record.version_text(),
            pinned: record.pinned.is_some(),
            active: active.contains(installation),
            can_open: engine.can_open(record),
            update,
            open_with: false,
            exposed: false,
        };
        if row.update_is_compatible() {
            pending.push(row.installation.clone());
        }
        rows.push(row);
    }
    Ok(Home {
        rows,
        active: active.len(),
        pending,
        host: Vec::new(),
    })
}

/// Calcula lo que Inicio muestra del host (F8): los botones `Extensiones ✓/✗` y
/// `Host · ✓/✗` de cada fila y las aplicaciones del host con sus enlaces. Un estado
/// que no se puede calcular cuenta como `✗`; solo falla la lectura del inventario.
///
/// # Errors
///
/// La lectura de `core/state/host-applications.json`.
pub fn host_state(
    home: &mut Home,
    engine: &Engine,
    profile: Option<&str>,
    integration: &Integration,
) -> Result<()> {
    let root = engine.root();
    let roots = (integration.host)();
    for row in &mut home.rows {
        if row.can_open {
            row.exposed = roots.as_ref().is_ok_and(|roots| {
                host_exposure::linked(root, roots, &row.installation).unwrap_or(false)
            });
        }
        if row.active
            && row.can_open
            && let Some(profile) = profile.filter(|name| !name.is_empty())
        {
            row.open_with = open_with::entries(root, profile, &row.installation)
                .is_ok_and(|records| !records.is_empty());
        }
    }
    let linked = |identity: &str, kind: Kind| {
        roots
            .as_ref()
            .is_ok_and(|roots| host_data::linked(root, roots, identity, kind).unwrap_or(false))
    };
    home.host = host_apps::inventory(root)?
        .into_iter()
        .map(|(identity, path)| HostRow {
            available: path.is_file(),
            data: linked(&identity, Kind::Data),
            cache: linked(&identity, Kind::Cache),
            identity,
            path,
        })
        .collect();
    Ok(())
}

/// Etiqueta de un botón con estado: `{texto}✓` o `{texto}✗` (`Cache · ✓`).
pub fn state_label(text: &str, on: bool) -> String {
    format!("{text}{}", if on { '✓' } else { '✗' })
}

/// Nombre de una aplicación del host: `{nombre} · Host`, más ` · No disponible` si falta
/// el archivo.
pub fn host_label(row: &HostRow, tr: &Translator) -> String {
    let stem = row
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut label = format!("{stem} · Host");
    if !row.available {
        label.push_str(tr.text(" · No disponible"));
    }
    label
}

/// Texto de `#installed-summary`.
pub fn summary(home: &Home, profile: Option<&str>, tr: &Translator) -> String {
    let profile = profile
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| tr.text("ningún profile"));
    tr.tr(&Message::new(
        "{p0} instalados · {p2} activos en {p4} · {p6} del host\nLos cambios de entorno se cargan al abrir una nueva pestaña.",
    )
    .arg("p0", home.rows.len())
    .arg("p2", home.active)
    .arg("p4", profile)
    .arg("p6", home.host.len()))
}

/// Texto de la lista cuando no hay componentes instalados.
pub fn empty_text(tr: &Translator) -> String {
    tr.text("Todavía no hay componentes instalados.\nUsa el botón de abajo para añadirlos.")
        .to_owned()
}

/// Etiqueta de la casilla de una fila: `{key}   ·   {version}` y, si está retenida,
/// `   ·   retenida`.
pub fn row_label(row: &HomeRow, tr: &Translator) -> String {
    let mut label = format!("{}   ·   {}", row.key, row.version);
    if row.pinned {
        label.push_str("   ·   ");
        label.push_str(tr.text("retenida"));
    }
    label
}

/// Texto del botón «Actualizar todos ({p1})» con el número de pendientes.
pub fn bulk_update_label(home: &Home, tr: &Translator) -> String {
    tr.tr(&Message::new("Actualizar todos ({p1})").arg("p1", home.pending.len()))
}

/// Ayuda del botón «Actualizar»: `Versión {p1} · {p3}` con el estado de compatibilidad.
pub fn update_tooltip(candidate: &Candidate, tr: &Translator) -> String {
    tr.tr(&Message::new("Versión {p1} · {p3}")
        .arg("p1", &candidate.version)
        .arg("p3", tr.text(&candidate.status)))
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::fixture::{Fx, fx};

    fn tr() -> Translator {
        crate::model::fixture::spanish()
    }

    fn catalog(f: &Fx) -> BTreeMap<String, Manifest> {
        f.engine
            .catalog()
            .unwrap()
            .all()
            .unwrap()
            .into_iter()
            .collect()
    }

    fn installed_tool(f: &Fx) {
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.engine.create_profile("work", None).unwrap();
        f.install("main/tool", Some("work"));
    }

    #[test]
    fn sin_componentes_el_resumen_dice_ningun_profile() {
        let f = fx();
        let home = home(&f.engine, None, &BTreeMap::new()).unwrap();
        assert!(home.rows.is_empty() && home.pending.is_empty());
        assert_eq!(
            summary(&home, None, &tr()),
            "0 instalados · 0 activos en ningún profile · 0 del host\nLos cambios de entorno se cargan al abrir una nueva pestaña."
        );
        assert_eq!(
            empty_text(&tr()),
            "Todavía no hay componentes instalados.\nUsa el botón de abajo para añadirlos."
        );
    }

    #[test]
    fn una_fila_activa_y_abrible_con_resumen_del_profile() {
        let f = fx();
        installed_tool(&f);
        let home = home(&f.engine, Some("work"), &catalog(&f)).unwrap();
        assert_eq!(home.rows.len(), 1);
        let row = &home.rows[0];
        assert_eq!(row.installation, "main/tool");
        assert_eq!(row.key, "main/tool");
        assert_eq!(row.version, "1.0");
        assert!(row.active && row.can_open && !row.pinned);
        assert_eq!(row.update, None);
        assert_eq!(row_label(row, &tr()), "main/tool   ·   1.0");
        assert_eq!(
            summary(&home, Some("work"), &tr()),
            "1 instalados · 1 activos en work · 0 del host\nLos cambios de entorno se cargan al abrir una nueva pestaña."
        );
    }

    #[test]
    fn sin_profile_o_con_otro_profile_nada_esta_activo() {
        let f = fx();
        installed_tool(&f);
        f.engine.create_profile("other", None).unwrap();
        for profile in [None, Some("other")] {
            let home = home(&f.engine, profile, &catalog(&f)).unwrap();
            assert!(!home.rows[0].active);
            assert_eq!(home.active, 0);
        }
    }

    #[test]
    fn una_version_posterior_compatible_queda_pendiente() {
        let f = fx();
        installed_tool(&f);
        f.catalog(json!({"tool": f.manifest("2.0")}));
        let home = home(&f.engine, Some("work"), &catalog(&f)).unwrap();
        let row = &home.rows[0];
        assert_eq!(
            row.update,
            Some(Candidate {
                version: "2.0".to_owned(),
                status: COMPATIBLE.to_owned()
            })
        );
        assert!(row.update_is_compatible());
        assert_eq!(home.pending, ["main/tool"]);
        assert_eq!(bulk_update_label(&home, &tr()), "Actualizar todos (1)");
        assert_eq!(
            update_tooltip(row.update.as_ref().unwrap(), &tr()),
            "Versión 2.0 · Compatible"
        );
    }

    #[test]
    fn una_actualizacion_no_compatible_no_queda_pendiente() {
        let f = fx();
        installed_tool(&f);
        let mut newer = f.manifest("2.0");
        newer["psmodule"] = json!("x");
        f.catalog(json!({"tool": newer}));
        let home = home(&f.engine, Some("work"), &catalog(&f)).unwrap();
        let row = &home.rows[0];
        assert!(row.update.is_some());
        assert!(!row.update_is_compatible());
        assert!(home.pending.is_empty());
    }

    #[test]
    fn la_version_retenida_se_marca_y_no_propone_actualizacion() {
        let f = fx();
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.engine.create_profile("work", None).unwrap();
        f.engine
            .install_version(
                "main/tool",
                f.manifest("1.0"),
                json!({"source": "test", "commit": "0123456789abcdef"}),
                None,
                &|_| {},
                &mito_core::Operation::detached(),
            )
            .unwrap();
        f.catalog(json!({"tool": f.manifest("2.0")}));
        let home = home(&f.engine, None, &catalog(&f)).unwrap();
        let row = &home.rows[0];
        assert!(row.pinned);
        assert_eq!(row.update, None);
        assert_eq!(row_label(row, &tr()), "main/tool   ·   1.0   ·   retenida");
        assert!(home.pending.is_empty());
    }

    #[test]
    fn un_profile_inexistente_es_un_error() {
        let f = fx();
        assert!(home(&f.engine, Some("nope"), &BTreeMap::new()).is_err());
    }
}
