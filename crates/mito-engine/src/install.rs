//! Plan e instalación de componentes (`engine.py::installation_plan/install/_install_payload`).
//!
//! Todo el plan (dependencias, compatibilidad, pasos no soportados, diseño de datos) se
//! calcula y valida antes de descargar nada. Cada payload se prepara en
//! `core/staging` y solo `publish_plan` lo mueve a su ruta estable.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mito_catalog::pyjson::{py_str, py_truthy};
use mito_catalog::{Manifest, array, compatibility, fingerprint};
use mito_core::diagnostics::{self, LogLevel};
use mito_core::storage::{identifier, inside, write_json};
use mito_core::{Message, Operation};
use mito_transfer::archives::{ExtractOptions, Extraction};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::environment::bins;
use crate::error::{Error, Result, bail, io_error, step};
use crate::fsutil::{copy_file, create_dir_all, is_link};
use crate::profiles::Profile;
use crate::record::{Record, Records, effective};
use crate::{Log, LogLine};

/// Opciones de [`Engine::install`].
#[derive(Default)]
pub struct InstallOptions<'a> {
    /// Profile al que se añaden los componentes (y sus dependencias).
    pub profile: Option<&'a str>,
    /// Manifiestos que sustituyen al del catálogo (versión elegida), por clave.
    pub manifests: BTreeMap<String, Value>,
    /// Retenciones por clave (`pinned`).
    pub pins: BTreeMap<String, Value>,
    /// Confirmación del plan completo (`Importación cancelada` si devuelve `false`).
    pub confirm_plan: Option<&'a dyn Fn(&Records) -> bool>,
    /// Confirmación de las descargas sin hash. Sin callback, o si devuelve `false`, la
    /// instalación se cancela antes de descargar.
    pub confirm_unverified: Option<crate::Confirm<'a>>,
}

/// Plan de instalación calculado sin descargar nada.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Registros a instalar, dependencias primero.
    pub records: Records,
    /// Inventario actual.
    pub current: Records,
    /// Profile destino con los ids añadidos en memoria (sin guardar).
    pub data: Option<Profile>,
    /// Descargas sin hash y recursos remotos que requieren confirmación.
    pub unverified: Vec<String>,
    /// Origen de los recursos de bucket de cada componente que cambia.
    pub resource_sources: BTreeMap<String, Vec<(String, String)>>,
    /// Todos los profiles tal como estaban al planificar.
    pub profiles: BTreeMap<String, Profile>,
}

struct Planner<'a> {
    engine: &'a Engine,
    catalog: mito_catalog::Catalog,
    current: &'a Records,
    manifests: &'a BTreeMap<String, Value>,
    pins: &'a BTreeMap<String, Value>,
    log: Log<'a>,
    retained: BTreeMap<String, Value>,
    visiting: BTreeSet<String>,
    visited: BTreeSet<String>,
    plan: Vec<(String, Value)>,
}

impl Planner<'_> {
    fn resolve(&mut self, name: &str) -> Result<()> {
        let (key, manifest) = self.catalog.resolve(name)?;
        let mut manifest = manifest.into_raw();
        if let Some(chosen) = self.manifests.get(&key) {
            manifest = chosen.clone();
            if let Some(pin) = self.pins.get(&key) {
                self.retained.insert(key.clone(), pin.clone());
            }
        } else if let Some(pinned) = self.current.get(&key).and_then(|c| c.pinned.clone()) {
            manifest = self
                .current
                .get(&key)
                .map(|c| c.manifest.clone())
                .unwrap_or(manifest);
            self.retained.insert(key.clone(), pinned);
        }
        if self.visiting.contains(&key) {
            return bail(Message::new("Dependencia circular: {p1}").arg("p1", &key));
        }
        if self.visited.contains(&key) {
            return Ok(());
        }
        let effective = effective(&manifest)?;
        if effective.get("post_install").is_some_and(py_truthy) {
            (self.log)(
                Message::new("{p0}: {p2} omitido por la política general de MITO")
                    .arg("p0", &key)
                    .arg("p2", "post_install")
                    .into(),
            );
        }
        let status = compatibility(&key, &Manifest::new(manifest.clone()));
        if !status.is_compatible() {
            return Err(Error::untranslated(format!("{key}: {}", status.text())));
        }
        if let Some(field) = self.engine.extensions().hooks.unsupported(&effective) {
            return bail(Message::new("Pasos no soportados: ").with_suffix(field));
        }
        self.visiting.insert(key.clone());
        for dependency in array(effective.get("depends")) {
            self.resolve(&py_str(dependency))?;
        }
        self.visiting.remove(&key);
        self.visited.insert(key.clone());
        self.plan.push((key, manifest));
        Ok(())
    }
}

