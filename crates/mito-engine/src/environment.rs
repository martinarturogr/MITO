//! Construcción del entorno de ejecución de un profile (`engine.py::_environment`).
//!
//! Orden real de `PATH` (el código del original manda sobre su documentación): shims
//! del profile, `home/custom-commands`, `core/commands`, Pocketools, y por cada
//! componente en el orden de `apps` su `env_add_path` y el directorio de cada `bin`;
//! al final el `PATH` heredado (o `MITO_HOST_PATH`). Se quitan duplicados conservando el
//! primero. `HOME`, `USERPROFILE`, `APPDATA`… salen siempre de `home/`, único para toda
//! la instancia.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use mito_catalog::fingerprint;
use mito_catalog::pyjson::py_str;
use mito_core::Message;
use mito_core::storage::{command_name, identifier, inside, portable_path, read_json, write_json};
use serde_json::{Map, Value};

use crate::arguments::split;
use crate::engine::Engine;
use crate::error::{Error, Result, bail};
use crate::fsutil::relative_posix;
use crate::isolation::{payload_path, persist_root};
use crate::profiles::Profile;
use crate::record::{MARKER, Record, effective};

/// Variables de entorno de un proceso, con los nombres en mayúsculas como `os.environ`
/// de Python en Windows.
pub type Env = BTreeMap<String, String>;

/// Un comando expuesto por el manifiesto (`bin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    /// Ruta del ejecutable relativa al payload.
    pub executable: String,
    /// Nombre del comando (tal cual, sin normalizar).
    pub alias: String,
    /// Argumentos fijos.
    pub args: Vec<String>,
}

/// Entrada de lanzamiento declarada (`shortcuts` o `bin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Etiqueta (del shortcut o alias del `bin`).
    pub label: String,
    /// Ejecutable relativo al payload.
    pub executable: String,
    /// Argumentos fijos.
    pub args: Vec<String>,
}

/// Variables y carpetas de `PATH` que aporta un componente.
pub type Contribution = (Vec<(String, String)>, Vec<String>);

/// Entorno del proceso actual con los nombres en mayúsculas.
pub fn process_env() -> Env {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.to_str()?.to_uppercase(), v.to_str()?.to_owned())))
        .collect()
}

/// `Path(item.replace('\\','/')).stem`.
fn stem(path: &str) -> String {
    mito_catalog::pypath::stem(path).to_owned()
}

fn args_of(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => split(text),
        Value::Array(items) => items.iter().map(py_str).collect(),
        Value::Null => Vec::new(),
        other => split(&py_str(other)),
    }
}

/// Comandos de un manifiesto efectivo (`Engine.bins`).
///
/// # Errors
///
/// `Nombre de comando no válido: …` si un alias no es válido o la entrada está mal
/// formada.
pub fn bins(manifest: &Map<String, Value>) -> Result<Vec<Bin>> {
    let items = mito_catalog::array(manifest.get("bin"));
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let bin = match item {
            Value::Array(parts) => {
                let (Some(executable), Some(alias)) = (parts.first(), parts.get(1)) else {
                    return bail(
                        Message::new("Nombre de comando no válido: {p1}")
                            .arg("p1", mito_core::message::py_repr(&py_str(item))),
                    );
                };
                Bin {
                    executable: py_str(executable),
                    alias: py_str(alias),
                    args: parts.get(2).map(args_of).unwrap_or_default(),
                }
            }
            other => {
                let executable = py_str(other);
                Bin {
                    alias: stem(&executable.replace('\\', "/")),
                    executable,
                    args: Vec::new(),
                }
            }
        };
        command_name(&bin.alias)?;
        result.push(bin);
    }
    Ok(result)
}

