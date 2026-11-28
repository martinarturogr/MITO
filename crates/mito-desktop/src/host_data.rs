//! Carpetas de datos del host enlazadas dentro del `AppData` portable (`host_data.py`).
//!
//! `data` es `%APPDATA%` (Roaming) y `cache` es `%LOCALAPPDATA%` (Local). Cada enlace es
//! un junction creado en `home/AppData/<Roaming|Local>/<relativo>` que apunta a la
//! carpeta del host. Los enlaces son de instancia: los comparten todos los profiles.
//! Es el sentido inverso de [`crate::host_exposure`].
//!
//! Las carpetas reales del usuario llegan en un [`HostRoots`], que los tests sustituyen
//! por carpetas temporales para no tocar el `AppData` verdadero.
//!
//! Contrato: `docs/spec/modules/host_data.md`.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mito_core::storage::{portable_path, read_json, resolve_lenient, write_json};
use mito_core::{Layout, Message, MutationLock};
use mito_engine::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::host_apps;
use crate::native::{self, NativeError};

/// Registro de los enlaces de datos, bajo `core/state`.
pub const REGISTRY: &str = "core/state/host-data-links.json";

/// Categoría de datos del host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `%APPDATA%` (`Roaming`).
    Data,
    /// `%LOCALAPPDATA%` (`Local`).
    Cache,
}

impl Kind {
    /// Las dos categorías, en el orden de `KINDS` del original.
    pub const ALL: [Self; 2] = [Self::Data, Self::Cache];

    /// Clave en los registros y en la interfaz (`data`, `cache`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Cache => "cache",
        }
    }

    /// Carpeta bajo `AppData` (`Roaming`, `Local`).
    pub fn folder(self) -> &'static str {
        match self {
            Self::Data => "Roaming",
            Self::Cache => "Local",
        }
    }

    /// Categoría de una clave de registro; `None` si no es `data` ni `cache`.
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// Carpetas `AppData` reales del usuario del host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRoots {
    data: PathBuf,
    cache: PathBuf,
}

impl HostRoots {
    /// Raíces explícitas (`Roaming` y `Local`), para pruebas.
    pub fn new(data: impl Into<PathBuf>, cache: impl Into<PathBuf>) -> Self {
        Self {
            data: data.into(),
            cache: cache.into(),
        }
    }

    /// Las carpetas conocidas del usuario (`host_folder`). Siempre las del host, aunque
    /// el entorno del proceso apunte a `home/` de MITO.
    ///
    /// # Errors
    ///
    /// `No se pudo localizar AppData del host`.
    pub fn system() -> Result<Self> {
        let lookup = |folder| {
            mito_win::shell::known_folder(folder)
                .map_err(|_| Error::domain(Message::new("No se pudo localizar AppData del host")))
        };
        Ok(Self::new(
            lookup(mito_win::shell::KnownFolder::RoamingAppData)?,
            lookup(mito_win::shell::KnownFolder::LocalAppData)?,
        ))
    }

    /// `AppData` del host para `kind`.
    pub fn folder(&self, kind: Kind) -> &Path {
        match kind {
            Kind::Data => &self.data,
            Kind::Cache => &self.cache,
        }
    }
}

/// Enlace registrado: carpeta relativa en `home/AppData/<kind>` y carpeta del host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Ruta relativa bajo `home/AppData/Roaming` o `Local` (separador `\`).
    pub relative: String,
    /// Carpeta del host a la que apunta el junction, ya resuelta.
    pub target: String,
}

/// Qué hace falta para crear un junction (`destino`, `enlace`); los tests lo sustituyen.
pub(crate) type CreateJunction<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

// --- Utilidades de rutas compartidas con `host_apps` y `host_exposure` ------------------

/// `home/AppData` de la instancia, con separadores de Windows (aparece en mensajes).
pub(crate) fn appdata(root: &Path) -> PathBuf {
    root.join("home").join("AppData")
}

/// `home/AppData/<Roaming|Local>` de la instancia.
pub(crate) fn portable_base(root: &Path, kind: Kind) -> PathBuf {
    appdata(root).join(kind.folder())
}

/// `str.casefold()`.
pub(crate) fn fold(text: &str) -> String {
    caseless::default_case_fold_str(text)
}

