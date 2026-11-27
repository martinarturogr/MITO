//! Tests portados de `core/tests/test_cli.py` (`__main__.py`) y de los subcomandos de F4.
//!
//! El motor se construye con puntos de extensión de prueba: un descargador que sirve un
//! ZIP en memoria y unos `Jobs` que registran los lanzamientos `--owned` y `--coupled`
//! en lugar de crear procesos. Ninguno sale a Internet.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::Parser as _;
use mito_core::Operation;
use mito_engine::{Downloader, Engine, Extensions, Jobs, LaunchCommand};
use serde_json::json;
use zip::write::SimpleFileOptions;

use super::{OWNED_PAUSE, dispatch};
use crate::cli::Cli;
use crate::console::testing::console;

const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// ZIP con `tool.cmd` (sale con 0).
fn zip_bytes() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file("tool.cmd", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"@echo off\r\nexit /b 0\r\n").unwrap();
    writer.finish().unwrap().into_inner()
}

struct Fake;

impl Downloader for Fake {
    fn download(
        &self,
        _url: &str,
        destination: &Path,
        _expected: Option<&str>,
        _operation: &Operation,
    ) -> mito_transfer::Result<()> {
        fs::write(destination, zip_bytes()).map_err(|e| mito_core::Error::io(destination, e))?;
        Ok(())
    }
}

/// Registro de los lanzamientos con Job.
#[derive(Default)]
struct Recorder {
    owned: Mutex<Vec<LaunchCommand>>,
    coupled: Mutex<Vec<CoupledCall>>,
    code: i32,
    fail: bool,
}

/// Perfil, componente, argumentos y shortcut de un lanzamiento acoplado.
type CoupledCall = (String, String, Vec<String>, Option<String>);

struct Shared(Arc<Recorder>);

impl Jobs for Shared {
    fn owned(&self, command: &LaunchCommand) -> mito_engine::Result<i32> {
        self.0.owned.lock().unwrap().push(command.clone());
        if self.0.fail {
            return Err(mito_engine::Error::io(
                "tool.cmd",
                std::io::Error::other("Cannot start"),
            ));
        }
        Ok(self.0.code)
    }