/// Ruta de `relative` dentro de `payload` (`component_path`).
///
/// # Errors
///
/// Rutas no portables, reservadas o fuera de la raíz.
pub fn component_path(root: &Path, payload: &Path, relative: &str) -> Result<PathBuf> {
    portable_path(relative)?;
    let Some(base) = relative_posix(payload, root) else {
        return bail(Message::new("Ruta fuera de su raíz: {p1}").arg("p1", payload.display()));
    };
    let joined = if base.is_empty() {
        relative.replace('\\', "/")
    } else {
        format!("{base}/{}", relative.replace('\\', "/"))
    };
    Ok(inside(root, &joined)?)
}

fn substitute(value: &str, payload: &str, persist: &str) -> String {
    value
        .replace("${dir}", payload)
        .replace("$dir", payload)
        .replace("${persist_dir}", persist)
        .replace("$persist_dir", persist)
}

/// Aportaciones de un componente: variables (`env_set` sustituido) y carpetas del
/// `PATH`, sin duplicados. No escribe nada en disco.
///
/// # Errors
///
/// Rutas no válidas, alias inválidos y los de aislamiento.
pub fn component_environment(
    root: &Path,
    record: &Record,
    profile: Option<&str>,
) -> Result<Contribution> {
    let layout = mito_core::Layout::new(root);
    let payload = payload_path(&layout, record, profile)?;
    let manifest = record.effective()?;
    let mut paths: Vec<String> = Vec::new();
    for value in mito_catalog::array(manifest.get("env_add_path")) {
        paths.push(
            component_path(root, &payload, &py_str(value))?
                .display()
                .to_string(),
        );
    }
    for bin in bins(&manifest)? {
        let path = component_path(root, &payload, &bin.executable)?;
        paths.push(
            path.parent()
                .map_or_else(|| path.clone(), Path::to_path_buf)
                .display()
                .to_string(),
        );
    }
    let persist = persist_root(&layout, record, profile)?
        .display()
        .to_string();
    let payload_text = payload.display().to_string();
    let variables = manifest
        .get("env_set")
        .and_then(Value::as_object)
        .map(|set| {
            set.iter()
                .map(|(k, v)| (k.clone(), substitute(&py_str(v), &payload_text, &persist)))
                .collect()
        })
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    Ok((variables, paths))
}

/// Variables de contexto de un profile sin efectos (`context_variables`).
///
/// # Errors
///
/// Los de [`Engine::profile`].
fn context(engine: &Engine, profile: &str) -> Result<Env> {
    let profile = identifier(profile)?;
    let data = engine.profile(&profile)?;
    let layout = engine.layout();
    let home = layout.home();
    let drive = match home.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().into_owned(),
        _ => String::new(),
    };
    let home_text = home.display().to_string();
    let temp = home.join("tmp").display().to_string();
    let workspace = inside(&layout.workspaces(), &data.workspace)?;
    let roaming = home.join("AppData").join("Roaming");
    let local = home.join("AppData").join("Local");
    Ok([
        ("MITO_PROFILE", profile),
        ("MITO_ROOT", layout.root().display().to_string()),
        ("MITO_WORKSPACE", workspace.display().to_string()),
        ("HOME", home_text.clone()),
        ("USERPROFILE", home_text.clone()),
        ("APPDATA", roaming.display().to_string()),
        ("LOCALAPPDATA", local.display().to_string()),
        ("TEMP", temp.clone()),
        ("TMP", temp),
        ("HOMEPATH", home_text[drive.len()..].to_owned()),
        ("HOMEDRIVE", drive),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect())
}

/// Contenido de un shim de `bin`. Invoca `core\bin\mito.exe` (ADR 0009): desde
/// `core\state\shims\<profile>\` hay que subir tres niveles hasta `core\`.
fn shim_content(profile: &str, key: &str, alias: &str) -> String {
    let alias = command_name(alias).unwrap_or_default().replace('%', "%%");
    format!(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\n\"%~dp0..\\..\\..\\bin\\mito.exe\" run --profile {profile} --app {key} --bin=\"{alias}\" -- %*\r\n"
    )
}