/// Componentes de `path` sin mayúsculas, sin `.` y sin el prefijo `\\?\`.
fn components(path: &Path) -> Vec<String> {
    dunce::simplified(path)
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| fold(&c.as_os_str().to_string_lossy()))
        .collect()
}

/// `PurePath.is_relative_to`: sin distinguir mayúsculas, por componentes.
pub(crate) fn is_relative_to(path: &Path, base: &Path) -> bool {
    let (path, base) = (components(path), components(base));
    path.len() >= base.len() && path[..base.len()] == base[..]
}

/// Igualdad de rutas sin distinguir mayúsculas (`PureWindowsPath.__eq__`).
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    components(a) == components(b)
}

/// Resto de `path` respecto a `base`, con las mayúsculas originales.
pub(crate) fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    if !is_relative_to(path, base) {
        return None;
    }
    let skip = components(base).len();
    let simplified = dunce::simplified(path);
    let mut rest = PathBuf::new();
    rest.extend(
        simplified
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .skip(skip),
    );
    Some(rest)
}

/// `Path.resolve()` sin exigir que exista.
pub(crate) fn resolve(path: &Path) -> PathBuf {
    resolve_lenient(path)
}

/// Nombre sin extensión (`Path.stem`).
pub(crate) fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Subcarpetas inmediatas de `dir`, ordenadas por nombre sin mayúsculas.
pub(crate) fn subfolders(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<(String, PathBuf)> = listing
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_dir() || mito_win::links::is_junction(&path))
                .then(|| (entry.file_name().to_string_lossy().into_owned(), path))
        })
        .collect();
    names.sort_by_key(|(name, _)| fold(name));
    names
}

/// Un enlace de cualquier tipo (junction o enlace simbólico).
pub(crate) fn is_link(path: &Path) -> bool {
    mito_win::links::is_junction(path) || mito_win::links::is_symlink(path)
}

/// Error de dominio con un mensaje.
pub(crate) fn fail<T>(message: Message) -> Result<T> {
    Err(Error::domain(message))
}

/// Mensaje con un argumento `{p1}` que es una ruta.
pub(crate) fn with_path(template: &'static str, path: &Path) -> Message {
    Message::new(template).arg("p1", path.display())
}

/// Lock global de mutaciones de la instancia (`mutation(root)`).
pub(crate) fn lock(root: &Path) -> Result<MutationLock> {
    Ok(MutationLock::mutation(&Layout::new(root), Duration::ZERO)?)
}

/// Registro JSON de dos niveles (identidad, categoría).
pub(crate) type Records = Map<String, Value>;

/// Lee un registro; vacío si el archivo no existe.
pub(crate) fn read_records(path: &Path) -> Result<Records> {
    if path.exists() {
        Ok(read_json(path)?)
    } else {
        Ok(Records::new())
    }
}

