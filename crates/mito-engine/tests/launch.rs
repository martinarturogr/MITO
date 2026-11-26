//! Tests de lanzamiento (`launch`, `shell`, entradas de aplicación) y de los puntos de
//! extensión por defecto. Los `.cmd` se ejecutan de verdad con `cmd.exe`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

mod common;

use std::fs;

use common::{ZERO_HASH, error_text, fx, zip_of};
use mito_engine::{LaunchOptions, ShellKind};
use serde_json::{Value, json};

fn script_fixture(script: &str, bin: Value) -> common::Fx {
    let f = fx();
    f.bucket(
        "main",
        json!({"tool": {"version": "1", "url": "https://example.org/t.zip", "hash": ZERO_HASH,
                        "bin": bin, "shortcuts": [["tool.cmd", "Tool"]]}}),
    );
    f.serve(
        "https://example.org/t.zip",
        zip_of(&[("tool.cmd", script.as_bytes())]),
    );
    f.profile("p");
    f.install(&["main/tool"], Some("p")).unwrap();
    f
}

fn options<'a>(app: &'a str, binary: Option<&'a str>, arguments: &[&str]) -> LaunchOptions<'a> {
    LaunchOptions {
        app: Some(app),
        binary,
        arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
        ..LaunchOptions::default()
    }
}

#[test]
fn los_argumentos_de_un_cmd_viajan_por_variables_tras_los_del_manifiesto() {
    let f = script_fixture(
        "@echo off\r\nsetlocal EnableDelayedExpansion\r\nset \"out=%MITO_ROOT%\\out.txt\"\r\n(\r\n  echo [%~1]\r\n  echo [%~2]\r\n  set \"third=%~3\"\r\n  echo [!third!]\r\n  set \"fourth=%~4\"\r\n  echo [!fourth!]\r\n) > \"!out!\"\r\n",
        json!([["tool.cmd", "tool", "fixed \"two words\""]]),
    );
    let (command, _) = f
        .engine
        .launch_command("p", &options("main/tool", Some("tool"), &["user", "a & b"]))
        .unwrap()
        .unwrap();
    let get = |index: usize| command.env[&format!("MITO_COMMAND_ARG_{index}")].clone();
    assert_eq!(
        get(0),
        f.path("core/apps/main/tool/tool.cmd").display().to_string()
    );
    assert_eq!(
        [get(1), get(2), get(3), get(4)],
        ["fixed", "two words", "user", "a & b"]
    );
    assert_eq!(
        command.raw_args.as_deref(),
        Some(
            "/d /v:off /s /c \"\"%MITO_COMMAND_ARG_0%\" %MITO_COMMAND_ARG_1% \"%MITO_COMMAND_ARG_2%\" %MITO_COMMAND_ARG_3% \"%MITO_COMMAND_ARG_4%\"\""
        )
    );
    let code = f
        .engine
        .launch("p", &options("main/tool", Some("tool"), &["user", "a & b"]))
        .unwrap();
    assert_eq!(code, 0);
    let out = fs::read_to_string(f.path("out.txt")).unwrap();
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["[fixed]", "[two words]", "[user]", "[a & b]"]
    );
}

#[test]
fn un_subcomando_simple_llega_sin_comillas_y_el_codigo_de_salida_se_devuelve() {
    let f = script_fixture(
        "@echo off\r\nif \"%1\"==\"run\" (exit /b 0) else (exit /b 7)\r\n",
        json!([["tool.cmd", "tool"]]),
    );
    assert_eq!(
        f.engine
            .launch("p", &options("tool", Some("tool"), &["run"]))
            .unwrap(),
        0
    );
    assert_eq!(
        f.engine
            .launch("p", &options("tool", Some("TOOL"), &["other"]))
            .unwrap(),
        7
    );
}

#[test]
fn un_comando_de_consola_trabaja_en_el_directorio_del_llamador_y_una_app_en_el_workspace() {
    let f = script_fixture("@echo off\r\n", json!([["tool.cmd", "tool"]]));
    let (binary, _) = f
        .engine
        .launch_command("p", &options("main/tool", Some("tool"), &[]))
        .unwrap()
        .unwrap();
    assert_eq!(binary.cwd, std::env::current_dir().unwrap());
    let (app, _) = f
        .engine
        .launch_command("p", &options("main/tool", None, &[]))
        .unwrap()
        .unwrap();
    assert_eq!(app.cwd, f.path("workspaces/p"));
    // Una aplicación se lanza sin esperar.
    assert_eq!(
        f.engine.launch("p", &options("tool", None, &[])).unwrap(),
        0
    );
}

