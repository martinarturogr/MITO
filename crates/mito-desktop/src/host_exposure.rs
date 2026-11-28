//! Carpetas de datos de MITO expuestas en el `AppData` del host (`host_exposure.py`).
//!
//! Es la inversa de [`crate::host_data`]: crea junctions en el `AppData` del host que
//! apuntan a `home/AppData/<Roaming|Local>/<relativo>`, de modo que una aplicación que
//! arranca fuera de MITO (inicio automático, asociaciones de archivos) usa los datos de
//! MITO. Solo se comparten datos, no el entorno del profile. Una carpeta del host que ya
//! existe se aparta (se renombra) y se restaura al desenlazar; nunca se borra.
//!
//! Contrato: `docs/spec/modules/host_exposure.md`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mito_core::storage::{portable_path, write_json};
use mito_core::{Layout, Message, MutationLock};
use mito_engine::{Engine, Error, Record, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::host_data::{
    CreateJunction, HostRoots, Kind, destination, fail, fold, is_link, is_relative_to, items,
    portable_base, push, read_records, resolve, same_path, stem, subfolders, with_path,
};

/// Registro de las exposiciones, bajo `core/state`.
pub const REGISTRY: &str = "core/state/host-exposed-links.json";

/// Exposición registrada de una carpeta de datos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exposure {
    /// Ruta bajo `home/AppData/<Roaming|Local>` (la misma en MITO y en el host).
    pub relative: String,
    /// Carpeta del host apartada, o `None` si no había carpeta previa.
    pub backup: Option<String>,
}

fn registry_path(root: &Path) -> PathBuf {
    root.join(REGISTRY)
}

/// Registro de exposiciones (`core/state/host-exposed-links.json`), vacío si no existe.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn registry(root: &Path) -> Result<Map<String, Value>> {
    read_records(&registry_path(root))
}

/// Escribe el registro de exposiciones.
///
/// # Errors
///
/// Los de escritura del JSON.
pub fn save(root: &Path, records: &Map<String, Value>) -> Result<()> {
    Ok(write_json(&registry_path(root), records)?)
}

/// Normaliza `\` y elimina `.`, `..` y separadores repetidos, de forma léxica
/// (`os.path.normpath`).
fn normpath(value: &str) -> String {
    let value = value.replace('/', "\\");
    let (prefix, rest) = if let Some(rest) = value.strip_prefix("\\\\") {
        ("\\\\", rest)
    } else if value.as_bytes().get(1) == Some(&b':') {
        value.split_at(2)
    } else {
        ("", value.as_str())
    };
    let rooted = rest.starts_with('\\');
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let body = parts.join("\\");
    let root = if rooted { "\\" } else { "" };
    if prefix.is_empty() && root.is_empty() && body.is_empty() {
        ".".to_owned()
    } else {
        format!("{prefix}{root}{body}")
    }
}

/// Ruta normalizada para comparar: sin `\\?\` ni `\??\`, `normpath` y en minúsculas
/// (`normcase`).
pub fn normal(path: &Path) -> String {
    let text = path.display().to_string();
    let text = ["\\\\?\\", "\\??\\"]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .unwrap_or(&text);
    normpath(text).to_lowercase()
}

/// Destino normalizado de un junction, sin resolverlo (puede estar roto), o `None` si no
/// lo es.
pub fn junction_target(link: &Path) -> Option<String> {
    if !mito_win::links::is_junction(link) {
        return None;
    }
    mito_win::links::junction_target(link)
        .ok()
        .map(|target| normal(&target))
}

/// ¿Es `link` un junction que apunta a `folder`? El destino no se resuelve.
pub fn points_to(link: &Path, folder: &Path) -> bool {
    junction_target(link).is_some_and(|target| target == normal(folder))
}

