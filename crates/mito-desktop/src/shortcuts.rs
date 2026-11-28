//! Accesos directos de aplicaciones, del host y de MITO en el escritorio (`shortcuts.py`).
//!
//! Cada acceso es un `.lnk` escrito con `IShellLinkW` (sin PowerShell, ADR 0013) y un
//! registro JSON en `core/state/` para poder actualizarlo después. El destino de los
//! accesos de aplicaciones y del host es `core\bin\mitow.exe` (sin consola); el de la
//! terminal de MITO es `YitoTerminal.exe` o `WindowsTerminal.exe`.
//!
//! El escritorio se recibe como una función (normalmente [`desktop_path`]) para que las
//! pruebas escriban en una carpeta temporal y para que, como en el original, un
//! componente inactivo se rechace antes de tocar el escritorio.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mito_core::Message;
use mito_core::config::read_global;
use mito_core::storage::{identifier, read_json, write_json};
use mito_engine::{Engine, Error, Host, Log, LogLine, Record, Result};
use mito_terminal::{Shell, command, list2cmdline, prepare};
use mito_win::shell::{KnownFolder, Link, known_folder, write_link};
use serde_json::{Value, json};

use crate::icons::{
    BADGE_COLORS, DEFAULT_BADGE_COLOR, DEFAULT_BADGE_TEXT, create_mito_shortcut_icon, sha256_hex,
};

/// Opciones comunes de los accesos de aplicación y de host.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinkOptions<'a> {
    /// Letras de la insignia; `None` usa `shortcut.badgeText` de `config.properties`.
    pub badge: Option<&'a str>,
    /// Color de la insignia; `None` usa `shortcut.badgeColor`.
    pub color: Option<&'a str>,
    /// Abrir en una pestaña de la terminal de MITO (`--coupled`).
    pub coupled: bool,
    /// Argumentos que se pasan a la aplicación.
    pub arguments: &'a [String],
    /// Etiqueta del acceso declarado en el manifiesto (solo aplicaciones).
    pub shortcut: Option<&'a str>,
}

/// Función que da la carpeta del escritorio (ver [`desktop_path`]).
pub type Desktop<'a> = &'a dyn Fn() -> Result<PathBuf>;

fn domain(message: Message) -> Error {
    Error::domain(message)
}

/// Escritorio del usuario (`SHGetKnownFolderPath`), que respeta `OneDrive` y las
/// redirecciones.
///
/// # Errors
///
/// `No se pudo localizar el escritorio de Windows`.
pub fn desktop_path() -> Result<PathBuf> {
    known_folder(KnownFolder::Desktop).map_err(|_| {
        domain(Message::new(
            "No se pudo localizar el escritorio de Windows",
        ))
    })
}

fn configured(engine: &Engine, key: &str) -> Result<Option<String>> {
    let values = read_global(engine.layout())?;
    Ok(values.get(key).map(str::to_owned))
}

/// Letras de la insignia (`badge_text`): `value` o `shortcut.badgeText`, recortadas y en
/// mayúsculas; vacío da `MITO`. De 1 a 4 letras o números.
///
/// # Errors
///
/// `La etiqueta admite de 1 a 4 letras o números`.
pub fn badge_text(engine: &Engine, value: Option<&str>) -> Result<String> {
    let value = match value {
        Some(value) => Some(value.to_owned()),
        None => configured(engine, "shortcut.badgeText")?,
    };
    Ok(mito_core::config::badge_text(
        value.as_deref().unwrap_or(DEFAULT_BADGE_TEXT),
    )?)
}

/// Color de la insignia (`badge_color`): clave de la paleta, en minúsculas.
///
/// # Errors
///
/// `Elige un color de la paleta del icono`.
pub fn badge_color(engine: &Engine, value: Option<&str>) -> Result<String> {
    let value = match value {
        Some(value) => Some(value.to_owned()),
        None => configured(engine, "shortcut.badgeColor")?,
    };
    let value = value.unwrap_or_default().trim().to_lowercase();
    let value = if value.is_empty() {
        DEFAULT_BADGE_COLOR.to_owned()
    } else {
        value
    };
    if !BADGE_COLORS.iter().any(|(name, _)| *name == value) {
        return Err(domain(Message::new(
            "Elige un color de la paleta del icono",
        )));
    }
    Ok(value)
}

