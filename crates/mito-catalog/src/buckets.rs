//! Snapshots de buckets: importación, refresco, consulta y filtros (`Catalog` del original).
//!
//! Contrato: `docs/spec/modules/buckets.md` y `catalog.md`. Un snapshot es
//! `core/buckets/<bucket>.json` con `{"schema": 1, "source": …, "apps": {…}}` y los
//! manifiestos **originales**, escrito de forma atómica.
//!
//! El catálogo no lee `config.properties` ni hace red (ADR 0008, punto 10): recibe los
//! orígenes ya resueltos ([`BucketSources`]) y descarga mediante un [`Fetcher`] que la
//! capa superior conecta con `mito_transfer`.
//!
//! Bloqueo: [`Catalog::import_bucket`] y [`Catalog::refresh_sources`] no toman el lock de
//! mutación (en el original lo toma el comando que las llama); [`Catalog::refresh_keys`]
//! toma el lock sin espera solo para fusionar cada manifiesto.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use mito_core::storage::{identifier, read_json, write_json};
use mito_core::{Layout, Message, MutationLock, Operation};
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::archive::archive_manifests;
use crate::compat::{Compatibility, compatibility};
use crate::error::{Error, FetchError, Result, bail};
use crate::manifest::Manifest;
use crate::pyjson::py_str;
use crate::pypath;
use crate::resources::required_buckets;
use crate::search::Entry;

/// Descarga de archivos para el catálogo. La implementa la capa que conoce la red
/// (F4 la conecta con `mito_transfer::download`); los tests usan un servidor falso.
pub trait Fetcher {
    /// Descarga `url` en `destination`, verificando `expected` si se indica.
    ///
    /// # Errors
    ///
    /// [`FetchError::HttpStatus`] si el servidor responde con un error HTTP (el refresco
    /// por clave distingue el 404) y [`FetchError::Failed`] en cualquier otro fallo.
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        operation: &Operation,
    ) -> std::result::Result<(), FetchError>;
}

/// Expresión literal validada por los tests: compilarla no puede fallar.
#[expect(clippy::expect_used, reason = "expresión regular literal")]
static GITHUB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://github.com/([\w-]+)/([\w.-]+)$").expect("expresión literal válida")
});

/// Valor de `schema` que escribe el original.
const SCHEMA: u64 = 1;

/// Propietario y repositorio de un origen GitHub (`https?://github.com/<owner>/<repo>`).
fn github_repository(source: &str) -> Option<(String, String)> {
    let captures = GITHUB.captures(source.trim_end_matches('/'))?;
    Some((captures[1].to_owned(), captures[2].to_owned()))
}

fn is_remote(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Orígenes `bucket.<nombre>=URL` ya resueltos, en el orden de la configuración.
///
/// Un nombre repetido conserva su posición y toma el último valor, como un `dict`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BucketSources {
    entries: Vec<(String, String)>,
}

impl BucketSources {
    /// Sin orígenes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade u actualiza un origen. El nombre se valida y normaliza con `identifier`.
    ///
    /// # Errors
    ///
    /// Los de `identifier` para `name`.
    pub fn insert(&mut self, name: &str, source: &str) -> Result<()> {
        let name = identifier(name)?;
        let source = source.to_owned();
        match self.entries.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = source,
            None => self.entries.push((name, source)),
        }
        Ok(())
    }

    /// Origen del bucket `name`, tal como está registrado.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, source)| source.as_str())
    }

    /// Pares `(nombre, origen)` en orden.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, s)| (n.as_str(), s.as_str()))
    }

    /// Nombres de bucket en orden.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }

    /// ¿Hay algún origen?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Extrae los orígenes del texto de `config.properties` (`Catalog.sources`).
    ///
    /// Cuenta toda línea que **empieza** por `bucket.` y contiene `=`: el nombre (sin
    /// espacios de los lados) pasa por `identifier` y el valor se recorta. Es una función
    /// pura: leer el archivo corresponde a la capa de configuración.
    ///
    /// # Errors
    ///
    /// Los de `identifier` si un nombre de bucket no es válido.
    pub fn from_properties(text: &str) -> Result<Self> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut sources = Self::new();
        for line in py_splitlines(text) {
            if line.starts_with("bucket.")
                && let Some((key, value)) = line.split_once('=')
            {
                sources.insert(key["bucket.".len()..].trim(), value.trim())?;
            }
        }
        Ok(sources)
    }
}

