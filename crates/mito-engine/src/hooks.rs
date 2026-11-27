//! Ejecución de los hooks del catálogo con PowerShell (`hooks.py`, F7).
//!
//! `pre_install`, `installer.script` e `installer.file` se ejecutan como código de
//! confianza, sin sandbox, con un preámbulo ([`SCRIPT`]) que define las funciones de
//! Scoop que usan los catálogos. `post_install` no se ejecuta nunca (el planificador lo
//! registra y lo omite).
//!
//! Los helpers `Expand-*` del preámbulo vuelven a `mito.exe hook-expand` (ADR 0012), que
//! llama a [`hook_expand`]. Contrato: `docs/spec/modules/hooks.md`.

use std::fs;
use std::os::windows::process::CommandExt as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use mito_catalog::array;
use mito_catalog::installer_definition;
use mito_catalog::pyjson::{py_str, py_truthy};
use mito_core::persistence::persist_root;
use mito_core::storage::{inside, resolve_lenient};
use mito_core::{Layout, Message};
use mito_transfer::archives::{extract_7zip, extract_burn, extract_inno, extract_msi, inno_root};
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::arguments::split;
use crate::environment::{Env, process_env};
use crate::error::{Error, Result, bail, io_error};
use crate::extensions::{HookContext, Hooks};
use crate::fsutil::{copy_dir, create_dir_all, is_link};
use crate::isolation::isolated_effective;
use crate::raw_line;

/// Preámbulo PowerShell de cada hook (`SCRIPT` del original). Es parte del contrato con
/// los catálogos: la única diferencia es la llamada de `Invoke-MitoExpand` (ADR 0012).
pub const SCRIPT: &str = include_str!("hooks.ps1");

/// `CREATE_NO_WINDOW` de Win32.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Variables que se sustituyen en `installer.args`; el orden de las alternativas importa.
#[expect(clippy::expect_used, reason = "expresión regular literal")]
static ARGUMENT_VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\$\{(dir|original_dir|persist_dir|version|architecture|global)\}|\$(dir|original_dir|persist_dir|version|architecture|global)\b",
    )
    .expect("expresión literal válida")
});

/// Ejecutor de hooks con `powershell.exe`.
#[derive(Debug, Clone)]
pub struct PowerShellHooks {
    mito: PathBuf,
}

impl PowerShellHooks {
    /// `mito` es la ruta de `core\bin\mito.exe`, que los helpers `Expand-*` invocan
    /// (`$context.mito`).
    pub fn new(mito: impl Into<PathBuf>) -> Self {
        Self { mito: mito.into() }
    }