/// Icono con insignia de un acceso (`application_icon`): `base` o el icono de MITO. Si
/// el icono base falla se reintenta con el de MITO.
///
/// # Errors
///
/// Los de [`create_mito_shortcut_icon`].
pub fn application_icon(
    engine: &Engine,
    name: &str,
    badge: &str,
    base: Option<&Path>,
    color: &str,
) -> Result<PathBuf> {
    let fallback = engine.layout().branding_icon();
    let directory = engine.layout().state().join("icons");
    let launcher = identifier(name)?;
    match create_mito_shortcut_icon(
        base.unwrap_or(&fallback),
        &directory,
        &launcher,
        badge,
        color,
    ) {
        Err(_) if base.is_some_and(|base| base != fallback) => {
            create_mito_shortcut_icon(&fallback, &directory, &launcher, badge, color)
        }
        other => other,
    }
}

/// Icono de la entrada con la que se abre el componente (`component_icon_source`): el
/// declarado en el manifiesto si existe, y si no el ejecutable.
///
/// # Errors
///
/// Los de `application_target` y `component_path` (rutas fuera del componente).
pub fn component_icon_source(
    engine: &Engine,
    record: &Record,
    profile: &str,
    shortcut: Option<&str>,
) -> Result<PathBuf> {
    let (executable, arguments) = engine.application_target(record, shortcut)?;
    let payload = mito_engine::isolation::payload_path(engine.layout(), record, Some(profile))?;
    let manifest = record.effective()?;
    if let Some(Value::Array(shortcuts)) = manifest.get("shortcuts") {
        for entry in shortcuts {
            let Some(parts) = entry.as_array() else {
                continue;
            };
            let declared = parts.first().map(mito_catalog::pyjson::py_str);
            let fixed = match parts.get(2) {
                Some(Value::String(text)) => mito_engine::arguments::split(text),
                Some(Value::Array(items)) => {
                    items.iter().map(mito_catalog::pyjson::py_str).collect()
                }
                _ => Vec::new(),
            };
            if declared.as_deref() == Some(executable.as_str()) && fixed == arguments {
                if let Some(icon) = parts.get(3).filter(|i| mito_catalog::pyjson::py_truthy(i)) {
                    let icon =
                        engine.component_path(&payload, &mito_catalog::pyjson::py_str(icon))?;
                    if icon.is_file() {
                        return Ok(icon);
                    }
                }
                break;
            }
        }
    }
    engine.component_path(&payload, &executable)
}

/// Escribe el `.lnk` (`write_link`).
fn write_desktop_link(path: &Path, link: &Link) -> Result<()> {
    write_link(path, link).map_err(|error| Error::io(path, error))
}

/// Nombre libre del escritorio: `<stem>.lnk` o `<stem> (2).lnk`, … si existe y no es el
/// que ya estaba registrado. Un `.lnk` ajeno nunca se sobrescribe.
fn free_path(folder: &Path, stem: &str, registered: Option<&str>) -> PathBuf {
    let mut path = folder.join(format!("{stem}.lnk"));
    let mut index = 2;
    while path.exists() && Some(path.display().to_string().as_str()) != registered {
        path = folder.join(format!("{stem} ({index}).lnk"));
        index += 1;
    }
    path
}

fn registered_path(registry: &Path) -> Result<Option<String>> {
    if !registry.exists() {
        return Ok(None);
    }
    let old: Value = read_json(registry)?;
    Ok(old.get("path").and_then(Value::as_str).map(str::to_owned))
}

/// Primeros 16 caracteres del SHA-256 de `text`.
fn tag(text: &str) -> String {
    sha256_hex(&[text.as_bytes()])[..16].to_owned()
}

/// Primeros 8 caracteres del SHA-256 de la insignia.
fn badge_tag(badge: &str) -> String {
    sha256_hex(&[badge.as_bytes()])[..8].to_owned()
}

/// Argumentos de `mitow.exe` de un acceso de aplicación.
fn application_arguments(
    profile: &str,
    app: &str,
    coupled: bool,
    shortcut: Option<&str>,
    arguments: &[String],
) -> Vec<String> {
    let mut argv = vec!["run".to_owned()];
    if coupled {
        argv.push("--coupled".to_owned());
    }
    argv.extend(["--profile", profile, "--app", app].map(str::to_owned));
    if let Some(shortcut) = shortcut {
        argv.extend(["--shortcut".to_owned(), shortcut.to_owned()]);
    }
    if !arguments.is_empty() {
        argv.push("--".to_owned());
        argv.extend(arguments.iter().cloned());
    }
    argv
}

