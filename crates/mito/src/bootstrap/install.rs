//! Instalación de una herramienta privada: descarga, verificación, extracción en staging
//! y publicación con un único `rename` (`ensure_tools` del original).

use std::fs;
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

use mito_core::diagnostics::{self, LogLevel};
use mito_core::storage::{inside, write_json};
use mito_core::{Layout, Message, Operation};
use mito_engine::Downloader;
use serde_json::json;

use super::lock::{Artifact, Tool, incomplete};

/// `CREATE_NO_WINDOW`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Operación de los registros de las herramientas.
pub const OPERATION: &str = "bootstrap.tools";

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> mito_core::Error {
    let path = path.to_path_buf();
    move |source| mito_core::Error::io(path, source)
}

/// Registra el inicio de una fase (como `step` del original).
fn phase(layout: &Layout, name: &str, tool: &Tool) {
    diagnostics::write(
        layout,
        LogLevel::Info,
        OPERATION,
        name,
        &[("component", json!(tool.id))],
    );
}

/// `shutil.copytree(dirs_exist_ok=True)`: copia `source` dentro de `destination`.
pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), mito_core::Error> {
    fs::create_dir_all(destination).map_err(io(destination))?;
    for entry in fs::read_dir(source).map_err(io(source))? {
        let entry = entry.map_err(io(source))?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(io(&entry.path()))?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(io(&target))?;
        }
    }
    Ok(())
}

/// Instala un artefacto en `payload` (`stage` es la carpeta temporal de la herramienta).
fn install_artifact(
    layout: &Layout,
    tool: &Tool,
    artifact: &Artifact,
    stage: &Path,
    payload: &Path,
    downloader: &dyn Downloader,
    operation: &Operation,
) -> anyhow::Result<()> {
    let archive = inside(stage, &artifact.file_name)?;
    phase(layout, "download-and-hash", tool);
    downloader.download(&artifact.url, &archive, Some(&artifact.sha256), operation)?;
    let install = &artifact.install;
    match install.kind.as_str() {
        "file" => {
            let target = install.target.as_deref().unwrap_or(&artifact.file_name);
            let target = inside(payload, target)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io(parent))?;
            }
            fs::copy(&archive, &target).map_err(io(&target))?;
        }
        "zip" => {
            let expanded = stage.join("expanded");
            phase(layout, "extract-zip", tool);
            mito_transfer::extract_zip(&archive, &expanded)?;
            let source = match install.source.as_deref() {
                None | Some("." | "") => expanded.clone(),
                Some(source) => inside(&expanded, source)?,
            };
            copy_tree(&source, payload)?;
        }
        "sevenZip" => {
            phase(layout, "extract-7zip", tool);
            let executable = install
                .executable
                .as_deref()
                .ok_or_else(|| incomplete(&tool.id))?;
            let executable = inside(payload, executable)?;
            let mut output_option = std::ffi::OsString::from("-o");
            output_option.push(payload);
            let output = Command::new(&executable)
                .arg("x")
                .arg(&archive)
                .arg(output_option)
                .arg("-y")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(io(&executable))?;
            if !output.status.success() {
                // Sin argumentos del comando en el mensaje.
                let code = output.status.code().unwrap_or(1);
                return Err(mito_core::Error::domain(
                    Message::new("{p1}").arg("p1", diagnostics::child_failed_message(code)),
                )
                .into());
            }
        }
        _ => {
            return Err(mito_core::Error::domain(Message::new(
                "Formato de herramienta core desconocido",
            ))
            .into());
        }
    }
    Ok(())
}

/// Instala `tool` en `core/tools/<id>` sustituyendo una versión anterior si la hay.
///
/// Todo ocurre en un directorio temporal de `core/tools` (mismo volumen, para que el
/// `rename` final sea atómico) que se borra siempre, también si falla. La carpeta
/// anterior solo se retira después de preparar la nueva por completo y se restaura si
/// la publicación falla.
///
/// # Errors
///
/// Los de descarga (incluido el hash), extracción y E/S; `Herramienta incompleta: <id>`
/// si tras instalar falta un fichero requerido.
pub fn install_tool(
    layout: &Layout,
    tool: &Tool,
    downloader: &dyn Downloader,
    operation: &Operation,
) -> anyhow::Result<()> {
    let destination = tool.folder(layout);
    let tools = layout.tools();
    fs::create_dir_all(&tools).map_err(io(&tools))?;
    diagnostics::write(
        layout,
        LogLevel::Info,
        OPERATION,
        "Preparing tool",
        &[("component", json!(tool.id))],
    );
    let temporary = tempfile::Builder::new()
        .prefix("tmp")
        .tempdir_in(&tools)
        .map_err(io(&tools))?;
    let stage = temporary.path();
    let payload = stage.join("payload");
    fs::create_dir(&payload).map_err(io(&payload))?;
    for artifact in &tool.artifacts {
        install_artifact(
            layout, tool, artifact, stage, &payload, downloader, operation,
        )?;
    }
    for required in &tool.required {
        if !inside(&payload, required)?.is_file() {
            return Err(incomplete(&tool.id).into());
        }
    }
    if tool.id == "windows-terminal" {
        let portable = payload.join(".portable");
        fs::write(&portable, b"").map_err(io(&portable))?;
    }
    write_json(
        &payload.join(".mito-tool.json"),
        &json!({"version": tool.version}),
    )?;
    publish(&payload, &destination, stage)
}

/// Sustituye `destination` por `payload`; la carpeta anterior se mueve antes aparte y se
/// restaura si el `rename` falla (p. ej. una herramienta en uso).
fn publish(payload: &Path, destination: &Path, stage: &Path) -> anyhow::Result<()> {
    let previous = destination.exists().then(|| stage.join("previous"));
    if let Some(previous) = &previous {
        fs::rename(destination, previous).map_err(|source| {
            mito_core::Error::domain(
                Message::new("No se pudo sustituir la herramienta: {p1} ({p2})")
                    .arg("p1", destination.display())
                    .arg("p2", source),
            )
        })?;
    }
    if let Err(source) = fs::rename(payload, destination) {
        if let Some(previous) = &previous {
            let _ = fs::rename(previous, destination);
        }
        return Err(mito_core::Error::io(destination, source).into());
    }
    Ok(())
}