/// `str.splitlines()` de Python: además de `\n` y `\r\n`, separa por `\r`, `\v`, `\f`,
/// `\x1c`-`\x1e`, `\x85`, ` ` y ` `.
fn py_splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(&text[start..index]);
            start = index + c.len_utf8();
            if c == '\r'
                && let Some(&(next, '\n')) = chars.peek()
            {
                chars.next();
                start = next + 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Lee un JSON de manifiesto o snapshot, con o sin BOM.
///
/// # Errors
///
/// E/S del núcleo si no se puede leer y un error de dominio `JSON no válido en …` si el
/// contenido no es JSON (el original deja escapar el `JSONDecodeError`).
pub fn read_json_value(path: &Path) -> Result<Value> {
    match read_json::<Value>(path) {
        Ok(value) => Ok(value),
        Err(mito_core::Error::Json { path, source }) => bail(
            Message::new("JSON no válido en {p1}: {p2}")
                .arg("p1", path.display())
                .arg("p2", source),
        ),
        Err(error) => Err(error.into()),
    }
}

fn invalid_snapshot(path: &Path) -> Error {
    Error::domain(Message::new("Snapshot de catálogo no válido: {p1}").arg("p1", path.display()))
}

/// Archivos `*.json` de una carpeta, ordenados como `sorted(Path.glob('*.json'))` en
/// Windows (sin distinguir mayúsculas). Una carpeta inexistente no tiene ninguno.
fn json_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(mito_core::Error::io(folder, error).into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| mito_core::Error::io(folder, e))?.path();
        let is_json = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_lowercase().ends_with(".json"));
        if is_json {
            files.push(path);
        }
    }
    files.sort_by_cached_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    Ok(files)
}

/// Nombre sin extensión (`Path.stem`) de un archivo.
fn file_stem(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    pypath::stem(name.as_deref().unwrap_or_default()).to_owned()
}

/// Catálogo de una instancia: snapshots en `core/buckets/` y orígenes configurados.
#[derive(Debug, Clone)]
pub struct Catalog {
    layout: Layout,
    sources: BucketSources,
}

impl Catalog {
    /// Catálogo de la instancia `layout` con los orígenes ya resueltos.
    pub fn new(layout: Layout, sources: BucketSources) -> Self {
        Self { layout, sources }
    }

    /// Instancia a la que pertenece.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Orígenes configurados (`bucket.<nombre>`).
    pub fn sources(&self) -> &BucketSources {
        &self.sources
    }

    /// Carpeta de los snapshots (`core/buckets`).
    pub fn folder(&self) -> PathBuf {
        self.layout.buckets()
    }

    /// Archivo del snapshot del bucket `bucket`.
    pub fn snapshot_path(&self, bucket: &str) -> PathBuf {
        self.folder().join(format!("{bucket}.json"))
    }

    /// Todas las apps como `("bucket/app", manifiesto)`: snapshots por nombre de archivo
    /// y apps en el orden del archivo.
    ///
    /// # Errors
    ///
    /// Un snapshot ilegible, con JSON inválido o sin objeto `apps` falla (el original
    /// propaga `JSONDecodeError` o `KeyError`).
    pub fn all(&self) -> Result<Vec<(String, Manifest)>> {
        let mut rows = Vec::new();
        for path in json_files(&self.folder())? {
            let stem = file_stem(&path);
            let index = read_json_value(&path)?;
            let Some(apps) = index.get("apps").and_then(Value::as_object) else {
                return Err(invalid_snapshot(&path));
            };
            for (name, manifest) in apps {
                rows.push((format!("{stem}/{name}"), Manifest::new(manifest.clone())));
            }
        }
        Ok(rows)
    }