    /// Ejecuta un paso (`run_script`): escribe el contexto y el runner junto al payload y
    /// lanza PowerShell.
    fn run(&self, context: &HookContext<'_>, hook: &str, step: &Step) -> Result<()> {
        if !step.script.as_ref().is_some_and(py_truthy) && step.file.is_none() {
            return Ok(());
        }
        let key = context.key;
        let payload = context.payload;
        let env = environment(context)?;
        let persist = if isolated_effective(context.manifest) {
            parent_of(payload)?.join("persist")
        } else {
            persist_root(context.layout, key, None)?
        };
        create_dir_all(&persist)?;
        let (bucket, app) = key.split_once('/').unwrap_or((key, ""));
        let version = context
            .manifest
            .get("version")
            .map(py_str)
            .unwrap_or_default();
        let mut names = Vec::new();
        for url in array(context.manifest.get("url")) {
            names.push(mito_transfer::download_filename(&py_str(url))?);
        }
        let fname = if names.len() == 1 {
            json!(names[0])
        } else {
            json!(names)
        };
        let stage = parent_of(payload)?;
        let mut document = Map::new();
        document.insert(
            "root".into(),
            json!(context.layout.root().to_string_lossy()),
        );
        document.insert("mito".into(), json!(self.mito.to_string_lossy()));
        document.insert("dir".into(), json!(payload.to_string_lossy()));
        document.insert("persist_dir".into(), json!(persist.to_string_lossy()));
        document.insert("app".into(), json!(app));
        document.insert("bucket".into(), json!(bucket));
        document.insert(
            "bucketsdir".into(),
            json!(stage.join("hook-buckets").to_string_lossy()),
        );
        document.insert("version".into(), json!(version));
        document.insert("manifest".into(), Value::Object(context.manifest.clone()));
        document.insert("fname".into(), fname);
        document.insert("script".into(), json!(script_text(step.script.as_ref())));
        if let Some(file) = &step.file {
            let values = [
                ("dir", payload.to_string_lossy().into_owned()),
                ("original_dir", payload.to_string_lossy().into_owned()),
                ("persist_dir", persist.to_string_lossy().into_owned()),
                ("version", version.clone()),
                ("architecture", "64bit".to_owned()),
                ("global", "False".to_owned()),
            ];
            let arguments: Vec<String> = argument_list(step.arguments.as_ref())
                .iter()
                .map(|argument| substitute(argument, &values))
                .collect();
            document.insert("file".into(), json!(file.to_string_lossy()));
            document.insert("arguments".into(), json!(arguments));
        }
        let context_path = stage.join("hook-context.json");
        let encoded = serde_json::to_vec(&Value::Object(document))
            .map_err(|e| Error::io(&context_path, std::io::Error::other(e)))?;
        fs::write(&context_path, encoded).map_err(io_error(&context_path))?;
        let runner = stage.join(format!("{hook}.ps1"));
        let mut text = b"\xEF\xBB\xBF".to_vec();
        text.extend_from_slice(
            SCRIPT
                .replace("\r\n", "\n")
                .replace('\n', "\r\n")
                .as_bytes(),
        );
        fs::write(&runner, text).map_err(io_error(&runner))?;

        (context.log)(
            Message::new("{p0}: ejecutando {hook}")
                .arg("p0", key)
                .arg("hook", hook)
                .into(),
        );
        let output = Command::new(powershell())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&runner)
            .arg(&context_path)
            .current_dir(payload)
            .env_clear()
            .envs(&env)
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| {
                Error::domain(
                    Message::new("{p0}: no se pudo ejecutar {hook}: {p2}")
                        .arg("p0", key)
                        .arg("hook", hook)
                        .arg("p2", error),
                )
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stdout.lines() {
            (context.log)(raw_line(format!("{key}: {line}")));
        }
        if !output.status.success() {
            // Python muestra los códigos de Windows sin signo.
            let code = output.status.code().map_or(0, i32::cast_unsigned);
            return bail(
                Message::new("{p0}: {hook} falló ({p2}): {p4}")
                    .arg("p0", key)
                    .arg("hook", hook)
                    .arg("p2", code)
                    .arg("p4", stderr.trim()),
            );
        }
        if !stderr.trim().is_empty() {
            (context.log)(raw_line(format!("{key}: {}", stderr.trim())));
        }
        if let Some(name) = first_link(payload)? {
            return bail(
                Message::new("{p0}: {hook} creó un enlace no gestionado: {p2}")
                    .arg("p0", key)
                    .arg("hook", hook)
                    .arg("p2", name),
            );
        }
        Ok(())
    }
}

impl Hooks for PowerShellHooks {
    fn unsupported(&self, _manifest: &Map<String, Value>) -> Option<String> {
        None
    }

    fn pre_install(&self, context: &HookContext<'_>) -> Result<()> {
        let step = Step {
            script: context.manifest.get("pre_install").cloned(),
            file: None,
            arguments: None,
        };
        self.run(context, "pre_install", &step)
    }

    fn installer(&self, context: &HookContext<'_>) -> Result<()> {
        let Some(installer) = installer_definition(context.manifest)? else {
            return Ok(());
        };
        if let Some(file) = installer.get("file") {
            let name = py_str(file);
            let path = inside(context.payload, &name)?;
            if !path.is_file() {
                return bail(Message::new("No existe {p1}").arg("p1", name));
            }
            let step = Step {
                script: None,
                file: Some(path),
                arguments: installer.get("args").cloned(),
            };
            self.run(context, "installer.file", &step)
        } else {
            let step = Step {
                script: installer.get("script").cloned(),
                file: None,
                arguments: None,
            };
            self.run(context, "installer.script", &step)
        }
    }
}

/// Lo que ejecuta un hook: un texto de script o un archivo `.ps1` con argumentos.
struct Step {
    script: Option<Value>,
    file: Option<PathBuf>,
    arguments: Option<Value>,
}

/// Texto del script: las líneas de una lista se unen con `\n`.
fn script_text(script: Option<&Value>) -> String {
    match script {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(lines)) => lines.iter().map(py_str).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// `arguments.split(value)`: texto partido como `CommandLineToArgvW`, lista tal cual.
fn argument_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(py_str).collect(),
        Some(other) if py_truthy(other) => split(&py_str(other)),
        _ => Vec::new(),
    }
}

/// Sustituye `$dir`/`${dir}` y el resto de variables documentadas en un argumento.
fn substitute(argument: &str, values: &[(&str, String)]) -> String {
    ARGUMENT_VARIABLE
        .replace_all(argument, |captures: &regex::Captures<'_>| {
            let name = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map_or("", |m| m.as_str());
            values
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        })
        .into_owned()
}

fn parent_of(payload: &Path) -> Result<&Path> {
    payload.parent().ok_or_else(|| {
        Error::io(
            payload,
            std::io::Error::other("el payload no tiene carpeta padre"),
        )
    })
}