#[test]
fn errores_de_seleccion_y_modos_no_validos() {
    let f = script_fixture("@echo off\r\n", json!([["tool.cmd", "tool"]]));
    let launch = |o: LaunchOptions<'_>| error_text(f.engine.launch("p", &o));
    assert_eq!(
        launch(options("ghost", None, &[])),
        "ghost no está seleccionado en p"
    );
    assert_eq!(
        launch(options("tool", Some("nope"), &[])),
        "Comando no encontrado: nope"
    );
    let mut both = options("tool", Some("tool"), &[]);
    both.shortcut = Some("Tool");
    assert_eq!(launch(both), "Modo de lanzamiento no válido");
    let mut bad_shortcut = options("tool", None, &[]);
    bad_shortcut.shortcut = Some("Nope");
    assert_eq!(
        launch(bad_shortcut),
        "Shortcut unavailable or ambiguous: Nope"
    );
    let mut coupled = options("tool", None, &[]);
    coupled.coupled = true;
    assert_eq!(
        launch(coupled),
        "Modo de lanzamiento no soportado todavía: --coupled"
    );
    let mut owned = options("tool", None, &[]);
    owned.owned = true;
    assert_eq!(
        launch(owned),
        "Modo de lanzamiento no soportado todavía: --owned"
    );
    let mut invalid = options("tool", Some("tool"), &[]);
    invalid.coupled = true;
    assert_eq!(launch(invalid), "Modo de lanzamiento no válido");
    // Sin `app` se abre la terminal del profile, que aún no existe (F5).
    assert_eq!(
        launch(LaunchOptions::default()),
        "Lanzamiento no soportado todavía: terminal del profile"
    );
}

#[test]
fn una_argumento_con_comillas_no_puede_ir_a_un_bat() {
    let f = script_fixture("@echo off\r\n", json!([["tool.cmd", "tool"]]));
    let text = error_text(
        f.engine
            .launch("p", &options("tool", Some("tool"), &["a\"b"])),
    );
    assert_eq!(
        text,
        "INVALID_ARGUMENT: BAT/CMD argv cannot contain quotes or newlines; use an explicit script"
    );
}