    /// Filas del catálogo con su compatibilidad, listas para [`crate::rank_entries`].
    ///
    /// # Errors
    ///
    /// Los de [`Self::all`].
    pub fn search_entries(&self) -> Result<Vec<Entry<Compatibility>>> {
        Ok(self
            .all()?
            .into_iter()
            .map(|(key, manifest)| {
                let status = compatibility(&key, &manifest);
                Entry::new(key, manifest, status)
            })
            .collect())
    }

    /// Nombres de bucket conocidos: orígenes configurados y snapshots existentes.
    ///
    /// # Errors
    ///
    /// Error de E/S al listar `core/buckets`.
    pub fn bucket_names(&self) -> Result<BTreeSet<String>> {
        let mut names: BTreeSet<String> = self.sources.names().map(str::to_owned).collect();
        names.extend(json_files(&self.folder())?.iter().map(|p| file_stem(p)));
        Ok(names)
    }

    /// Resuelve `bucket/app` o `app` a su clave y manifiesto (`resolve`).
    ///
    /// La comparación es literal: `Main/Tool` no coincide con `main/tool`.
    ///
    /// # Errors
    ///
    /// `Componente no encontrado o ambiguo: …` si no hay exactamente una coincidencia.
    pub fn resolve(&self, name: &str) -> Result<(String, Manifest)> {
        let mut matches: Vec<(String, Manifest)> = self
            .all()?
            .into_iter()
            .filter(|(key, _)| {
                key == name || (!name.contains('/') && key.split('/').nth(1) == Some(name))
            })
            .collect();
        if matches.len() != 1 {
            return bail(
                Message::new("Componente no encontrado o ambiguo: {p1}. Usa bucket/app.")
                    .arg("p1", name),
            );
        }
        Ok(matches.remove(0))
    }

    /// Importa un bucket desde un repositorio GitHub o una carpeta local y reemplaza su
    /// snapshot de forma atómica. Devuelve el número de manifiestos.
    ///
    /// No añade el origen a `config.properties` y no toma el lock de mutación: lo hace
    /// quien llama. Si algo falla, el snapshot anterior no cambia.
    ///
    /// # Errors
    ///
    /// `La fuente debe ser un repositorio GitHub HTTPS o una carpeta local`,
    /// `No hay manifiestos en …`, los de [`archive_manifests`], fallos de la descarga y
    /// de lectura de manifiestos locales.
    pub fn import_bucket(
        &self,
        name: &str,
        source: &str,
        fetcher: &dyn Fetcher,
        operation: &Operation,
    ) -> Result<usize> {
        let name = identifier(name)?;
        let folder = self.folder();
        fs::create_dir_all(&folder).map_err(|e| mito_core::Error::io(&folder, e))?;
        let stage = tempfile::Builder::new()
            .tempdir_in(&folder)
            .map_err(|e| mito_core::Error::io(&folder, e))?;
        let apps = if is_remote(source) {
            let Some((owner, repository)) = github_repository(source) else {
                return bail(Message::new(
                    "La fuente debe ser un repositorio GitHub HTTPS o una carpeta local",
                ));
            };
            let archive = stage.path().join("bucket.zip");
            let url = format!("https://api.github.com/repos/{owner}/{repository}/zipball");
            fetcher.download(&url, &archive, None, operation)?;
            archive_manifests(&archive)?
        } else {
            local_manifests(Path::new(source))?
        };
        if apps.is_empty() {
            return bail(Message::new("No hay manifiestos en {p1}").arg("p1", source));
        }
        let count = apps.len();
        let snapshot = json!({"schema": SCHEMA, "source": source, "apps": apps});
        write_json(&self.snapshot_path(&name), &snapshot)?;
        Ok(count)
    }

