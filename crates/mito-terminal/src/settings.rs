//! `settings.json` del Windows Terminal privado (`terminal.prepare` del original).
//!
//! [`build_settings`] es una función pura (perfiles, selección y ajustes previos entran;
//! el JSON sale) y [`prepare`] la envuelve con la lectura y escritura de disco.

use std::fs;

use mito_core::storage::{read_json, write_json};
use mito_core::{Layout, config};
use mito_engine::Engine;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::branding;
use crate::command::{Shell, child_command};
use crate::commandline::list2cmdline;
use crate::error::Result;

/// Espacio de nombres de los GUID de los perfiles del Terminal.
const NAMESPACE: Uuid = Uuid::from_u128(0x87bc_a5b6_a16a_40f6_8eb6_d20a_9467_76c2_u128);

/// Entradas integradas de Terminal que se ocultan (la última coincide con el espacio de
/// nombres; el original las lista sin comentario).
const BUILT_IN: [&str; 3] = [
    "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
    "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
    "{87bca5b6-a16a-40f6-8eb6-d20a946776c2}",
];

/// Fuentes de perfil dinámicas desactivadas.
const DISABLED_SOURCES: [&str; 5] = [
    "Windows.Terminal.Wsl",
    "Windows.Terminal.Azure",
    "Windows.Terminal.PowershellCore",
    "Windows.Terminal.SSH",
    "Windows.Terminal.VisualStudio",
];

/// Modo de una entrada: determina el esquema de color y el color de pestaña.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// La pestaña del gestor MITO.
    Manager,
    /// `PowerShell (<profile>)`.
    PowerShell,
    /// `CMD (<profile>)`.
    Cmd,
}

impl Mode {
    /// `(nombre del esquema, fondo, color de pestaña)`.
    fn style(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Manager => ("MITO Forest", "#102820", "#368448"),
            Self::PowerShell => ("MITO PowerShell", "#102332", "#317ca4"),
            Self::Cmd => ("MITO CMD", "#282319", "#9a7735"),
        }
    }

    const ALL: [Self; 3] = [Self::Manager, Self::PowerShell, Self::Cmd];
}

/// GUID estable de la entrada de `profile` con `shell`, o del gestor si no hay profile:
/// `uuid5(NAMESPACE, "<profile>/<shell>")` y `uuid5(NAMESPACE, "manager")`.
pub fn guid(profile: Option<&str>, shell: Shell) -> String {
    let name = match profile {
        Some(profile) => format!("{profile}/{}", shell.as_str()),
        None => "manager".to_owned(),
    };
    format!(
        "{{{}}}",
        Uuid::new_v5(&NAMESPACE, name.as_bytes()).hyphenated()
    )
}

/// Esquema de color de MITO para `mode`.
pub fn scheme(mode: Mode) -> Value {
    let (name, background, _) = mode.style();
    json!({
        "name": name,
        "background": background,
        "foreground": "#e7efdf",
        "cursorColor": "#b6e568",
        "selectionBackground": "#456451",
        "black": "#14241e",
        "red": "#e27878",
        "green": "#8ccf58",
        "yellow": "#e4c465",
        "blue": "#69acce",
        "purple": "#be94ca",
        "cyan": "#63c6b3",
        "white": "#d5dfd5",
        "brightBlack": "#72877b",
        "brightRed": "#ffa09a",
        "brightGreen": "#b5ef75",
        "brightYellow": "#ffe38a",
        "brightBlue": "#8fcfee",
        "brightPurple": "#d7b4e3",
        "brightCyan": "#91edda",
        "brightWhite": "#f5faef",
    })
}

/// Qué abre Terminal por defecto (`terminal.defaultShell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultShell {
    /// `cmd` (valor por defecto y el de cualquier valor no válido).
    Cmd,
    /// `powershell`.
    PowerShell,
    /// `mito`: el gestor.
    Mito,
}

impl DefaultShell {
    /// Interpreta el valor de `terminal.defaultShell`; lo desconocido es `cmd`.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("powershell") => Self::PowerShell,
            Some("mito") => Self::Mito,
            _ => Self::Cmd,
        }
    }
}

/// Una entrada visible del menú: su profile (o el gestor) y su modo.
fn entry(layout: &Layout, profile: Option<&str>, mode: Mode) -> Map<String, Value> {
    let shell = match (profile, mode) {
        (Some(_), Mode::PowerShell) => Shell::PowerShell,
        _ => Shell::Cmd,
    };
    let name = match (profile, mode) {
        (Some(profile), Mode::PowerShell) => format!("PowerShell ({profile})"),
        (Some(profile), _) => format!("CMD ({profile})"),
        (None, _) => "MITO".to_owned(),
    };
    let (scheme_name, _, tab_color) = mode.style();
    let mut map = Map::new();
    map.insert("guid".into(), json!(guid(profile, shell)));
    map.insert("name".into(), json!(name));
    map.insert("tabTitle".into(), json!(name));
    map.insert(
        "commandline".into(),
        json!(list2cmdline(&child_command(layout, profile, shell))),
    );
    map.insert(
        "startingDirectory".into(),
        json!(layout.root().display().to_string()),
    );
    map.insert(
        "icon".into(),
        json!(layout.branding_icon().display().to_string()),
    );
    map.insert("colorScheme".into(), json!(scheme_name));
    map.insert("tabColor".into(), json!(tab_color));
    map.insert("hidden".into(), json!(false));
    map
}