impl Engine {
    /// Plan de instalación (toma el lock de mutación).
    ///
    /// # Errors
    ///
    /// `Componente no encontrado o ambiguo`, `Dependencia circular`, `{key}: {estado}`,
    /// `Pasos no soportados: …`, cambios de persistencia y los de lectura.
    pub fn installation_plan(
        &self,
        requested: &[&str],
        profile: Option<&str>,
        manifests: &BTreeMap<String, Value>,
        pins: &BTreeMap<String, Value>,
        log: Log<'_>,
    ) -> Result<Plan> {
        let _lock = self.mutation()?;
        self.plan_unlocked(requested, profile, manifests, pins, log)
    }

    pub(crate) fn plan_unlocked(
        &self,
        requested: &[&str],
        profile: Option<&str>,
        manifests: &BTreeMap<String, Value>,
        pins: &BTreeMap<String, Value>,
        log: Log<'_>,
    ) -> Result<Plan> {
        self.initialize()?;
        let mut data = profile.map(|name| self.profile(name)).transpose()?;
        let current = self.installed()?;
        let mut planner = Planner {
            engine: self,
            catalog: self.catalog()?,
            current: &current,
            manifests,
            pins,
            log,
            retained: BTreeMap::new(),
            visiting: BTreeSet::new(),
            visited: BTreeSet::new(),
            plan: Vec::new(),
        };
        for request in requested {
            planner.resolve(request)?;
        }
        let Planner {
            catalog,
            retained,
            plan,
            ..
        } = planner;
        let mut records = Records::new();
        for (key, manifest) in &plan {
            let digest = fingerprint(manifest);
            let (bucket, app) = key.split_once('/').unwrap_or((key, ""));
            let relative = format!("core/apps/{}/{}", identifier(bucket)?, identifier(app)?);
            inside(self.root(), &relative)?;
            let record = Record {
                key: key.clone(),
                version: manifest.get("version").cloned().unwrap_or(Value::Null),
                manifest_sha256: digest,
                path: relative,
                manifest: manifest.clone(),
                pinned: retained.get(key).cloned(),
            };
            records.insert(key.clone(), record);
            if let Some(data) = data.as_mut()
                && !data.apps.contains(key)
            {
                data.apps.push(key.clone());
            }
        }
        for (key, manifest) in &plan {
            if let Some(old) = current.get(key) {
                crate::components::validate_update_layout(old, manifest)?;
            }
        }
        let mut unverified = Vec::new();
        let mut resource_sources = BTreeMap::new();
        for (key, manifest) in &plan {
            if current
                .get(key)
                .is_some_and(|c| c.manifest_sha256 == fingerprint(manifest))
            {
                continue;
            }
            let m = effective(manifest)?;
            let sources = catalog.resource_sources(key, &m)?;
            for (name, source) in &sources {
                if source.starts_with("http://") || source.starts_with("https://") {
                    unverified.push(format!("{key} · recursos del catálogo {name}"));
                }
            }
            resource_sources.insert(key.clone(), sources);
            let urls = array(m.get("url"));
            let hashes = mito_transfer::download_hashes(urls.len(), m.get("hash"))?;
            let version = manifest.get("version").map(py_str).unwrap_or_default();
            for (index, expected) in hashes.iter().enumerate() {
                if expected.is_none() {
                    unverified.push(format!("{key} {version} · descarga {}", index + 1));
                }
            }
        }
        let profiles = self
            .profiles()
            .into_iter()
            .map(|name| self.profile(&name).map(|p| (name, p)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Plan {
            records,
            current,
            data,
            unverified,
            resource_sources,
            profiles,
        })
    }

    /// Instala `requested` (y sus dependencias) y devuelve los registros del plan.
    ///
    /// Sin `profile` no se modifica ningún profile. Los componentes nuevos o
    /// reemplazados no se activan salvo en el profile pedido.
    ///
    /// # Errors
    ///
    /// `Importación cancelada`, `Instalación cancelada: descargas sin hash no
    /// confirmadas`, `STATE_CONFLICT: …` si el plan cambió, los del plan y los de cada
    /// descarga, extracción y publicación. Si falla, ningún profile ni componente cambia.
    pub fn install(
        &self,
        requested: &[&str],
        options: &InstallOptions<'_>,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<Records> {
        let prepared = self.installation_plan(
            requested,
            options.profile,
            &options.manifests,
            &options.pins,
            log,
        )?;
        if let Some(confirm) = options.confirm_plan
            && !confirm(&prepared.records)
        {
            return bail(Message::new("Importación cancelada"));
        }
        if !prepared.unverified.is_empty()
            && !options
                .confirm_unverified
                .is_some_and(|confirm| confirm(&prepared.unverified))
        {
            return bail(Message::new(
                "Instalación cancelada: descargas sin hash no confirmadas",
            ));
        }
        let _lock = self.mutation()?;
        let current = self.plan_unlocked(
            requested,
            options.profile,
            &options.manifests,
            &options.pins,
            &|_| {},
        )?;
        if current != prepared {
            return Err(Error::untranslated(
                "STATE_CONFLICT: installation plan changed; retry and review again",
            ));
        }
        self.publish_plan(
            &current.records,
            &current.current,
            options.profile,
            current.data.as_ref(),
            log,
            operation,
        )?;
        match options.profile {
            Some(profile) => log(Message::new("Profile {p1} preparado")
                .arg("p1", profile)
                .into()),
            None => {
                log(Message::new("Componente instalado. Puedes activarlo en Inicio.").into());
            }
        }
        Ok(current.records)
    }

    /// Ruta de `7z.exe`: la inyectada en [`crate::Extensions::sevenzip`] o, si existe,
    /// `core/tools/7zip/7z.exe` (preparado por el bootstrap).
    pub fn sevenzip_path(&self) -> Option<PathBuf> {
        self.extensions().sevenzip.clone().or_else(|| {
            let path = self.layout().sevenzip_exe();
            path.is_file().then_some(path)
        })
    }

    /// Ruta de `innounp.exe` si existe `core/tools/innounp/innounp.exe` (preparado por el
    /// bootstrap). El extractor de Inno Setup lo conecta F7 con [`crate::Extensions::inno`].
    pub fn innounp_path(&self) -> Option<PathBuf> {
        let path = self.layout().innounp_exe();
        path.is_file().then_some(path)
    }

    /// Prepara un componente en `destination` (una carpeta nueva dentro de
    /// `core/staging`): descarga, extrae, valida los `bin` y escribe `.mito.json`.
    ///
    /// Todo ocurre en un directorio temporal de `core/staging` que se borra siempre;
    /// el resultado se mueve a `destination` con un único `rename`.
    pub(crate) fn install_payload(
        &self,
        record: &Record,
        destination: &Path,
        log: Log<'_>,
        operation: &Operation,
    ) -> Result<()> {
        let key = record.key.as_str();
        let version = record.version_text();
        let m = record.effective()?;
        let layout = self.layout();
        create_dir_all(&layout.staging())?;
        let tmp = tempfile::tempdir_in(layout.staging()).map_err(io_error(&layout.staging()))?;
        let stage = tmp.path();
        let payload = stage.join("payload");
        create_dir_all(&payload)?;
        let urls: Vec<String> = array(m.get("url")).into_iter().map(py_str).collect();
        let hashes = mito_transfer::download_hashes(urls.len(), m.get("hash"))?;
        let total_urls = urls.len();
        for (index, (url, expected)) in urls.iter().zip(&hashes).enumerate() {
            let number = index + 1;
            if expected.is_none() {
                log(
                    Message::new("{p0}: descarga sin hash declarado ({p2}/{p4})")
                        .arg("p0", key)
                        .arg("p2", number)
                        .arg("p4", total_urls)
                        .into(),
                );
            }
            let text = Message::new("Descargando {p1} {p3} ({p5}/{p7})")
                .arg("p1", key)
                .arg("p3", &version)
                .arg("p5", number)
                .arg("p7", total_urls);
            log(text.clone().into());
            operation.phase("DESCARGANDO", text, true);
            let archive = stage.join(format!("download-{index}"));
            step(layout, "install", "download-and-hash", Some(key), || {
                self.extensions()
                    .downloader
                    .download(url, &archive, expected.as_deref(), operation)
                    .map_err(Error::from)
            })?;
            operation.check()?;
            let expanded = stage.join(format!("expanded-{index}"));
            let filename = mito_transfer::download_filename(url)?;
            let positional = |field: &str| -> Result<String> {
                let value = m.get(field);
                let chosen = match value {
                    Some(Value::Array(items)) => {
                        if items.len() != urls.len() {
                            return bail(
                                Message::new("{p0}: cantidad distinta de URL").arg("p0", field),
                            );
                        }
                        items.get(index).filter(|v| py_truthy(v)).map(py_str)
                    }
                    Some(other) if py_truthy(other) => Some(py_str(other)),
                    _ => None,
                };
                Ok(chosen.unwrap_or_else(|| ".".to_owned()))
            };
            let extract_dir = positional("extract_dir")?;
            operation.phase(
                "PREPARANDO ARCHIVOS",
                Message::new("Extrayendo {p1}…").arg("p1", key).to_string(),
                true,
            );
            let sevenzip = self.sevenzip_path();
            let extractor = ExtractOptions {
                sevenzip: sevenzip.as_deref(),
                innosetup: m.get("innosetup").is_some_and(py_truthy),
                inno: self
                    .extensions()
                    .inno
                    .as_deref()
                    .map(|f| f as &dyn Fn(&Path, &Path) -> mito_transfer::Result<()>),
                msi: self
                    .extensions()
                    .msi
                    .as_deref()
                    .map(|f| f as &dyn Fn(&Path, &Path) -> mito_transfer::Result<()>),
                log: Some(&|message| log(message.into())),
                limits: None,
            };
            let extracted = step(layout, "install", "extract", Some(key), || {
                Ok(mito_transfer::archives::extract(
                    &archive, &expanded, &filename, &extractor,
                )?)
            })?;
            operation.check()?;
            let source = inside(
                &expanded,
                if extracted == Extraction::Extracted {
                    &extract_dir
                } else {
                    "."
                },
            )?;
            let target = inside(&payload, &positional("extract_to")?)?;
            if !source.is_dir() {
                return bail(Message::new("extract_dir no encontrado: {p1}").arg(
                    "p1",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                ));
            }
            log(Message::new("Preparando {p1}").arg("p1", key).into());
            let files = list_files(&source)?;
            for relative in &files {
                if target.join(relative).exists() {
                    return bail(Message::new(
                        "Dos artefactos intentan escribir el mismo archivo",
                    ));
                }
            }
            let total = files.len();
            let moving = Message::new("Moviendo {p1} archivos de {p2} a su carpeta. Con muchos archivos este paso puede tardar varios minutos; MITO sigue trabajando.")
                .arg("p1", total)
                .arg("p2", key);
            let mut copied = 0usize;
            step(layout, "install", "copy-payload", Some(key), || {
                operation.phase("PREPARANDO ARCHIVOS", moving.clone(), true);
                log(LogLine::long_step(moving, true));
                // Esperar al antivirus compensa mientras cuesta menos que el
                // respaldo archivo a archivo (~0,2 s por archivo), con tope de 30 s.
                #[expect(clippy::cast_precision_loss, reason = "solo estima una espera")]
                let wait = Duration::from_secs_f64((0.15 * total as f64).min(30.0));
                let outcome = move_whole(&source, &target, wait);
                diagnostics::write(
                    layout,
                    LogLevel::Info,
                    "install.move-whole",
                    if outcome.moved { "moved" } else { "per-file" },
                    &[
                        ("component", json!(key)),
                        ("files", json!(total)),
                        ("attempts", json!(outcome.attempts)),
                        ("reason", json!(outcome.reason)),
                    ],
                );
                let result = if outcome.moved {
                    copied = total;
                    Ok(())
                } else {
                    copy_tree_by_file(
                        &source,
                        &files,
                        &target,
                        key,
                        total,
                        &mut copied,
                        log,
                        operation,
                    )
                };
                log(LogLine::long_step(
                    Message::new("Archivos {p1}: {p2}/{p3}")
                        .arg("p1", key)
                        .arg("p2", copied)
                        .arg("p3", total),
                    false,
                ));
                result
            })?;
            operation.check()?;
        }
        operation.phase(
            "PREPARANDO ARCHIVOS",
            Message::new("Preparando {p1}").arg("p1", key),
            true,
        );
        operation.check()?;
        step(layout, "install", "pre-install", Some(key), || {
            self.extensions().hooks.pre_install(key, &m, &payload)
        })?;
        operation.check()?;
        step(layout, "install", "installer", Some(key), || {
            self.extensions().hooks.installer(key, &m, &payload)
        })?;
        operation.check()?;
        log(Message::new("Validando {p1}").arg("p1", key).into());
        step(layout, "install", "validate", Some(key), || {
            for bin in bins(&m)? {
                if !inside(&payload, &bin.executable)?.is_file() {
                    return bail(Message::new("Comando ausente: {p1}").arg("p1", &bin.executable));
                }
            }
            Ok(())
        })?;
        write_json(&payload.join(crate::record::MARKER), &record.marker())?;
        if let Some(parent) = destination.parent() {
            create_dir_all(parent)?;
        }
        if destination.exists() {
            return bail(
                Message::new("Instalación incompleta existente: {p1}")
                    .arg("p1", destination.display()),
            );
        }
        log(Message::new(
            "Publicando {p1}; el antivirus puede retrasar este paso con muchos archivos",
        )
        .arg("p1", key)
        .into());
        step(layout, "install", "publish", Some(key), || {
            fs::rename(&payload, destination).map_err(io_error(destination))
        })?;
        log(Message::new("Instalado {p1} {p3}")
            .arg("p1", key)
            .arg("p3", &version)
            .into());
        Ok(())
    }
}

/// Rutas relativas de todos los archivos bajo `source`.
fn list_files(source: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let folder = source.join(&relative);
        for entry in fs::read_dir(&folder).map_err(io_error(&folder))? {
            let entry = entry.map_err(io_error(&folder))?;
            let path = relative.join(entry.file_name());
            if entry.path().is_dir() && !is_link(&entry.path()) {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[expect(
    clippy::too_many_arguments,
    reason = "reproduce el cierre del original"
)]
fn copy_tree_by_file(
    source: &Path,
    files: &[PathBuf],
    target: &Path,
    key: &str,
    total: usize,
    copied: &mut usize,
    log: Log<'_>,
    operation: &Operation,
) -> Result<()> {
    create_dir_all(target)?;
    for relative in files {
        let from = source.join(relative);
        let to = target.join(relative);
        if let Some(parent) = to.parent() {
            create_dir_all(parent)?;
        }
        // `expanded-N` es temporal y está en el mismo staging: mover evita reescribir
        // cada archivo y que el antivirus lo vuelva a escanear.
        if fs::rename(&from, &to).is_err() {
            copy_file(&from, &to).map_err(io_error(&to))?;
        }
        *copied += 1;
        if (*copied).is_multiple_of(1000) && *copied < total {
            log(Message::new("Archivos {p1}: {p2}/{p3}")
                .arg("p1", key)
                .arg("p2", *copied)
                .arg("p3", total)
                .into());
        }
        if (*copied).is_multiple_of(50) || *copied == total {
            operation.transfer(*copied as u64, Some(total as u64), "files");
            operation.check()?;
        }
    }
    Ok(())
}

/// Resultado de [`move_whole`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    /// `true` si el árbol se movió con un solo `rename`.
    pub moved: bool,
    /// Intentos de `rename`.
    pub attempts: u32,
    /// Motivo si no se movió (`target-not-empty`, `winerror-5`…).
    pub reason: String,
}

/// Mueve un árbol recién extraído con un solo `rename` si no hay nada que fusionar.
///
/// Renombrar archivo a archivo espera al antivirus uno a uno; un `rename` de directorio
/// lo evita. Justo tras extraer, el antivirus puede denegarlo: se reintenta hasta `wait`
/// con una espera que se duplica hasta 2 s. Si el destino tiene contenido o el `rename`
/// no llega a funcionar, devuelve `moved == false` y deja el destino creado para fusionar.
pub fn move_whole(source: &Path, target: &Path, wait: Duration) -> MoveOutcome {
    move_whole_with(
        source,
        target,
        wait,
        &mut |a, b| fs::rename(a, b),
        &mut std::thread::sleep,
    )
}

/// [`move_whole`] con el `rename` y la espera inyectados (el tiempo transcurrido es la
/// suma de las esperas pedidas).
pub fn move_whole_with(
    source: &Path,
    target: &Path,
    wait: Duration,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    sleep: &mut dyn FnMut(Duration),
) -> MoveOutcome {
    let mut outcome = MoveOutcome {
        moved: false,
        attempts: 0,
        reason: String::new(),
    };
    let mut attempt = |outcome: &mut MoveOutcome| -> io::Result<bool> {
        if target.exists() {
            let empty =
                target.is_dir() && !is_link(target) && fs::read_dir(target)?.next().is_none();
            if !empty {
                "target-not-empty".clone_into(&mut outcome.reason);
                return Ok(false);
            }
            fs::remove_dir(target)?;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut elapsed = Duration::ZERO;
        let mut delay = Duration::from_millis(250);
        loop {
            outcome.attempts += 1;
            match rename(source, target) {
                Ok(()) => return Ok(true),
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    outcome.reason = format!("winerror-{}", error.raw_os_error().unwrap_or(5));
                    if elapsed + delay > wait {
                        return Err(error);
                    }
                    sleep(delay);
                    elapsed += delay;
                    delay = (delay * 2).min(Duration::from_secs(2));
                }
                Err(error) => return Err(error),
            }
        }
    };
    match attempt(&mut outcome) {
        Ok(true) => outcome.moved = true,
        Ok(false) => {}
        Err(error) => {
            if outcome.reason.is_empty() {
                outcome.reason = format!("{:?}", error.kind());
            }
            // El destino queda creado para fusionar archivo a archivo.
            let _ = fs::create_dir_all(target);
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_whole_reintenta_el_antivirus_y_acaba_cediendo() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target) = (dir.path().join("s"), dir.path().join("t"));
        fs::create_dir(&source).unwrap();
        let mut sleeps = Vec::new();
        let mut denied = 0;
        let outcome = move_whole_with(
            &source,
            &target,
            Duration::from_secs(5),
            &mut |a, b| {
                if denied < 2 {
                    denied += 1;
                    Err(io::Error::from(io::ErrorKind::PermissionDenied))
                } else {
                    fs::rename(a, b)
                }
            },
            &mut |d| sleeps.push(d),
        );
        assert!(outcome.moved);
        assert_eq!(outcome.attempts, 3);
        assert_eq!(
            sleeps,
            [Duration::from_millis(250), Duration::from_millis(500)]
        );
        // Sin tiempo de espera cede en el primer intento y deja el destino creado.
        let (source, target) = (dir.path().join("s2"), dir.path().join("t2"));
        fs::create_dir(&source).unwrap();
        let outcome = move_whole_with(
            &source,
            &target,
            Duration::ZERO,
            &mut |_, _| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            &mut |_| {},
        );
        assert!(!outcome.moved);
        assert_eq!(outcome.attempts, 1);
        assert!(target.is_dir());
    }

    #[test]
    fn move_whole_no_fusiona_con_un_destino_con_contenido() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target) = (dir.path().join("s"), dir.path().join("t"));
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("x"), "x").unwrap();
        let outcome = move_whole(&source, &target, Duration::ZERO);
        assert!(!outcome.moved);
        assert_eq!(outcome.reason, "target-not-empty");
        assert!(source.is_dir());
        // Un destino vacío se sustituye.
        fs::remove_file(target.join("x")).unwrap();
        assert!(move_whole(&source, &target, Duration::ZERO).moved);
    }
}