    fn coupled(
        &self,
        _engine: &Engine,
        profile: &str,
        key: &str,
        _title: &str,
        arguments: &[String],
        shortcut: Option<&str>,
    ) -> mito_engine::Result<i32> {
        self.0.coupled.lock().unwrap().push((
            profile.to_owned(),
            key.to_owned(),
            arguments.to_vec(),
            shortcut.map(str::to_owned),
        ));
        Ok(self.0.code)
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    engine: Engine,
    jobs: Arc<Recorder>,
}

struct Outcome {
    code: i32,
    out: String,
    err: String,
}

/// Instancia con `extras/tool` instalado y activo en el profile `one`.
fn rig(code: i32, fail: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let jobs = Arc::new(Recorder {
        code,
        fail,
        ..Recorder::default()
    });
    let extensions = Extensions {
        downloader: Box::new(Fake),
        jobs: Box::new(Shared(Arc::clone(&jobs))),
        ..Extensions::default()
    };
    let engine = Engine::with_extensions(dir.path(), extensions);
    engine.initialize().unwrap();
    let manifest = json!({
        "version": "1.0",
        "url": "https://example.org/tool.zip",
        "hash": ZERO_HASH,
        "bin": [["tool.cmd", "tool"]],
        "shortcuts": [["tool.cmd", "Tool"], ["tool.cmd", "Tool Help", "--help"]],
    });
    let snapshot = engine.layout().buckets().join("extras.json");
    fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
    fs::write(
        snapshot,
        serde_json::to_vec(&json!({"schema": 1, "apps": {"tool": manifest}})).unwrap(),
    )
    .unwrap();
    let rig = Rig {
        _dir: dir,
        engine,
        jobs,
    };
    assert_eq!(cli(&rig, &["profile", "create", "one"], "").code, 0);
    let install = cli(&rig, &["install", "extras/tool", "--profile", "one"], "");
    assert_eq!(install.code, 0, "{}", install.err);
    rig
}

fn cli(rig: &Rig, args: &[&str], stdin: &str) -> Outcome {
    let parsed = Cli::try_parse_from(std::iter::once("mito").chain(args.iter().copied())).unwrap();
    let command = parsed.command.unwrap();
    let (console, out, err) = console(stdin);
    let code = dispatch(
        &rig.engine,
        &command,
        &console,
        &Operation::silent(),
        &AtomicBool::new(false),
    );
    Outcome {
        code,
        out: out.text(),
        err: err.text(),
    }
}

/// Argumentos que recibió el script: `MITO_COMMAND_ARG_1…`.
fn forwarded(command: &LaunchCommand) -> Vec<String> {
    (1..)
        .map_while(|i| command.env.get(&format!("MITO_COMMAND_ARG_{i}")).cloned())
        .collect()
}

#[test]
fn owned_reenvia_los_argumentos_y_pausa_con_el_codigo_de_fallo() {
    let rig = rig(7, false);
    let result = cli(
        &rig,
        &[
            "run",
            "--owned",
            "--profile",
            "one",
            "--app",
            "extras/tool",
            "--",
            "--name",
            "two words",
        ],
        "",
    );
    assert_eq!(result.code, 7);
    assert_eq!(
        result.out,
        format!("La aplicación terminó con el código 7.\n{OWNED_PAUSE}")
    );
    let owned = rig.jobs.owned.lock().unwrap();
    assert_eq!(forwarded(&owned[0]), ["--name", "two words"]);
}

#[test]
fn owned_sin_argumentos_pausa_aunque_el_codigo_sea_cero() {
    let rig = rig(0, false);
    let result = cli(
        &rig,
        &["run", "--owned", "--profile", "one", "--app", "extras/tool"],
        "\n",
    );
    assert_eq!(result.code, 0);
    assert_eq!(
        result.out,
        format!("La aplicación terminó con el código 0.\n{OWNED_PAUSE}")
    );
    assert_eq!(forwarded(&rig.jobs.owned.lock().unwrap()[0]).len(), 0);
}

#[test]
fn un_componente_independiente_no_pausa() {
    let rig = rig(0, false);
    let result = cli(
        &rig,
        &["run", "--profile", "one", "--app", "extras/tool"],
        "",
    );
    assert_eq!(result.code, 0);
    assert_eq!(result.out, "");
    assert!(rig.jobs.owned.lock().unwrap().is_empty());
}

#[test]
fn un_fallo_al_arrancar_con_owned_pausa_y_sale_con_uno() {
    let rig = rig(0, true);
    let result = cli(
        &rig,
        &["run", "--owned", "--profile", "one", "--app", "extras/tool"],
        "",
    );
    assert_eq!(result.code, 1);
    assert!(result.err.starts_with("MITO: "), "{}", result.err);
    assert!(result.err.contains("Cannot start"));
    assert_eq!(
        result.out,
        format!("La aplicación terminó con el código 1.\n{OWNED_PAUSE}")
    );
}

#[test]
fn run_reenvia_el_shortcut_elegido_y_el_modo() {
    for (mode, coupled) in [
        (None, false),
        (Some("--coupled"), true),
        (Some("--owned"), false),
    ] {
        let rig = rig(0, false);
        let mut args = vec![
            "run",
            "--profile",
            "one",
            "--app",
            "extras/tool",
            "--shortcut",
            "Tool Help",
        ];
        args.extend(mode);
        args.extend(["--", "two words"]);
        let result = cli(&rig, &args, "");
        assert_eq!(result.code, 0, "{mode:?}: {}", result.err);
        if coupled {
            let calls = rig.jobs.coupled.lock().unwrap();
            assert_eq!(
                calls[0],
                (
                    "one".to_owned(),
                    "extras/tool".to_owned(),
                    vec!["two words".to_owned()],
                    Some("Tool Help".to_owned())
                )
            );
        } else if mode.is_some() {
            // El shortcut añade `--help` antes de los argumentos del usuario.
            let owned = rig.jobs.owned.lock().unwrap();
            assert_eq!(forwarded(&owned[0]), ["--help", "two words"]);
        }
    }
}

#[test]
fn un_comando_de_bin_devuelve_el_codigo_del_proceso() {
    let rig = rig(0, false);
    let result = cli(
        &rig,
        &["run", "--profile", "one", "--app", "tool", "--bin", "tool"],
        "",
    );
    assert_eq!(result.code, 0);
}

#[test]
fn los_comandos_de_otras_fases_no_estan_disponibles() {
    let rig = rig(0, false);
    for (args, name) in [
        (vec!["ui"], "ui"),
        (vec!["start"], "start"),
        (vec!["bootstrap"], "bootstrap"),
        (vec!["terminal", "--profile", "one"], "terminal"),
        (vec!["migrate-storage"], "migrate-storage"),
        (vec!["release", "--local"], "release"),
        (vec!["shortcut", "--profile", "one"], "shortcut"),
        (
            vec!["app-shortcut", "extras/tool", "--profile", "one"],
            "app-shortcut",
        ),
        (vec!["host-run", "id", "--profile", "one"], "host-run"),
        (
            vec![
                "pocketool-run",
                "--profile",
                "one",
                "--package",
                "p",
                "--entry",
                "e",
            ],
            "pocketool-run",
        ),
        (
            vec!["profile", "export", "one", "out.zip"],
            "profile export",
        ),
        (
            vec!["profile", "import", "in.zip", "--name", "two"],
            "profile import",
        ),
        (
            vec![
                "mcp-server",
                "--pipe",
                "p",
                "--owner",
                "1",
                "--session",
                "s",
            ],
            "mcp-server",
        ),
    ] {
        let result = cli(&rig, &args, "");
        assert_eq!(result.code, 1, "{name}");
        assert_eq!(
            result.err,
            format!("MITO: Comando no disponible todavía: {name}\n")
        );
        assert_eq!(result.out, "");
    }
}

#[test]
fn ctrl_c_sale_con_130() {
    let rig = rig(0, false);
    let parsed = Cli::try_parse_from(["mito", "installed"]).unwrap();
    let (console, out, err) = console("");
    let code = dispatch(
        &rig.engine,
        &parsed.command.unwrap(),
        &console,
        &Operation::silent(),
        &AtomicBool::new(true),
    );
    assert_eq!(code, 130);
    assert_eq!(err.text(), "Operación interrumpida\n");
    assert_eq!(out.text().lines().count(), 1);
}

#[test]
fn los_errores_de_dominio_salen_sin_cadena_de_causas() {
    let rig = rig(0, false);
    let result = cli(&rig, &["profile", "show", "missing"], "");
    assert_eq!(result.code, 1);
    assert_eq!(result.err.lines().count(), 1);
    assert!(result.err.starts_with("MITO: "));
}