/// Construye el `settings.json` a partir de los ajustes previos (`existing`).
///
/// Conserva las claves ajenas y los esquemas ajenos, reemplaza los tres esquemas de MITO
/// y sobrescribe las claves de MITO. La entrada predeterminada va primera en la lista y
/// en el menú.
pub fn build_settings(
    layout: &Layout,
    profiles: &[String],
    active: Option<&str>,
    default_shell: DefaultShell,
    existing: &Value,
) -> Value {
    let mut settings = existing.as_object().cloned().unwrap_or_default();
    let default_guid = match (active, default_shell) {
        (Some(active), DefaultShell::Cmd) => guid(Some(active), Shell::Cmd),
        (Some(active), DefaultShell::PowerShell) => guid(Some(active), Shell::PowerShell),
        _ => guid(None, Shell::Cmd),
    };
    let mut entries: Vec<Map<String, Value>> = Vec::new();
    for profile in profiles {
        entries.push(entry(layout, Some(profile), Mode::PowerShell));
        entries.push(entry(layout, Some(profile), Mode::Cmd));
    }
    entries.push(entry(layout, None, Mode::Manager));
    // La predeterminada primero, para que `+` y Ctrl+Shift+1 coincidan. La ordenación es
    // estable: el resto conserva su orden.
    entries.sort_by_key(|item| item.get("guid").and_then(Value::as_str) != Some(&default_guid));
    let menu: Vec<Value> = entries
        .iter()
        .map(|item| json!({"type": "profile", "profile": item["guid"]}))
        .collect();
    let mut list: Vec<Value> = entries.into_iter().map(Value::Object).collect();
    // Terminal añade estas entradas aunque las fuentes dinámicas estén desactivadas.
    list.extend(
        BUILT_IN
            .iter()
            .map(|value| json!({"guid": value, "hidden": true})),
    );
    let mut defaults = settings
        .get("profiles")
        .and_then(|profiles| profiles.get("defaults"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (key, value) in [
        ("font", json!({"face": "Cascadia Mono", "size": 11})),
        ("padding", json!("12")),
        ("opacity", json!(94)),
        ("useAcrylic", json!(true)),
        ("suppressApplicationTitle", json!(true)),
        ("closeOnExit", json!("graceful")),
    ] {
        defaults.insert(key.to_owned(), value);
    }
    let own_names: Vec<&str> = Mode::ALL.iter().map(|mode| mode.style().0).collect();
    let mut schemes: Vec<Value> = settings
        .get("schemes")
        .and_then(Value::as_array)
        .map(|old| {
            old.iter()
                .filter(|item| {
                    !item
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| own_names.contains(&name))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    schemes.extend(Mode::ALL.iter().map(|mode| scheme(*mode)));
    for (key, value) in [
        ("theme", json!("dark")),
        ("launchMode", json!("maximized")),
        ("showTabsInFullscreen", json!(true)),
        ("alwaysShowTabs", json!(true)),
        ("copyOnSelect", json!(false)),
        ("defaultProfile", json!(default_guid)),
        ("firstWindowPreference", json!("defaultProfile")),
        ("newTabMenu", Value::Array(menu)),
        ("disabledProfileSources", json!(DISABLED_SOURCES)),
        (
            "profiles",
            json!({"defaults": Value::Object(defaults), "list": list}),
        ),
        ("schemes", Value::Array(schemes)),
    ] {
        settings.insert(key.to_owned(), value);
    }
    Value::Object(settings)
}

/// Perfil seleccionado: el pedido si existe, si no el previo si existe, si no el primero.
fn selection(names: &[String], requested: Option<&str>, previous: Option<&str>) -> Option<String> {
    [requested, previous]
        .into_iter()
        .flatten()
        .find(|name| names.iter().any(|candidate| candidate == name))
        .map(str::to_owned)
        .or_else(|| names.first().cloned())
}

/// Genera `core/tools/windows-terminal/settings/settings.json` y los archivos de estado.
///
/// No hace nada si falta `WindowsTerminal.exe`. Es idempotente: no reescribe el JSON si
/// no cambia. La primera vez conserva una copia de los ajustes previos en
/// `core/state/terminal-settings-before-branding.json`. Un fallo al derivar
/// `YitoTerminal.exe` no impide generar los ajustes: se registra un aviso y se usará
/// `WindowsTerminal.exe` (ADR 0010).
///
/// `selected` es el profile que se quiere como predeterminado; sin él se conserva el de
/// la última selección (`core/state/terminal-profile.json`).
///
/// # Errors
///
/// Los de lectura y escritura de JSON y de los profiles.
pub fn prepare(engine: &Engine, selected: Option<&str>) -> Result<()> {
    let layout = engine.layout();
    let folder = layout.windows_terminal();
    if !layout.windows_terminal_exe().is_file() {
        return Ok(());
    }
    let names = engine.profiles();
    let selection_path = layout.state().join("terminal-profile.json");
    let previous: Option<String> = if selection_path.exists() {
        read_json::<Value>(&selection_path)?
            .as_str()
            .map(str::to_owned)
    } else {
        None
    };
    let active = selection(&names, selected, previous.as_deref());
    if active != previous {
        write_json(&selection_path, &active)?;
    }
    let default_shell =
        DefaultShell::parse(config::read_global(layout)?.effective("terminal.defaultShell"));
    let portable = folder.join(".portable");
    if !portable.exists() {
        fs::write(&portable, b"").map_err(|e| mito_core::Error::io(&portable, e))?;
    }
    branding::brand_or_warn(layout);
    let path = folder.join("settings").join("settings.json");
    let existing: Value = if path.exists() {
        read_json(&path)?
    } else {
        json!({})
    };
    let backup = layout
        .state()
        .join("terminal-settings-before-branding.json");
    if existing.as_object().is_some_and(|map| !map.is_empty()) && !backup.exists() {
        write_json(&backup, &existing)?;
    }
    let settings = build_settings(layout, &names, active.as_deref(), default_shell, &existing);
    if !path.exists() || existing != settings {
        write_json(&path, &settings)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