#[test]
fn jar_ico_y_java_del_profile() {
    let f = fx();
    f.bucket(
        "main",
        json!({
            "java": {"version": "1", "url": "https://example.org/java.zip", "hash": ZERO_HASH,
                     "bin": "bin/java.exe", "env_add_path": "bin"},
            "app": {"version": "1", "url": "https://example.org/app.jar", "hash": ZERO_HASH,
                    "bin": [["app.jar", "app"]]},
            "icon": {"version": "1", "url": "https://example.org/icon.ico", "hash": ZERO_HASH},
        }),
    );
    let jar: Vec<u8> = zip_of(&[("Main.class", b"x")]);
    f.serve("https://example.org/app.jar", jar.clone());
    f.serve(
        "https://example.org/java.zip",
        zip_of(&[("bin/java.exe", b"java")]),
    );
    f.serve("https://example.org/icon.ico", vec![0, 0, 1, 0, 1, 0]);
    f.profile("p");
    f.install(&["main/app"], Some("p")).unwrap();
    // El JAR es un ZIP, pero se conserva byte a byte.
    assert_eq!(fs::read(f.path("core/apps/main/app/app.jar")).unwrap(), jar);
    assert_eq!(
        error_text(
            f.engine
                .launch_command_from("p", &options("app", Some("app"), &[]), common::base_env())
                .map(|_| ())
        ),
        "Comando no encontrado: java.exe"
    );
    f.install(&["main/java"], Some("p")).unwrap();
    let (command, _) = f
        .engine
        .launch_command_from(
            "p",
            &options("app", Some("app"), &["--x"]),
            common::base_env(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        command.program,
        f.path("core/apps/main/java/bin/java.exe")
            .display()
            .to_string()
    );
    assert_eq!(
        command.args,
        [
            "-jar".to_owned(),
            f.path("core/apps/main/app/app.jar").display().to_string(),
            "--x".to_owned()
        ]
    );
    f.install(&["main/icon"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/icon/icon.ico")).unwrap(),
        [0, 0, 1, 0, 1, 0]
    );
}

#[test]
fn comandos_ps1_y_chm() {
    let f = fx();
    f.bucket(
        "main",
        json!({"docs": {"version": "1", "url": "https://example.org/d.zip", "hash": ZERO_HASH,
                        "bin": [["run.ps1", "run", "-Flag 1"]],
                        "shortcuts": [["help.chm", "Help"], ["run.ps1", "Run"]]}}),
    );
    f.serve(
        "https://example.org/d.zip",
        zip_of(&[("run.ps1", b""), ("help.chm", b"")]),
    );
    f.profile("p");
    f.install(&["main/docs"], Some("p")).unwrap();
    let docs = f
        .engine
        .installed()
        .unwrap()
        .get("main/docs")
        .cloned()
        .unwrap();
    // El `.chm` se ordena el último y las etiquetas salen de shortcuts y bin.
    let entries = f.engine.application_entries(&docs).unwrap();
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["Help", "Run", "run"]);
    assert_eq!(
        f.engine.application_target(&docs, None).unwrap().0,
        "run.ps1"
    );
    assert_eq!(
        f.engine.application_target(&docs, Some("Help")).unwrap().0,
        "help.chm"
    );
    assert_eq!(f.engine.application_name(&docs, None).unwrap(), "Run");
    assert_eq!(
        f.engine.application_name(&docs, Some("Help")).unwrap(),
        "Help"
    );
    assert!(f.engine.can_open(&docs));
    let (ps1, _) = f
        .engine
        .launch_command("p", &options("docs", Some("run"), &["extra"]))
        .unwrap()
        .unwrap();
    assert_eq!(ps1.program, "powershell.exe");
    assert_eq!(
        ps1.args[..3],
        [
            "-NoLogo".to_owned(),
            "-NoProfile".to_owned(),
            "-File".to_owned()
        ]
    );
    assert_eq!(
        ps1.args[4..],
        ["-Flag".to_owned(), "1".to_owned(), "extra".to_owned()]
    );
    let mut chm = options("docs", None, &[]);
    chm.shortcut = Some("Help");
    let (chm, _) = f.engine.launch_command("p", &chm).unwrap().unwrap();
    assert!(
        chm.program.to_lowercase().ends_with("hh.exe"),
        "{}",
        chm.program
    );
}

#[test]
fn una_app_sin_ejecutable_declarado_se_usa_desde_la_terminal() {
    let f = fx();
    f.bucket(
        "main",
        json!({"lib": {"version": "1", "url": "https://example.org/l.zip", "hash": ZERO_HASH,
                       "env_add_path": "."}}),
    );
    f.serve("https://example.org/l.zip", zip_of(&[("tool.exe", b"x")]));
    f.install(&["main/lib"], None).unwrap();
    let lib = f
        .engine
        .installed()
        .unwrap()
        .get("main/lib")
        .cloned()
        .unwrap();
    assert!(!f.engine.can_open(&lib));
    assert_eq!(
        error_text(f.engine.application_target(&lib, None)),
        "La app no declara un ejecutable disponible; úsala desde la terminal"
    );
}

#[test]
fn shell_ejecuta_los_argumentos_en_el_workspace_del_profile() {
    let f = fx();
    f.profile("p");
    let code = f
        .engine
        .shell(
            "p",
            &["cmd.exe".into(), "/c".into(), "exit 3".into()],
            ShellKind::Cmd,
        )
        .unwrap();
    assert_eq!(code, 3);
    let code = f
        .engine
        .shell(
            "p",
            &[
                "cmd.exe".into(),
                "/c".into(),
                "cd > out.txt & echo %MITO_PROFILE%>> out.txt".into(),
            ],
            ShellKind::Powershell,
        )
        .unwrap();
    assert_eq!(code, 0);
    let out = fs::read_to_string(f.path("workspaces/p/out.txt")).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], f.path("workspaces/p").display().to_string());
    assert_eq!(lines[1].trim(), "p");
}