/// `powershell.exe` de Windows PowerShell, por ruta absoluta si existe (el `PATH` del
/// hook puede ser el del host y no incluir `System32`).
fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// Entorno del hook (`run_script` del original), en este orden: `PATH` del host,
/// variables activas fuera, proxy (F9), carpetas de `home/` y limpieza de `MITO_*`.
fn environment(context: &HookContext<'_>) -> Result<Env> {
    let mut env = process_env();
    let path = env
        .get("MITO_HOST_PATH")
        .or_else(|| env.get("PATH"))
        .cloned()
        .unwrap_or_default();
    env.insert("PATH".to_owned(), path);
    if let Some(active) = env.get("MITO_ACTIVE_VARS").cloned() {
        for name in active.split(';').filter(|name| !name.is_empty()) {
            env.remove(&name.to_uppercase());
        }
    }
    context
        .network
        .proxy_environment(context.layout, &mut env)?;
    let home = context.layout.home();
    let drive = match home.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().into_owned(),
        _ => String::new(),
    };
    let home_text = home.display().to_string();
    let temp = home.join("tmp").display().to_string();
    for (name, value) in [
        ("HOME", home_text.clone()),
        ("USERPROFILE", home_text.clone()),
        (
            "APPDATA",
            home.join("AppData").join("Roaming").display().to_string(),
        ),
        (
            "LOCALAPPDATA",
            home.join("AppData").join("Local").display().to_string(),
        ),
        ("TEMP", temp.clone()),
        ("TMP", temp),
        ("HOMEDRIVE", drive.clone()),
        ("HOMEPATH", home_text[drive.len()..].to_owned()),
        ("MITO_ROOT", context.layout.root().display().to_string()),
    ] {
        env.insert(name.to_owned(), value);
    }
    env.remove("MITO_PROFILE");
    env.remove("MITO_ACTIVE_VARS");
    Ok(env)
}

/// Primer enlace o junction bajo `payload` (sin entrar en ellos): su nombre.
fn first_link(payload: &Path) -> Result<Option<String>> {
    let mut pending = vec![payload.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).map_err(io_error(&folder))? {
            let path = entry.map_err(io_error(&folder))?.path();
            if is_link(&path) {
                return Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()));
            }
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(None)
}

/// Extractor de los helpers `Expand-*` (`kind` de `mito hook-expand`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandKind {
    /// `Expand-7zipArchive`: 7-Zip privado.
    SevenZip,
    /// `Expand-InnoArchive`: `innounp` privado.
    Inno,
    /// `Expand-DarkArchive`: instaladores Burn de `WiX`.
    Dark,
    /// `Expand-MsiArchive`: paquetes MSI.
    Msi,
}

impl ExpandKind {
    /// Interpreta el `kind` del protocolo: `7zip`, `inno`, `dark` o `msi`.
    ///
    /// # Errors
    ///
    /// `Extractor no soportado: {kind}`.
    pub fn parse(kind: &str) -> Result<Self> {
        match kind {
            "7zip" => Ok(Self::SevenZip),
            "inno" => Ok(Self::Inno),
            "dark" => Ok(Self::Dark),
            "msi" => Ok(Self::Msi),
            other => bail(Message::new("Extractor no soportado: {p1}").arg("p1", other)),
        }
    }

    fn outside_message(self) -> &'static str {
        match self {
            Self::SevenZip => "Expand-7zipArchive requiere rutas dentro del payload",
            Self::Inno => "Expand-InnoArchive requiere rutas dentro del payload",
            Self::Dark => "Expand-DarkArchive requiere rutas dentro del payload",
            Self::Msi => "Expand-MsiArchive requiere rutas dentro del payload",
        }
    }

    fn missing_message(self) -> &'static str {
        match self {
            Self::SevenZip => "Expand-7zipArchive: ExtractDir no encontrado",
            Self::Inno => "Expand-InnoArchive: ExtractDir no encontrado",
            Self::Dark => "Expand-DarkArchive: ExtractDir no encontrado",
            Self::Msi => "Expand-MsiArchive: ExtractDir no encontrado",
        }
    }
}

/// Ruta absoluta, sin `.`/`..` y con los enlaces del prefijo existente resueltos
/// (`Path.resolve()`).
fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    resolve_lenient(&absolute)
}