/// Etiqueta saneada para un nombre de archivo: los caracteres no válidos pasan a `_`,
/// sin espacios ni puntos en los extremos y como mucho 80 caracteres.
fn sanitize_label(label: &str) -> String {
    let replaced: String = label
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || u32::from(c) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    replaced.trim_matches([' ', '.']).chars().take(80).collect()
}

/// Crea o actualiza el acceso de escritorio de una aplicación activa en un profile
/// (`application_link`) y devuelve la ruta del `.lnk`.
///
/// El acceso no depende de la versión: `mitow.exe run --app <clave>` resuelve el
/// componente seleccionado en cada lanzamiento.
///
/// # Errors
///
/// `El componente debe estar activo en el profile`, los de la insignia, de
/// `application_target` y del icono, y los de E/S.
pub fn application_link(
    engine: &Engine,
    desktop: Desktop<'_>,
    profile: &str,
    installation: &str,
    options: &LinkOptions<'_>,
) -> Result<PathBuf> {
    let name = identifier(profile)?;
    let data = engine.profile(&name)?;
    let inactive = || {
        domain(Message::new(
            "El componente debe estar activo en el profile",
        ))
    };
    if !data.apps.iter().any(|app| app == installation) {
        return Err(inactive());
    }
    let selected = engine.selected(&data)?;
    let record = selected.get(installation).ok_or_else(inactive)?;
    let badge = badge_text(engine, options.badge)?;
    let color = badge_color(engine, options.color)?;
    let app = record.key.as_str();
    let tag = match options.shortcut {
        Some(shortcut) => {
            engine.application_target(record, Some(shortcut))?;
            tag(&format!("{name}/{app}/{shortcut}"))
        }
        None => tag(&format!("{name}/{app}")),
    };
    let launcher = format!("app-{tag}-{}", badge_tag(&badge));
    let source = component_icon_source(engine, record, &name, options.shortcut)?;
    let icon = application_icon(engine, &launcher, &badge, Some(&source), &color)?;
    let folder = desktop()?;
    std::fs::create_dir_all(&folder).map_err(|error| Error::io(&folder, error))?;
    let registry = engine
        .layout()
        .state()
        .join("application-shortcuts")
        .join(format!("{tag}.json"));
    let previous = registered_path(&registry)?;
    let mut stem = format!("{name} - {}", app.replace('/', "-"));
    if let Some(shortcut) = options.shortcut {
        // Las etiquetas pueden traer subcarpetas del menú Inicio de Scoop, nunca rutas.
        stem = format!("{stem} - {}", sanitize_label(shortcut));
    }
    let path = free_path(&folder, &stem, previous.as_deref());
    let link = Link {
        target: engine.layout().mitow_exe(),
        arguments: list2cmdline(&application_arguments(
            &name,
            app,
            options.coupled,
            options.shortcut,
            options.arguments,
        )),
        working_directory: Some(engine.root().to_path_buf()),
        icon: Some((icon, 0)),
    };
    write_desktop_link(&path, &link)?;
    write_json(
        &registry,
        &json!({
            "path": path.display().to_string(),
            "profile": name,
            "app": app,
            "badge": badge,
            "color": color,
            "coupled": options.coupled,
            "arguments": options.arguments,
            "shortcut": options.shortcut,
        }),
    )?;
    Ok(path)
}