impl Engine {
    /// Variables de contexto de un profile (`MITO_*`, `HOME`, `APPDATA`…) sin activar
    /// componentes ni crear nada.
    ///
    /// # Errors
    ///
    /// Los de [`Self::profile`].
    pub fn context_variables(&self, profile: &str) -> Result<Env> {
        context(self, profile)
    }

    /// Entorno completo del profile con el del proceso actual como base. Crea las
    /// carpetas, prepara las copias por profile y escribe los shims.
    ///
    /// # Errors
    ///
    /// `Falta el componente …`, `Lock o instalación incoherente: …` y los de validación.
    pub fn environment(&self, profile: &str) -> Result<Env> {
        self.environment_from(profile, process_env())
    }

    /// Como [`Self::environment`] con un entorno base explícito (no toca el del proceso).
    ///
    /// # Errors
    ///
    /// Ver [`Self::environment`].
    pub fn environment_from(&self, profile: &str, base: Env) -> Result<Env> {
        let _lock = self.mutation()?;
        self.build_environment(profile, base)
    }

    /// Entorno y carpeta de trabajo del profile desde una misma lectura. Crea el
    /// workspace.
    ///
    /// # Errors
    ///
    /// Ver [`Self::environment`].
    pub fn execution_context(&self, profile: &str) -> Result<(Env, PathBuf)> {
        let _lock = self.mutation()?;
        let env = self.build_environment(profile, process_env())?;
        let workspace = self.workspace_of(profile)?;
        Ok((env, workspace))
    }

    /// Workspace del profile, creándolo si falta.
    pub(crate) fn workspace_of(&self, profile: &str) -> Result<PathBuf> {
        let data = self.profile(profile)?;
        let workspace = inside(&self.layout().workspaces(), &data.workspace)?;
        fs::create_dir_all(&workspace).map_err(|e| Error::io(&workspace, e))?;
        Ok(workspace)
    }

    /// Aportaciones de un componente (ver [`component_environment`]).
    ///
    /// # Errors
    ///
    /// Ver [`component_environment`].
    pub fn component_environment(
        &self,
        record: &Record,
        profile: Option<&str>,
    ) -> Result<Contribution> {
        component_environment(self.root(), record, profile)
    }

    /// Ruta de `relative` dentro de `payload`.
    ///
    /// # Errors
    ///
    /// Ver [`component_path`].
    pub fn component_path(&self, payload: &Path, relative: &str) -> Result<PathBuf> {
        component_path(self.root(), payload, relative)
    }

    /// Comandos de un manifiesto efectivo.
    ///
    /// # Errors
    ///
    /// Ver [`bins`].
    pub fn bins(manifest: &Map<String, Value>) -> Result<Vec<Bin>> {
        bins(manifest)
    }