/// Extrae `archive` en un directorio temporal junto al payload y copia el resultado en
/// `destination` (`expand`, `expand_inno` y `expand_installer` del original).
///
/// `archive` y `destination` deben estar dentro de `payload`. `extract_dir` es la carpeta
/// del contenido extraído que se copia (para Inno, la constante `{app}` o
/// `{code_Función}`). `log` recibe el progreso de los extractores de MSI y Burn.
///
/// # Errors
///
/// `Expand-<Helper>Archive requiere rutas dentro del payload`, `Expand-<Helper>Archive:
/// ExtractDir no encontrado` y los de cada extractor.
pub fn hook_expand(
    kind: ExpandKind,
    layout: &Layout,
    payload: &Path,
    archive: &Path,
    destination: &Path,
    extract_dir: &str,
    log: &dyn Fn(Message),
) -> Result<()> {
    let payload = resolve(payload);
    let archive = resolve(archive);
    let destination = resolve(destination);
    if !archive.starts_with(&payload) || !destination.starts_with(&payload) {
        return bail(Message::new(kind.outside_message()));
    }
    let constant = if kind == ExpandKind::Inno {
        match inno_root(extract_dir) {
            Some(constant) => Some(constant),
            None => return bail(Message::new(kind.missing_message())),
        }
    } else {
        None
    };
    let stage = parent_of(&payload)?;
    let temporary = tempfile::tempdir_in(stage).map_err(io_error(stage))?;
    let expanded = temporary.path();
    let sevenzip = layout.sevenzip_exe();
    match kind {
        ExpandKind::SevenZip => extract_7zip(&archive, expanded, Some(&sevenzip))?,
        ExpandKind::Inno => extract_inno(
            &archive,
            expanded,
            &layout.innounp_exe(),
            constant.unwrap_or_default(),
        )?,
        ExpandKind::Dark => extract_burn(&archive, expanded, Some(&sevenzip), log)?,
        ExpandKind::Msi => extract_msi(&archive, expanded, Some(&sevenzip), log)?,
    }
    // Inno ya elige su raíz con la constante; el resto, con `extract_dir`.
    let source = if kind == ExpandKind::Inno {
        expanded.to_path_buf()
    } else {
        inside(expanded, extract_dir)?
    };
    if !source.is_dir() {
        return bail(Message::new(kind.missing_message()));
    }
    copy_dir(&source, &destination).map_err(io_error(&destination))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_preambulo_expone_los_helpers_genericos() {
        for text in [
            "function Expand-InnoArchive",
            "function Get-HelperPath",
            "function Invoke-ExternalCommand",
            "& $context.mito hook-expand $Kind $context.root $dir $Path $DestinationPath $ExtractDir",
            "Invoke-MitoExpand 'inno' 'Expand-InnoArchive'",
            "Invoke-MitoExpand '7zip' 'Expand-7zipArchive'",
        ] {
            assert!(SCRIPT.contains(text), "{text}");
        }
        assert!(!SCRIPT.contains("python"));
    }

    #[test]
    fn los_argumentos_solo_sustituyen_las_variables_documentadas() {
        let values = [
            ("dir", "C:\\p q".to_owned()),
            ("original_dir", "C:\\p q".to_owned()),
            ("persist_dir", "C:\\persist".to_owned()),
            ("version", "1".to_owned()),
            ("architecture", "64bit".to_owned()),
            ("global", "False".to_owned()),
        ];
        let cases = [
            ("$dir", "C:\\p q"),
            ("${version}x", "1x"),
            ("$global", "False"),
            ("$dir_suffix", "$dir_suffix"),
            ("$(throw \"x\")", "$(throw \"x\")"),
            ("$Dir", "$Dir"),
            ("$architecture/$persist_dir", "64bit/C:\\persist"),
        ];
        for (input, expected) in cases {
            assert_eq!(substitute(input, &values), expected, "{input}");
        }
    }

    #[test]
    fn los_argumentos_de_texto_se_parten_y_los_de_lista_no() {
        assert_eq!(
            argument_list(Some(&json!("$dir \"two words\""))),
            ["$dir", "two words"]
        );
        assert_eq!(argument_list(Some(&json!(["a b", ""]))), ["a b", ""]);
        assert_eq!(argument_list(None), Vec::<String>::new());
        assert_eq!(argument_list(Some(&json!(""))), Vec::<String>::new());
    }

    #[test]
    fn el_script_de_lista_une_las_lineas() {
        assert_eq!(script_text(Some(&json!(["a", "b"]))), "a\nb");
        assert_eq!(script_text(Some(&json!("x"))), "x");
        assert_eq!(script_text(None), "");
    }

    #[test]
    fn el_tipo_de_extractor_es_cerrado() {
        assert_eq!(ExpandKind::parse("7zip").unwrap(), ExpandKind::SevenZip);
        assert_eq!(ExpandKind::parse("msi").unwrap(), ExpandKind::Msi);
        assert_eq!(
            ExpandKind::parse("rar").unwrap_err().to_string(),
            "Extractor no soportado: rar"
        );
    }
}