/// Crea o actualiza el acceso a la terminal de MITO de un profile (`mito_link`): abre
/// la ventana de MITO con ese profile seleccionado.
///
/// # Errors
///
/// `Falta Windows Terminal portable…`, los de la insignia y los de E/S.
pub fn mito_link(
    engine: &Engine,
    desktop: Desktop<'_>,
    profile: &str,
    badge: Option<&str>,
    color: Option<&str>,
) -> Result<PathBuf> {
    let name = identifier(profile)?;
    engine.profile(&name)?;
    let badge = badge_text(engine, badge)?;
    let color = badge_color(engine, color)?;
    let icon = application_icon(engine, &format!("mito-{name}"), &badge, None, &color)?;
    let folder = desktop()?;
    std::fs::create_dir_all(&folder).map_err(|error| Error::io(&folder, error))?;
    let registry = engine
        .layout()
        .state()
        .join("mito-shortcuts")
        .join(format!("{name}.json"));
    let previous = registered_path(&registry)?;
    let path = free_path(&folder, &format!("MITO - {name}"), previous.as_deref());
    // Se abre la terminal privada directamente para que Windows asocie su ventana de la
    // barra de tareas con este acceso. El profile solo elige el estado inicial.
    prepare(engine, Some(&name))?;
    let argv = command(engine, None, Shell::Cmd, Some(&name))?;
    let (program, rest) = argv.split_first().ok_or_else(|| {
        domain(Message::new(
            "Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.",
        ))
    })?;
    let link = Link {
        target: PathBuf::from(program),
        arguments: list2cmdline(rest),
        working_directory: Some(engine.root().to_path_buf()),
        icon: Some((icon, 0)),
    };
    write_desktop_link(&path, &link)?;
    write_json(
        &registry,
        &json!({
            "path": path.display().to_string(),
            "profile": name,
            "badge": badge,
            "color": color,
        }),
    )?;
    Ok(path)
}

/// Crea o actualiza el acceso de escritorio de un ejecutable del host (`shortcut` de
/// `host_apps.py`) y devuelve la ruta del `.lnk`.
///
/// `identity` y `executable` son la identidad registrada del ejecutable y su ruta; el
/// destino es `mitow.exe host-run <identidad> --profile <p> [--coupled] [-- args]`, con
/// la carpeta del ejecutable como directorio de trabajo y su icono con insignia.
///
/// # Errors
///
/// Los de la insignia, del icono y de E/S.
pub fn host_link(
    engine: &Engine,
    desktop: Desktop<'_>,
    profile: &str,
    identity: &str,
    executable: &Path,
    options: &LinkOptions<'_>,
) -> Result<PathBuf> {
    let name = identifier(profile)?;
    engine.profile(&name)?;
    let badge = badge_text(engine, options.badge)?;
    let color = badge_color(engine, options.color)?;
    let tag = tag(&format!("{name}/{identity}"));
    let launcher = format!("host-{tag}-{}", badge_tag(&badge));
    let icon = application_icon(engine, &launcher, &badge, Some(executable), &color)?;
    let folder = desktop()?;
    std::fs::create_dir_all(&folder).map_err(|error| Error::io(&folder, error))?;
    let registry = engine
        .layout()
        .state()
        .join("host-shortcuts")
        .join(format!("{tag}.json"));
    let previous = registered_path(&registry)?;
    let stem = format!(
        "MITO - {name} - Host - {}",
        executable
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let path = free_path(&folder, &stem, previous.as_deref());
    let mut arguments = vec![
        "host-run".to_owned(),
        identity.to_owned(),
        "--profile".to_owned(),
        name.clone(),
    ];
    if options.coupled {
        arguments.push("--coupled".to_owned());
    }
    if !options.arguments.is_empty() {
        arguments.push("--".to_owned());
        arguments.extend(options.arguments.iter().cloned());
    }
    let link = Link {
        target: engine.layout().mitow_exe(),
        arguments: list2cmdline(&arguments),
        working_directory: executable.parent().map(Path::to_path_buf),
        icon: Some((icon, 0)),
    };
    write_desktop_link(&path, &link)?;
    write_json(
        &registry,
        &json!({
            "path": path.display().to_string(),
            "badge": badge,
            "color": color,
            "profile": name,
            "application": identity,
            "coupled": options.coupled,
            "arguments": options.arguments,
        }),
    )?;
    Ok(path)
}

fn invalid_link() -> Error {
    domain(Message::new("Acceso registrado no válido"))
}

/// Reapunta los accesos registrados de un componente a su versión publicada
/// (`migrate_application_links`), conservando nombre, insignia y color.
///
/// `backups` acumula el contenido previo de cada archivo tocado (o `None` si no
/// existía) para poder deshacerlo. Los accesos de profiles borrados se conservan sin
/// modificar.
///
/// # Errors
///
/// `Acceso registrado no válido` y los del icono y de E/S.
pub fn migrate_application_links(
    engine: &Engine,
    key: &str,
    record: &Record,
    backups: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
    log: Log<'_>,
) -> Result<()> {
    let folder = engine.layout().state().join("application-shortcuts");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Ok(());
    };
    let mut registries: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json") && path.is_file())
        .collect();
    registries.sort();
    for registry in registries {
        let data: Value = read_json(&registry)?;
        let owner = data.get("app").or_else(|| data.get("installation"));
        if owner.and_then(Value::as_str) != Some(key) {
            continue;
        }
        let profile = data
            .get("profile")
            .and_then(Value::as_str)
            .ok_or_else(invalid_link)?;
        if !engine.profiles().iter().any(|name| name == profile) {
            log(LogLine::from(
                Message::new("Acceso de profile eliminado conservado sin modificar: {p1}")
                    .arg("p1", profile),
            ));
            continue;
        }
        let path = PathBuf::from(
            data.get("path")
                .and_then(Value::as_str)
                .ok_or_else(invalid_link)?,
        );
        if !path.is_file() {
            continue;
        }
        let is_link = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        let symlink = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
        if !is_link || symlink {
            return Err(invalid_link());
        }
        retarget(engine, record, &registry, &data, &path, backups)?;
    }
    Ok(())
}