/// Nombres (sin mayúsculas, de 3 o más caracteres) de los ejecutables declarados por el
/// componente, ordenados y sin repetir.
///
/// # Errors
///
/// Los de [`Engine::application_entries`].
pub fn words(engine: &Engine, record: &Record) -> Result<Vec<String>> {
    let mut names: Vec<String> = engine
        .application_entries(record)?
        .iter()
        .map(|entry| fold(&stem(Path::new(&entry.executable))))
        .filter(|name| name.chars().count() >= 3)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Carpetas reales del `AppData` de MITO con el nombre de un ejecutable declarado:
/// `(categoría, ruta relativa)`. No se descienden las coincidencias ni los enlaces.
///
/// # Errors
///
/// Los de [`words`].
pub fn candidates(engine: &Engine, record: &Record) -> Result<Vec<(Kind, String)>> {
    let names = words(engine, record)?;
    let mut found = Vec::new();
    if names.is_empty() {
        return Ok(found);
    }
    for kind in Kind::ALL {
        let base = portable_base(engine.root(), kind);
        if base.is_dir() {
            walk(&base, &base, &names, kind, &mut found);
        }
    }
    Ok(found)
}

fn walk(base: &Path, dir: &Path, names: &[String], kind: Kind, found: &mut Vec<(Kind, String)>) {
    let mut descend = Vec::new();
    for crate::host_data::Folder { name, path, link } in subfolders(dir) {
        if link {
            continue;
        }
        if names.iter().any(|word| fold(&name).contains(word.as_str())) {
            let relative = path.strip_prefix(base).unwrap_or(&path);
            found.push((kind, relative.display().to_string()));
        } else {
            descend.push(path);
        }
    }
    for path in descend {
        walk(base, &path, names, kind, found);
    }
}

/// Carpeta de MITO y ruta en el host para una carpeta relativa del `AppData`: `(origen,
/// destino)`.
///
/// # Errors
///
/// `No se reemplazan enlaces ajenos a MITO`, `Se esperaba una carpeta de datos en MITO:
/// {p1}`, `Destino fuera del AppData de MITO`, `Selecciona datos externos a MITO, sin
/// enlaces circulares` y los de [`destination`].
pub fn plan(
    root: &Path,
    host: &HostRoots,
    kind: Kind,
    relative: &str,
) -> Result<(PathBuf, PathBuf)> {
    let source = destination(root, kind, relative)?;
    let base = portable_base(root, kind);
    let mut current = source.clone();
    while !same_path(&current, &base) {
        if is_link(&current) {
            return fail(Message::new("No se reemplazan enlaces ajenos a MITO"));
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    if !source.is_dir() {
        return fail(with_path(
            "Se esperaba una carpeta de datos en MITO: {p1}",
            &source,
        ));
    }
    let host_base = host.folder(kind);
    let host_path = portable_path(relative)?.join_to(host_base);
    let outside = same_path(&host_path, host_base)
        || !host_path
            .parent()
            .is_some_and(|parent| is_relative_to(&resolve(parent), &resolve(host_base)));
    if outside {
        return fail(Message::new("Destino fuera del AppData de MITO"));
    }
    let root = resolve(root);
    if is_relative_to(&resolve(host_base), &root) || is_relative_to(&root, &resolve(&host_path)) {
        return fail(Message::new(
            "Selecciona datos externos a MITO, sin enlaces circulares",
        ));
    }
    Ok((source, host_path))
}

/// Carpeta hermana donde se aparta `host`: `<nombre>.mito-backup-AAAAMMDD-HHMMSS`, con la
/// hora local.
pub fn backup_path(host: &Path) -> PathBuf {
    let stamp = jiff::ZonedDateTime::now().strftime("%Y%m%d-%H%M%S");
    backup_path_at(host, &stamp.to_string())
}

/// Como [`backup_path`] con la marca de tiempo indicada.
pub fn backup_path_at(host: &Path, stamp: &str) -> PathBuf {
    let name = host
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    host.with_file_name(format!("{name}.mito-backup-{stamp}"))
}

fn entries_of(records: &Map<String, Value>, installation: &str) -> Vec<(Kind, Exposure)> {
    let Some(app) = records.get(installation).and_then(Value::as_object) else {
        return Vec::new();
    };
    app.keys()
        .filter_map(|key| Kind::parse(key))
        .flat_map(|kind| {
            items::<Exposure>(records, installation, kind.key())
                .into_iter()
                .map(move |item| (kind, item))
        })
        .collect()
}

/// Exposiciones registradas de la instalación. No descubre junctions.
///
/// # Errors
///
/// Los de lectura del registro.
pub fn entries(root: &Path, installation: &str) -> Result<Vec<(Kind, Exposure)>> {
    Ok(entries_of(&registry(root)?, installation))
}

/// ¿Hay alguna exposición viva de la instalación? (el botón «Host · ✓»). Los errores de
/// [`plan`] por entrada se ignoran.
///
/// # Errors
///
/// Los de lectura del registro.
pub fn linked(root: &Path, host: &HostRoots, installation: &str) -> Result<bool> {
    for (kind, item) in entries(root, installation)? {
        if let Ok((source, host_path)) = plan(root, host, kind, &item.relative)
            && points_to(&host_path, &source)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn create_real(target: &Path, link: &Path) -> io::Result<()> {
    mito_win::links::create_junction(target, link)
}

fn lock(layout: &Layout) -> Result<MutationLock> {
    Ok(MutationLock::mutation(layout, Duration::ZERO)?)
}

/// Crea en el host el junction hacia la carpeta de datos de MITO.
///
/// Devuelve la carpeta apartada si `aside` y el host ya tenía una carpeta real, o `None`.
/// Es idempotente si el junction ya apunta a MITO.
///
/// # Errors
///
/// `El componente ya no está instalado`, `Esta ruta del host ya está gestionada por
/// MITO`, `No se reemplazan enlaces ajenos a MITO`, `La carpeta del host ya existe:
/// {p1}` y los de [`plan`] y E/S; si falla la creación se restaura la carpeta apartada.
pub fn link(
    engine: &Engine,
    host: &HostRoots,
    installation: &str,
    kind: Kind,
    relative: &str,
    aside: bool,
) -> Result<Option<PathBuf>> {
    link_with(
        engine,
        host,
        installation,
        kind,
        relative,
        aside,
        &create_real,
    )
}

pub(crate) fn link_with(
    engine: &Engine,
    host: &HostRoots,
    installation: &str,
    kind: Kind,
    relative: &str,
    aside: bool,
    create: CreateJunction<'_>,
) -> Result<Option<PathBuf>> {
    let root = engine.root();
    let _lock = lock(engine.layout())?;
    if !engine.installed()?.contains(installation) {
        return fail(Message::new("El componente ya no está instalado"));
    }
    let (source, host_path) = plan(root, host, kind, relative)?;
    let mut records = registry(root)?;
    for owner in records.keys() {
        for category in Kind::ALL {
            for item in items::<Exposure>(&records, owner, category.key()) {
                let other = portable_path(&item.relative)?.join_to(host.folder(category));
                if same_path(&host_path, &other)
                    || is_relative_to(&host_path, &other)
                    || is_relative_to(&other, &host_path)
                {
                    if owner == installation
                        && category == kind
                        && same_path(&host_path, &other)
                        && points_to(&host_path, &source)
                    {
                        return Ok(None);
                    }
                    return fail(Message::new(
                        "Esta ruta del host ya está gestionada por MITO",
                    ));
                }
            }
        }
    }
    let record = |backup: Option<&Path>| Exposure {
        relative: relative.to_owned(),
        backup: backup.map(|b| b.display().to_string()),
    };
    if points_to(&host_path, &source) {
        push(&mut records, installation, kind.key(), &record(None));
        save(root, &records)?;
        return Ok(None);
    }
    if is_link(&host_path) {
        return fail(Message::new("No se reemplazan enlaces ajenos a MITO"));
    }
    let mut moved = None;
    if host_path.exists() {
        if !aside || !host_path.is_dir() {
            return fail(with_path("La carpeta del host ya existe: {p1}", &host_path));
        }
        let backup = backup_path(&host_path);
        // Los datos del host nunca se borran: se restauran al desenlazar.
        fs::rename(&host_path, &backup).map_err(|e| Error::io(&host_path, e))?;
        moved = Some(backup);
    }
    let created = host_path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| create(&source, &host_path))
        .map_err(|e| Error::io(&host_path, e))
        .and_then(|()| {
            push(
                &mut records,
                installation,
                kind.key(),
                &record(moved.as_deref()),
            );
            save(root, &records)
        });
    if let Err(error) = created {
        if mito_win::links::is_junction(&host_path) {
            let _ = mito_win::links::remove_junction(&host_path);
        }
        if let Some(backup) = &moved {
            let _ = fs::rename(backup, &host_path);
        }
        return Err(error);
    }
    Ok(moved)
}

/// Quita las exposiciones de la instalación con el lock ya tomado. Solo se borran los
/// junctions y se restauran las carpetas apartadas; nunca datos.
///
/// # Errors
///
/// `El enlace ha cambiado fuera de MITO: {p1}` y `Se esperaba una junction gestionada:
/// {p1}`.
pub fn unlink_locked(root: &Path, host: &HostRoots, installation: &str) -> Result<()> {
    let mut records = registry(root)?;
    for (kind, item) in entries_of(&records, installation) {
        // La carpeta de MITO puede haber desaparecido: el junction se reconoce por su destino.
        let source = destination(root, kind, &item.relative)?;
        let host_path = portable_path(&item.relative)?.join_to(host.folder(kind));
        if mito_win::links::is_junction(&host_path) {
            if !points_to(&host_path, &source) {
                return fail(with_path(
                    "El enlace ha cambiado fuera de MITO: {p1}",
                    &host_path,
                ));
            }
            // Quita solo el junction, nunca los datos de MITO.
            mito_win::links::remove_junction(&host_path).map_err(|e| Error::io(&host_path, e))?;
        } else if host_path.exists() || mito_win::links::is_symlink(&host_path) {
            return fail(with_path(
                "Se esperaba una junction gestionada: {p1}",
                &host_path,
            ));
        }
        if let Some(backup) = item.backup.as_deref().map(Path::new)
            && backup.is_dir()
            && !host_path.exists()
        {
            fs::rename(backup, &host_path).map_err(|e| Error::io(backup, e))?;
        }
    }
    if records.shift_remove(installation).is_some() {
        save(root, &records)?;
    }
    Ok(())
}

/// Quita las exposiciones de la instalación, dentro del lock de mutaciones.
///
/// # Errors
///
/// Los de [`unlink_locked`] y del lock.
pub fn unlink(root: &Path, host: &HostRoots, installation: &str) -> Result<()> {
    let _lock = lock(&Layout::new(root))?;
    unlink_locked(root, host, installation)
}

/// Retira la exposición al host de una instalación eliminada
/// (`Host::unregister_installation` del motor): quita sus junctions y restaura las
/// carpetas apartadas.
///
/// El motor registra el fallo sin deshacer la eliminación del componente. Es
/// [`unlink`], con este nombre para el punto de extensión.
///
/// # Errors
///
/// Los de [`unlink`].
pub fn unregister_installation(root: &Path, host: &HostRoots, installation: &str) -> Result<()> {
    unlink(root, host, installation)
}

/// Reapunta los junctions tras mover la instancia o suelta los de una copia: devuelve
/// cuántos enlaces se vieron afectados.
///
/// Con `copied` falso se recrea cada junction hacia la nueva ruta; con `copied` verdadero
/// el enlace se suelta del registro y el junction sigue apuntando a la instancia original.
/// Los fallos por enlace se informan por `log` y no abortan. Solo se guarda el registro
/// si algo cambió.
pub fn relocate(
    root: &Path,
    host: &HostRoots,
    old: &Path,
    copied: bool,
    log: &dyn Fn(Message),
) -> usize {
    let Ok(mut records) = registry(root) else {
        return 0;
    };
    let old_root = normal(old);
    let mut changed = 0;
    for installation in records.keys().cloned().collect::<Vec<_>>() {
        let mut kinds = Map::new();
        for kind in Kind::ALL {
            let mut kept = Vec::new();
            for item in items::<Exposure>(&records, &installation, kind.key()) {
                let host_path = portable_path(&item.relative).map_or_else(
                    |_| host.folder(kind).join(&item.relative),
                    |p| p.join_to(host.folder(kind)),
                );
                let moved_here = junction_target(&host_path)
                    .is_some_and(|t| t == old_root || t.starts_with(&format!("{old_root}\\")));
                if !moved_here {
                    kept.push(item);
                    continue;
                }
                if copied {
                    changed += 1;
                    continue;
                }
                let retarget = plan(root, host, kind, &item.relative).and_then(|(source, path)| {
                    mito_win::links::remove_junction(&path).map_err(|e| Error::io(&path, e))?;
                    create_real(&source, &path).map_err(|e| Error::io(&path, e))?;
                    Ok(path)
                });
                match retarget {
                    Ok(path) => {
                        log(with_path(
                            "Enlaces del host actualizados a la nueva ruta: {p1}",
                            &path,
                        ));
                        changed += 1;
                    }
                    Err(error) => log(Message::new(
                        "No se pudo actualizar el enlace del host {p1}: {p3}",
                    )
                    .arg("p1", host_path.display())
                    .arg("p3", error)),
                }
                kept.push(item);
            }
            if !kept.is_empty() {
                kinds.insert(
                    kind.key().to_owned(),
                    Value::Array(
                        kept.iter()
                            .filter_map(|item| serde_json::to_value(item).ok())
                            .collect(),
                    ),
                );
            }
        }
        if kinds.is_empty() {
            records.shift_remove(&installation);
        } else {
            records.insert(installation, Value::Object(kinds));
        }
    }
    if changed > 0 {
        let _ = save(root, &records);
    }
    changed
}

#[cfg(test)]
mod tests;