    /// Reimporta los orígenes configurados (`bucket refresh [nombre]`): todos, o solo
    /// `only`. Devuelve `(bucket, manifiestos)` por origen, en orden. Un bucket que está
    /// en un snapshot pero no en la configuración no se refresca.
    ///
    /// # Errors
    ///
    /// `Bucket no configurado` si `only` no está en los orígenes, la cancelación entre
    /// orígenes y los de [`Self::import_bucket`].
    pub fn refresh_sources(
        &self,
        only: Option<&str>,
        fetcher: &dyn Fetcher,
        operation: &Operation,
    ) -> Result<Vec<(String, usize)>> {
        let selected: Vec<(String, String)> = match only {
            Some(name) => {
                let Some(source) = self.sources.get(name) else {
                    return bail(Message::new("Bucket no configurado"));
                };
                vec![(name.to_owned(), source.to_owned())]
            }
            None => self
                .sources
                .iter()
                .map(|(n, s)| (n.to_owned(), s.to_owned()))
                .collect(),
        };
        let mut results = Vec::with_capacity(selected.len());
        for (name, source) in selected {
            operation.check()?;
            let count = self.import_bucket(&name, &source, fetcher, operation)?;
            results.push((name, count));
        }
        Ok(results)
    }

    /// Refresca las claves instaladas (`refresh_installed`). Ver [`Self::refresh_keys`].
    pub fn refresh_installed<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
        fetcher: &dyn Fetcher,
        operation: &Operation,
        log: &mut dyn FnMut(Message),
    ) -> usize {
        self.refresh_keys(keys, fetcher, operation, log)
    }

    /// Refresca solo `apps[<app>]` de cada clave `bucket/app` (sin sincronizar buckets
    /// completos) y devuelve cuántas se refrescaron.
    ///
    /// Cada clave es independiente: un fallo (red, JSON, versión no válida, lock ocupado)
    /// se comunica a `log` con `No se pudo comprobar …` y se conserva el catálogo
    /// anterior. La fusión se hace bajo el lock de mutación sin espera, nunca durante la
    /// descarga, y no toca otras entradas del snapshot.
    pub fn refresh_keys<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
        fetcher: &dyn Fetcher,
        operation: &Operation,
        log: &mut dyn FnMut(Message),
    ) -> usize {
        let keys: BTreeSet<&str> = keys.into_iter().collect();
        let mut refreshed = 0;
        for key in keys {
            match self.refresh_key(key, fetcher, operation) {
                Ok(()) => refreshed += 1,
                Err(error) => {
                    let text = error.to_string();
                    tracing::warn!(
                        operation = "catalog.refresh-installed",
                        component = key,
                        error = %text,
                        "no se pudo refrescar el manifiesto"
                    );
                    log(Message::new(
                        "No se pudo comprobar {p1}: {p3}. Se conserva el catálogo anterior.",
                    )
                    .arg("p1", key)
                    .arg("p3", text));
                }
            }
        }
        refreshed
    }

    fn refresh_key(&self, key: &str, fetcher: &dyn Fetcher, operation: &Operation) -> Result<()> {
        let parts: Vec<&str> = key.split('/').collect();
        let [bucket, app] = parts.as_slice() else {
            let text = if parts.len() < 2 {
                format!(
                    "not enough values to unpack (expected 2, got {})",
                    parts.len()
                )
            } else {
                "too many values to unpack (expected 2)".to_owned()
            };
            return bail(Message::new(text));
        };
        let (bucket, app) = (identifier(bucket)?, identifier(app)?);
        let index_path = self.snapshot_path(&bucket);
        let index = if index_path.exists() {
            Some(read_json_value(&index_path)?)
        } else {
            None
        };
        let recorded = match &index {
            Some(Value::Object(map)) => map.get("source").and_then(Value::as_str),
            Some(_) => return Err(invalid_snapshot(&index_path)),
            None => None,
        };
        let source = self
            .sources
            .get(&bucket)
            .filter(|s| !s.is_empty())
            .or(recorded.filter(|s| !s.is_empty()));
        let Some(source) = source else {
            return bail(Message::new("No hay origen registrado para el catálogo"));
        };
        let manifest = if is_remote(source) {
            let Some((owner, repository)) = github_repository(source) else {
                return bail(Message::new(
                    "La fuente debe ser un repositorio GitHub o una carpeta local",
                ));
            };
            let scratch = tempfile::tempdir().map_err(|e| mito_core::Error::io("<temp>", e))?;
            let target = scratch.path().join("manifest.json");
            let base = format!("https://raw.githubusercontent.com/{owner}/{repository}/HEAD");
            match fetcher.download(
                &format!("{base}/bucket/{app}.json"),
                &target,
                None,
                operation,
            ) {
                Err(FetchError::HttpStatus(404)) => {
                    fetcher.download(&format!("{base}/{app}.json"), &target, None, operation)?;
                }
                other => other?,
            }
            read_json_value(&target)?
        } else {
            let folder = Path::new(source);
            let nested = folder.join("bucket");
            let folder = if nested.is_dir() {
                nested
            } else {
                folder.to_path_buf()
            };
            read_json_value(&folder.join(format!("{app}.json")))?
        };
        let valid_version = manifest.as_object().is_some_and(|map| {
            matches!(
                map.get("version"),
                Some(Value::String(_) | Value::Number(_) | Value::Bool(_))
            )
        });
        if !valid_version {
            return bail(Message::new("Manifiesto sin versión válida"));
        }
        if let Some(version) = manifest.get("version") {
            identifier(&py_str(version))?;
        }
        self.store_manifest(&bucket, &app, source, manifest)
    }

    /// Fusiona un manifiesto en el snapshot bajo el lock de mutación (sin espera).
    fn store_manifest(&self, bucket: &str, app: &str, source: &str, manifest: Value) -> Result<()> {
        let _lock = MutationLock::mutation(&self.layout, Duration::ZERO)?;
        let path = self.snapshot_path(bucket);
        let mut index = if path.exists() {
            read_json_value(&path)?
        } else {
            json!({"schema": SCHEMA, "source": source, "apps": {}})
        };
        let Some(apps) = index.get_mut("apps").and_then(Value::as_object_mut) else {
            return Err(invalid_snapshot(&path));
        };
        apps.insert(app.to_owned(), manifest);
        write_json(&path, &index)?;
        Ok(())
    }

    /// Origen de cada bucket que referencian los hooks con `$bucketsdir`
    /// (`bucket_resources.sources`), en orden de nombre.
    ///
    /// # Errors
    ///
    /// `No hay origen registrado para los recursos del catálogo: …`, `La fuente debe ser
    /// un repositorio GitHub o una carpeta local` y los de [`required_buckets`].
    pub fn resource_sources(
        &self,
        key: &str,
        manifest: &Map<String, Value>,
    ) -> Result<Vec<(String, String)>> {
        let mut result = Vec::new();
        for name in required_buckets(key, manifest)? {
            let index = self.snapshot_path(&name);
            let recorded = if index.exists() {
                read_json_value(&index)?
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            };
            let source = self
                .sources
                .get(&name)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
                .or(recorded.filter(|s| !s.is_empty()));
            let Some(source) = source else {
                return bail(
                    Message::new("No hay origen registrado para los recursos del catálogo: {p1}")
                        .arg("p1", &name),
                );
            };
            if is_remote(&source) && github_repository(&source).is_none() {
                return bail(Message::new(
                    "La fuente debe ser un repositorio GitHub o una carpeta local",
                ));
            }
            result.push((name, source));
        }
        Ok(result)
    }
}

/// Manifiestos de una carpeta local: `<origen>/bucket/*.json` si existe, o `<origen>/*.json`.
/// No es recursivo.
fn local_manifests(source: &Path) -> Result<Map<String, Value>> {
    let nested = source.join("bucket");
    let folder = if nested.is_dir() {
        nested
    } else {
        source.to_path_buf()
    };
    let mut manifests = Map::new();
    for path in json_files(&folder)? {
        let name = identifier(&file_stem(&path))?;
        manifests.insert(name, read_json_value(&path)?);
    }
    Ok(manifests)
}

#[cfg(test)]
mod tests;