/// Reescribe el `.lnk` de un registro de `application-shortcuts` para la versión
/// publicada y actualiza su registro.
fn retarget(
    engine: &Engine,
    record: &Record,
    registry: &Path,
    data: &Value,
    path: &Path,
    backups: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> Result<()> {
    {
        let folder = engine.layout().state().join("application-shortcuts");
        let profile = data
            .get("profile")
            .and_then(Value::as_str)
            .ok_or_else(invalid_link)?;
        let app = record.key.as_str();
        let shortcut = data.get("shortcut").and_then(Value::as_str);
        let tag = match shortcut {
            Some(shortcut) => {
                engine.application_target(record, Some(shortcut))?;
                tag(&format!("{profile}/{app}/{shortcut}"))
            }
            None => tag(&format!("{profile}/{app}")),
        };
        let new_registry = folder.join(format!("{tag}.json"));
        // Un acceso creado aparte para la versión nueva sigue registrado.
        let destination_registry = if new_registry.exists() {
            registry.to_path_buf()
        } else {
            new_registry
        };
        for item in [path, registry, destination_registry.as_path()] {
            backups
                .entry(item.to_path_buf())
                .or_insert_with(|| std::fs::read(item).ok());
        }
        let badge = badge_text(engine, data.get("badge").and_then(Value::as_str))?;
        let color = badge_color(
            engine,
            Some(
                data.get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("naranja"),
            ),
        )?;
        let source = component_icon_source(engine, record, profile, shortcut)?;
        let icon = application_icon(
            engine,
            &format!("app-{tag}-{}", badge_tag(&badge)),
            &badge,
            Some(&source),
            &color,
        )?;
        let arguments: Vec<String> = match data.get("arguments") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or_else(invalid_link)?,
            Some(_) => return Err(invalid_link()),
        };
        let coupled = data
            .get("coupled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let link = Link {
            target: engine.layout().mitow_exe(),
            arguments: list2cmdline(&application_arguments(
                profile, app, coupled, shortcut, &arguments,
            )),
            working_directory: Some(engine.root().to_path_buf()),
            icon: Some((icon, 0)),
        };
        write_desktop_link(path, &link)?;
        let mut updated = data.clone();
        if let Value::Object(map) = &mut updated {
            map.insert("app".to_owned(), Value::String(app.to_owned()));
            map.shift_remove("installation");
        }
        write_json(&destination_registry, &updated)?;
        if destination_registry != registry {
            std::fs::remove_file(registry).map_err(|error| Error::io(registry, error))?;
        }
    }
    Ok(())
}

/// Parte del punto de extensión `host` del motor que reescribe los accesos de las
/// aplicaciones publicadas (F8). El binario la compone con el resto del host.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShortcutHost;

impl Host for ShortcutHost {
    fn migrate_application_links(
        &self,
        engine: &Engine,
        key: &str,
        record: &Record,
        links: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
        log: Log<'_>,
    ) -> Result<()> {
        migrate_application_links(engine, key, record, links, log)
    }
}

#[cfg(test)]
mod tests;