/// Elementos de `records[identity][kind]`.
pub(crate) fn items<T: DeserializeOwned>(records: &Records, identity: &str, kind: &str) -> Vec<T> {
    records
        .get(identity)
        .and_then(|app| app.get(kind))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Añade `item` a `records[identity][kind]` (`setdefault(...).append`).
pub(crate) fn push<T: Serialize>(records: &mut Records, identity: &str, kind: &str, item: &T) {
    let Ok(item) = serde_json::to_value(item) else {
        return;
    };
    let app = records
        .entry(identity.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(app) = app.as_object_mut()
        && let Some(list) = app
            .entry(kind.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
    {
        list.push(item);
    }
}

// --- Registro y rutas ----------------------------------------------------------------

fn registry_path(root: &Path) -> PathBuf {
    root.join(REGISTRY)
}

/// Registro de enlaces de datos (`core/state/host-data-links.json`), vacío si no existe.
///
/// # Errors
///
/// Los de lectura del JSON.
pub fn registry(root: &Path) -> Result<Map<String, Value>> {
    read_records(&registry_path(root))
}

/// Escribe el registro de enlaces de datos.
///
/// # Errors
///
/// Los de escritura del JSON.
pub fn save(root: &Path, records: &Map<String, Value>) -> Result<()> {
    Ok(write_json(&registry_path(root), records)?)
}

/// Carpeta portable `home/AppData/<Roaming|Local>/<relative>` validada (`destination`).
///
/// # Errors
///
/// `Ruta no portable` / `Ruta reservada` si `relative` no es válida, `Destino fuera del
/// AppData de MITO` y `AppData de MITO está redirigido fuera de la instancia`.
pub fn destination(root: &Path, kind: Kind, relative: &str) -> Result<PathBuf> {
    let base = portable_base(root, kind);
    // La sintaxis se valida sin resolver el último junction hacia el host.
    let portable = portable_path(relative)?;
    let path = portable.join_to(&base);
    let parent = path.parent().unwrap_or(&base);
    if portable.is_empty()
        || same_path(&path, &base)
        || !is_relative_to(&resolve(parent), &resolve(&base))
    {
        return fail(Message::new("Destino fuera del AppData de MITO"));
    }
    if !is_relative_to(&resolve(&base), &resolve(root)) {
        return fail(Message::new(
            "AppData de MITO está redirigido fuera de la instancia",
        ));
    }
    Ok(path)
}

/// Prepara el enlace de `folder`: devuelve (ruta portable, carpeta resuelta, relativa).
///
/// # Errors
///
/// `Selecciona una carpeta existente`, `Selecciona una carpeta de aplicación, no todo
/// AppData`, `Selecciona datos externos a MITO, sin enlaces circulares` y los de
/// [`destination`].
pub fn plan(
    root: &Path,
    host: &HostRoots,
    kind: Kind,
    folder: &Path,
) -> Result<(PathBuf, PathBuf, String)> {
    if !folder.is_absolute() || !folder.is_dir() {
        return fail(Message::new("Selecciona una carpeta existente"));
    }
    let folder = resolve(folder);
    let base = resolve(host.folder(kind));
    if same_path(&folder, &base) {
        return fail(Message::new(
            "Selecciona una carpeta de aplicación, no todo AppData",
        ));
    }
    let relative = relative_to(&folder, &base).map_or_else(
        || {
            folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        },
        |rest| rest.display().to_string(),
    );
    let path = destination(root, kind, &relative)?;
    let root = resolve(root);
    if is_relative_to(&folder, &root) || is_relative_to(&root, &folder) {
        return fail(Message::new(
            "Selecciona datos externos a MITO, sin enlaces circulares",
        ));
    }
    Ok((path, folder, relative))
}

/// Carpetas del host candidatas: su nombre contiene el del ejecutable (sin
/// mayúsculas) y el ejecutable no está dentro de ellas. No se desciende ni a las
/// candidatas ni a los enlaces.
///
/// # Errors
///
/// `La aplicación del host ya no está registrada en MITO`.
pub fn candidates(
    root: &Path,
    host: &HostRoots,
    identity: &str,
    kind: Kind,
) -> Result<Vec<PathBuf>> {
    let executable = resolve(&host_apps::target(root, identity)?);
    let word = fold(&stem(&executable));
    let mut found = Vec::new();
    walk_candidates(host.folder(kind), &word, &executable, &mut found);
    Ok(found)
}

fn walk_candidates(dir: &Path, word: &str, executable: &Path, found: &mut Vec<PathBuf>) {
    let mut descend = Vec::new();
    for (name, path) in subfolders(dir) {
        if is_link(&path) {
            continue;
        }
        if fold(&name).contains(word) {
            if !is_relative_to(executable, &resolve(&path)) {
                found.push(path);
            }
            continue;
        }
        descend.push(path);
    }
    for path in descend {
        walk_candidates(&path, word, executable, found);
    }
}

/// Enlaces de la aplicación: los registrados y los junctions sin registrar que cumplen
/// todas las condiciones del contrato. No escribe nada.
///
/// # Errors
///
/// Los de lectura del registro y del inventario.
pub fn entries(root: &Path, host: &HostRoots, identity: &str, kind: Kind) -> Result<Vec<Entry>> {
    let records = registry(root)?;
    let mut result: Vec<Entry> = items(&records, identity, kind.key());
    let Some(executable) = host_apps::inventory_target(root, identity)? else {
        return Ok(result);
    };
    let word = fold(&stem(&executable));
    let base = portable_base(root, kind);
    let host_base = resolve(host.folder(kind));
    let root_resolved = resolve(root);
    let claimed: BTreeSet<String> = records
        .keys()
        .flat_map(|owner| items::<Entry>(&records, owner, kind.key()))
        .map(|item| fold(&item.relative))
        .collect();
    // Solo se inspecciona el árbol portable; nunca se sigue un junction hacia el host.
    let mut pending = vec![base.clone()];
    while let Some(dir) = pending.pop() {
        let mut descend = Vec::new();
        for (_, path) in subfolders(&dir) {
            let Some(relative) = relative_to(&path, &base).map(|r| r.display().to_string()) else {
                continue;
            };
            if mito_win::links::is_junction(&path) {
                let named = Path::new(&relative)
                    .components()
                    .any(|part| fold(&part.as_os_str().to_string_lossy()).contains(&word));
                if !claimed.contains(&fold(&relative)) && named {
                    let expected = host_base.join(&relative);
                    if let Ok(checked) = destination(root, kind, &relative)
                        && expected.is_dir()
                        && same_path(&resolve(&checked), &resolve(&expected))
                        && !is_relative_to(&resolve(&expected), &root_resolved)
                    {
                        result.push(Entry {
                            relative,
                            target: resolve(&expected).display().to_string(),
                        });
                    }
                }
            } else if !mito_win::links::is_symlink(&path) {
                descend.push(path);
            }
        }
        // Orden de lectura: el primer subdirectorio se procesa primero.
        pending.extend(descend.into_iter().rev());
    }
    Ok(result)
}

/// ¿Hay algún enlace de `kind` vivo? (el que activa «Data · ✓» / «Cache · ✓»).
///
/// # Errors
///
/// Los de [`entries`] y [`destination`].
pub fn linked(root: &Path, host: &HostRoots, identity: &str, kind: Kind) -> Result<bool> {
    for item in entries(root, host, identity, kind)? {
        let path = destination(root, kind, &item.relative)?;
        if mito_win::links::is_junction(&path)
            && same_path(&resolve(&path), &resolve(Path::new(&item.target)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn create_real(target: &Path, link: &Path) -> io::Result<()> {
    mito_win::links::create_junction(target, link)
}

/// Enlaza `folder` (carpeta del host) en el `AppData` portable (`link`).
///
/// Con `replace`, una carpeta local existente en la ruta portable se aparta a
/// `core/staging/host-data-<uuid>` y se borra si el enlace se crea; si falla, se
/// restaura. Nunca se sustituye un junction o enlace simbólico ajeno.
///
/// # Errors
///
/// `Esta ruta ya está gestionada por una aplicación del host`, `No se reemplazan enlaces
/// ajenos a MITO`, `La carpeta {p1} ya existe y ha de ser borrada para generar el
/// enlace`, `El destino existente no es una carpeta`, `Copia temporal fuera de staging`
/// y los de [`plan`], [`registry`] y E/S.
pub fn link(
    root: &Path,
    host: &HostRoots,
    identity: &str,
    kind: Kind,
    folder: &Path,
    replace: bool,
) -> Result<()> {
    link_with(root, host, identity, kind, folder, replace, &create_real)
}

pub(crate) fn link_with(
    root: &Path,
    host: &HostRoots,
    identity: &str,
    kind: Kind,
    folder: &Path,
    replace: bool,
    create: CreateJunction<'_>,
) -> Result<()> {
    let _lock = lock(root)?;
    host_apps::target(root, identity)?;
    let (path, folder, relative) = plan(root, host, kind, folder)?;
    let mut records = registry(root)?;
    for owner in records.keys() {
        for category in Kind::ALL {
            for item in items::<Entry>(&records, owner, category.key()) {
                let other = destination(root, category, &item.relative)?;
                if same_path(&path, &other)
                    || is_relative_to(&path, &other)
                    || is_relative_to(&other, &path)
                {
                    if owner == identity
                        && category == kind
                        && same_path(&path, &other)
                        && mito_win::links::is_junction(&path)
                        && same_path(&resolve(&path), &folder)
                        && same_path(&resolve(Path::new(&item.target)), &folder)
                    {
                        return Ok(());
                    }
                    return fail(Message::new(
                        "Esta ruta ya está gestionada por una aplicación del host",
                    ));
                }
            }
        }
    }
    let entry = Entry {
        relative,
        target: folder.display().to_string(),
    };
    if mito_win::links::is_junction(&path) && same_path(&resolve(&path), &folder) {
        push(&mut records, identity, kind.key(), &entry);
        return save(root, &records);
    }
    if is_link(&path) {
        return fail(Message::new("No se reemplazan enlaces ajenos a MITO"));
    }
    if path.exists() && !replace {
        return fail(with_path(
            "La carpeta {p1} ya existe y ha de ser borrada para generar el enlace",
            &path,
        ));
    }
    if path.exists() && !path.is_dir() {
        return fail(Message::new("El destino existente no es una carpeta"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let appdata = appdata(root);
    let staging = root.join("core").join("staging");
    let mut backup = None;
    if path.exists() {
        if !is_relative_to(&resolve(&path), &resolve(&appdata)) {
            return fail(Message::new("Destino fuera del AppData de MITO"));
        }
        let moved = staging.join(format!("host-data-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&staging).map_err(|e| Error::io(&staging, e))?;
        fs::rename(&path, &moved).map_err(|e| Error::io(&path, e))?;
        backup = Some(moved);
    }
    let created = create(&folder, &path)
        .map_err(|e| Error::io(&path, e))
        .and_then(|()| {
            push(&mut records, identity, kind.key(), &entry);
            save(root, &records)
        });
    if let Err(error) = created {
        if mito_win::links::is_junction(&path) {
            let _ = mito_win::links::remove_junction(&path);
        }
        if let Some(moved) = &backup {
            let _ = fs::rename(moved, &path);
        }
        return Err(error);
    }
    if let Some(moved) = backup {
        if !is_relative_to(&resolve(&moved), &resolve(&staging)) {
            return fail(Message::new("Copia temporal fuera de staging"));
        }
        fs::remove_dir_all(&moved).map_err(|e| Error::io(&moved, e))?;
    }
    Ok(())
}

/// Quita los enlaces de `kind` de la aplicación con el lock ya tomado (lo usa
/// [`host_apps::remove`]). Solo se borra el junction; los datos del host no se tocan.
///
/// # Errors
///
/// `El enlace ha cambiado fuera de MITO: {p1}` y `Se esperaba una junction gestionada:
/// {p1}`, antes de borrar nada más.
pub fn unlink_locked(root: &Path, host: &HostRoots, identity: &str, kind: Kind) -> Result<()> {
    let mut records = registry(root)?;
    for item in entries(root, host, identity, kind)? {
        let path = destination(root, kind, &item.relative)?;
        if mito_win::links::is_junction(&path) {
            if !same_path(&resolve(&path), &resolve(Path::new(&item.target))) {
                return fail(with_path(
                    "El enlace ha cambiado fuera de MITO: {p1}",
                    &path,
                ));
            }
            // Nunca se recorre ni se borra el destino del host.
            mito_win::links::remove_junction(&path).map_err(|e| Error::io(&path, e))?;
        } else if path.exists() || mito_win::links::is_symlink(&path) {
            return fail(with_path(
                "Se esperaba una junction gestionada: {p1}",
                &path,
            ));
        }
    }
    if let Some(app) = records.get_mut(identity).and_then(Value::as_object_mut) {
        app.remove(kind.key());
        if app.is_empty() {
            records.remove(identity);
        }
    }
    save(root, &records)
}

/// Quita los enlaces de `kind` de la aplicación, dentro del lock de mutaciones.
///
/// # Errors
///
/// Los de [`unlink_locked`] y del lock.
pub fn unlink(root: &Path, host: &HostRoots, identity: &str, kind: Kind) -> Result<()> {
    let _lock = lock(root)?;
    unlink_locked(root, host, identity, kind)
}

/// Selector nativo de carpeta que empieza en el `AppData` del host de `kind`. `None` si
/// se cancela.
///
/// # Errors
///
/// Los de [`native::choose_folder`].
pub fn choose_folder(
    host: &HostRoots,
    kind: Kind,
    translate: &dyn Fn(&str) -> String,
) -> std::result::Result<Option<PathBuf>, NativeError> {
    native::choose_folder(host.folder(kind), translate)
}

#[cfg(test)]
mod tests;