    /// Construye el entorno. El llamador tiene el lock de mutación.
    pub(crate) fn build_environment(&self, profile: &str, mut env: Env) -> Result<Env> {
        self.initialize()?;
        let data = self.profile(profile)?;
        let layout = self.layout();
        let inherited = env
            .get("MITO_HOST_PATH")
            .or_else(|| env.get("PATH"))
            .cloned()
            .unwrap_or_default();
        // Un profile nunca se apila sobre la activación de un MITO padre.
        if let Some(active) = env.get("MITO_ACTIVE_VARS").cloned() {
            for key in active.split(';').filter(|k| !k.is_empty()) {
                env.remove(key);
            }
        }
        self.extensions()
            .network
            .proxy_environment(layout, &mut env)?;
        env.extend(self.context_variables(profile)?);
        env.insert("MITO_HOST_PATH".to_owned(), inherited.clone());
        let mut paths = vec![
            layout.home().join("custom-commands").display().to_string(),
            layout.commands().display().to_string(),
        ];
        paths.extend(
            self.extensions()
                .pocketools
                .paths(layout)
                .iter()
                .map(|p| p.display().to_string()),
        );
        let mut variables: Vec<String> = Vec::new();
        for (_, record) in &self.selected(&data)? {
            let payload = inside(layout.root(), &record.path)?;
            let marker_path = payload.join(MARKER);
            if !marker_path.is_file() {
                return bail(
                    Message::new("Falta el componente {p1}; vuelve a instalarlo")
                        .arg("p1", &record.key),
                );
            }
            let marker: Value = read_json(&marker_path)?;
            let coherent = marker.get("key").and_then(Value::as_str) == Some(&record.key)
                && marker.get("manifest_sha256").and_then(Value::as_str)
                    == Some(&record.manifest_sha256)
                && fingerprint(&record.manifest) == record.manifest_sha256;
            if !coherent {
                return bail(
                    Message::new("Lock o instalación incoherente: {p1}").arg("p1", &record.key),
                );
            }
            self.prepare_payload(record, profile)?;
            let (component_vars, component_paths) =
                self.component_environment(record, Some(profile))?;
            paths.extend(component_paths);
            for (key, value) in component_vars {
                env.insert(key.clone(), value);
                variables.push(key);
            }
        }
        let mut overrides: Vec<(String, String)> = Vec::new();
        for (key, value) in self.global_env()?.into_iter().chain(data.env.clone()) {
            let key = key.to_uppercase();
            match overrides.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => overrides.push((key, value)),
            }
        }
        for (key, value) in overrides {
            env.retain(|existing, _| existing.to_uppercase() != key);
            env.insert(key.clone(), value);
            variables.push(key);
        }
        let mut seen = BTreeSet::new();
        variables.retain(|v| seen.insert(v.clone()));
        env.insert("MITO_ACTIVE_VARS".to_owned(), variables.join(";"));
        let shims = self.shims_folder(profile)?;
        self.write_shims(&data, profile, &shims)?;
        let mut seen = BTreeSet::new();
        paths.retain(|p| seen.insert(p.clone()));
        let mut path = vec![shims.display().to_string()];
        path.extend(paths);
        path.push(inherited);
        env.insert("PATH".to_owned(), path.join(";"));
        Ok(env)
    }

    /// `core/state/shims/<profile>`.
    pub(crate) fn shims_folder(&self, profile: &str) -> Result<PathBuf> {
        Ok(self
            .layout()
            .state()
            .join("shims")
            .join(identifier(profile)?))
    }

    /// Escribe los shims de `bin` de los componentes activos y retira los que ya no
    /// corresponden (`_write_shims`).
    ///
    /// # Errors
    ///
    /// Alias inválidos en cualquier componente seleccionado, falta de instalación y E/S.
    pub fn write_shims(&self, data: &Profile, profile: &str, folder: &Path) -> Result<()> {
        fs::create_dir_all(folder).map_err(|e| Error::io(folder, e))?;
        let registry = folder.join("generated.json");
        let previous: Vec<String> = if registry.exists() {
            read_json(&registry)?
        } else {
            Vec::new()
        };
        let selected = self.selected(data)?;
        let mut wanted = BTreeSet::new();
        let mut shims: Vec<(String, String)> = Vec::new();
        for (_, record) in &selected {
            for bin in bins(&record.effective()?)? {
                let name = format!("{}.cmd", command_name(&bin.alias)?);
                shims.push((
                    name.clone(),
                    shim_content(&identifier(profile)?, &record.key, &bin.alias),
                ));
                wanted.insert(name);
            }
        }
        for filename in previous {
            if !wanted.contains(&filename) {
                let path = inside(folder, &filename)?;
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Error::io(&path, e)),
                }
            }
        }
        for (name, content) in shims {
            let path = folder.join(name);
            let unchanged = fs::read_to_string(&path).is_ok_and(|current| {
                current.replace("\r\n", "\n") == content.replace("\r\n", "\n")
            });
            if !unchanged {
                fs::write(&path, content).map_err(|e| Error::io(&path, e))?;
            }
        }
        write_json(
            &registry,
            &Value::Array(wanted.into_iter().map(Value::String).collect()),
        )?;
        Ok(())
    }

    /// Entradas de lanzamiento declaradas (`shortcuts` y `bin`) cuyo ejecutable existe
    /// y tiene una extensión permitida. Nunca se escanean los runtimes.
    ///
    /// # Errors
    ///
    /// Rutas no válidas o manifiesto sin arquitectura x64.
    pub fn application_entries(&self, record: &Record) -> Result<Vec<AppEntry>> {
        let manifest = effective(&record.manifest)?;
        let mut candidates: Vec<AppEntry> = Vec::new();
        if let Some(Value::Array(shortcuts)) = manifest.get("shortcuts") {
            for shortcut in shortcuts {
                let Some(parts) = shortcut.as_array() else {
                    continue;
                };
                let (Some(executable), Some(label)) = (parts.first(), parts.get(1)) else {
                    continue;
                };
                candidates.push(AppEntry {
                    label: py_str(label),
                    executable: py_str(executable),
                    args: parts.get(2).map(args_of).unwrap_or_default(),
                });
            }
        }
        // El original omite un `bin` solo si su ruta coincide con la *etiqueta* de un
        // shortcut y sus argumentos con el texto sin partir de este: nunca ocurre, así
        // que todos los `bin` se añaden.
        for bin in bins(&manifest)? {
            candidates.push(AppEntry {
                label: bin.alias,
                executable: bin.executable,
                args: bin.args,
            });
        }
        let base = inside(self.root(), &record.path)?;
        let mut entries = Vec::new();
        for candidate in candidates {
            let target = component_path(self.root(), &base, &candidate.executable)?;
            let extension = target
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                .unwrap_or_default();
            let allowed = [".exe", ".com", ".cmd", ".bat", ".ps1", ".jar", ".chm"];
            if allowed.contains(&extension.as_str()) && target.is_file() {
                entries.push(candidate);
            }
        }
        Ok(entries)
    }

    /// Ejecutable y argumentos fijos con los que se abre el componente.
    ///
    /// # Errors
    ///
    /// `Shortcut unavailable or ambiguous: …` (sin traducir) y `La app no declara un
    /// ejecutable disponible; úsala desde la terminal`.
    pub fn application_target(
        &self,
        record: &Record,
        shortcut: Option<&str>,
    ) -> Result<(String, Vec<String>)> {
        let mut entries = self.application_entries(record)?;
        if let Some(shortcut) = shortcut {
            entries.retain(|e| e.label == shortcut);
            if entries.len() != 1 {
                return Err(Error::untranslated(format!(
                    "Shortcut unavailable or ambiguous: {shortcut}"
                )));
            }
        } else {
            entries.sort_by_key(|e| e.executable.to_lowercase().ends_with(".chm"));
        }
        match entries.into_iter().next() {
            Some(entry) => Ok((entry.executable, entry.args)),
            None => bail(Message::new(
                "La app no declara un ejecutable disponible; úsala desde la terminal",
            )),
        }
    }

    /// Nombre visible de la aplicación.
    ///
    /// # Errors
    ///
    /// Los de [`Self::application_target`].
    pub fn application_name(&self, record: &Record, shortcut: Option<&str>) -> Result<String> {
        let (executable, _) = self.application_target(record, shortcut)?;
        if let Some(shortcut) = shortcut {
            return Ok(shortcut.to_owned());
        }
        let manifest = effective(&record.manifest)?;
        if let Some(Value::Array(shortcuts)) = manifest.get("shortcuts") {
            for shortcut in shortcuts {
                if let Some(parts) = shortcut.as_array()
                    && parts.first().map(py_str).as_deref() == Some(executable.as_str())
                    && let Some(label) = parts.get(1)
                {
                    let name = py_str(label)
                        .lines()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_owned();
                    if !name.is_empty() {
                        return Ok(name);
                    }
                }
            }
        }
        Ok(record.key.rsplit('/').next().unwrap_or_default().to_owned())
    }

    /// ¿Se puede abrir el componente como aplicación?
    pub fn can_open(&self, record: &Record) -> bool {
        self.application_target(record, None).is_ok()
    }
}
