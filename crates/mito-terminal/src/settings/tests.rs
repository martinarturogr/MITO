//! Tests portados de `core/tests/test_terminal.py` (ajustes del Terminal privado).

use std::fs;
use std::path::Path;

use mito_core::Layout;
use mito_engine::Engine;
use serde_json::{Value, json};

use super::*;
use crate::command::{application_command, command, component_child};

/// Instancia temporal con un Windows Terminal de pega (solo importa que exista).
pub(crate) fn instance() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(dir.path());
    engine.initialize().unwrap();
    let folder = engine.layout().windows_terminal();
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("WindowsTerminal.exe"), b"terminal").unwrap();
    (dir, engine)
}

fn profile(engine: &Engine, name: &str) {
    engine.create_profile(name, None).unwrap();
}

fn settings(engine: &Engine) -> Value {
    let path = engine
        .layout()
        .windows_terminal()
        .join("settings")
        .join("settings.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn raw(engine: &Engine) -> Vec<u8> {
    fs::read(
        engine
            .layout()
            .windows_terminal()
            .join("settings")
            .join("settings.json"),
    )
    .unwrap()
}

fn visible_names(settings: &Value) -> Vec<&str> {
    settings["profiles"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["hidden"] == json!(false))
        .map(|item| item["name"].as_str().unwrap())
        .collect()
}

#[test]
fn los_guid_son_estables_y_coinciden_con_el_original() {
    assert_eq!(
        guid(None, Shell::Cmd),
        "{bf682261-68d8-5c8b-b536-29838d41c9a6}"
    );
    assert_eq!(
        guid(Some("java11"), Shell::PowerShell),
        "{5b8b1a8a-7a45-50a3-ba42-002700a6d109}"
    );
    assert_eq!(
        guid(Some("java11"), Shell::Cmd),
        "{d58b5131-c397-5636-be24-8e58dc2f02ca}"
    );
    assert_eq!(
        guid(Some("java21"), Shell::PowerShell),
        "{3210f90d-0a12-58f5-bd6a-9ffa9d5c819e}"
    );
    assert_eq!(
        guid(Some("java21"), Shell::Cmd),
        "{a9b4fa81-6ba6-55da-80a1-1a7afd1ffd25}"
    );
}

#[test]
fn menu_colores_pantalla_completa_y_actualizacion() {
    let (_dir, engine) = instance();
    profile(&engine, "java11");
    profile(&engine, "java21");
    prepare(&engine, None).unwrap();
    let first = raw(&engine);
    let data = settings(&engine);
    assert_eq!(
        visible_names(&data),
        [
            "CMD (java11)",
            "PowerShell (java11)",
            "PowerShell (java21)",
            "CMD (java21)",
            "MITO"
        ]
    );
    assert_eq!(
        data["defaultProfile"],
        json!(guid(Some("java11"), Shell::Cmd))
    );
    assert_eq!(data["launchMode"], "maximized");
    assert_eq!(data["showTabsInFullscreen"], true);
    assert_eq!(data["profiles"]["defaults"]["opacity"], 94);
    assert_eq!(data["schemes"].as_array().unwrap().len(), 3);
    let backgrounds: std::collections::BTreeSet<&str> = data["schemes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scheme| scheme["background"].as_str().unwrap())
        .collect();
    assert_eq!(backgrounds.len(), 3);
    let powershell = data["profiles"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "PowerShell (java11)")
        .unwrap();
    assert!(
        powershell["commandline"]
            .as_str()
            .unwrap()
            .ends_with(r"core\bin\mito.exe shell --type powershell --profile java11")
    );
    profile(&engine, "java31");
    prepare(&engine, None).unwrap();
    assert_eq!(settings(&engine)["newTabMenu"].as_array().unwrap().len(), 7);
    let second = raw(&engine);
    prepare(&engine, None).unwrap();
    assert_eq!(raw(&engine), second);
    assert_ne!(first, second);
}

#[test]
fn la_predeterminada_sigue_a_la_seleccion_y_sobrevive_al_prepare_de_fondo() {
    let (_dir, engine) = instance();
    profile(&engine, "java11");
    profile(&engine, "java21");
    prepare(&engine, Some("java21")).unwrap();
    for _ in 0..2 {
        let data = settings(&engine);
        let wanted = json!(guid(Some("java21"), Shell::Cmd));
        assert_eq!(data["defaultProfile"], wanted);
        assert_eq!(data["newTabMenu"][0]["profile"], wanted);
        assert_eq!(data["profiles"]["list"][0]["guid"], wanted);
        prepare(&engine, None).unwrap();
    }
    assert_eq!(
        fs::read_to_string(engine.layout().state().join("terminal-profile.json")).unwrap(),
        "\"java21\"\n"
    );
    engine.delete_profile("java21").unwrap();
    prepare(&engine, None).unwrap();
    assert_eq!(
        settings(&engine)["defaultProfile"],
        json!(guid(Some("java11"), Shell::Cmd))
    );
}

#[test]
fn el_shell_predeterminado_y_el_valor_invalido_cae_a_cmd() {
    for (value, expected) in [
        ("powershell", guid(Some("java21"), Shell::PowerShell)),
        ("mito", guid(None, Shell::Cmd)),
        ("cmd", guid(Some("java21"), Shell::Cmd)),
        ("invalid", guid(Some("java21"), Shell::Cmd)),
    ] {
        let (_dir, engine) = instance();
        profile(&engine, "java21");
        fs::write(
            engine.layout().config_file(),
            format!("terminal.defaultShell={value}\n"),
        )
        .unwrap();
        prepare(&engine, None).unwrap();
        let data = settings(&engine);
        assert_eq!(data["defaultProfile"], json!(expected), "{value}");
        assert_eq!(data["newTabMenu"][0]["profile"], json!(expected), "{value}");
        assert_eq!(
            data["profiles"]["list"][0]["guid"],
            json!(expected),
            "{value}"
        );
        let menu = data["newTabMenu"].as_array().unwrap();
        if value != "mito" {
            assert_eq!(
                menu.last().unwrap()["profile"],
                json!(guid(None, Shell::Cmd)),
                "{value}"
            );
        }
    }
}

#[test]
fn sin_profiles_la_predeterminada_es_el_gestor() {
    let (_dir, engine) = instance();
    prepare(&engine, None).unwrap();
    let data = settings(&engine);
    assert_eq!(data["defaultProfile"], json!(guid(None, Shell::Cmd)));
    assert_eq!(data["newTabMenu"].as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(engine.layout().state().join("terminal-profile.json"))
            .unwrap_or_default(),
        ""
    );
}

#[test]
fn la_seleccion_del_gestor_no_cambia_el_perfil_del_comando() {
    let (_dir, engine) = instance();
    profile(&engine, "java11");
    profile(&engine, "java21");
    prepare(&engine, Some("java21")).unwrap();
    let argv = command(&engine, None, Shell::Cmd, Some("java21")).unwrap();
    let at = argv.iter().position(|a| a == "--profile").unwrap();
    assert_eq!(argv[at + 1], guid(None, Shell::Cmd));
    assert_eq!(&argv[argv.len() - 2..], ["--profile", "java21"]);
    assert_eq!(
        settings(&engine)["defaultProfile"],
        json!(guid(Some("java21"), Shell::Cmd))
    );
    assert!(command(&engine, None, Shell::Cmd, Some("no-existe")).is_err());
}

#[test]
fn prepare_sincroniza_los_iconos_de_images_y_usa_mito_ico_en_las_entradas() {
    let (_dir, engine) = instance();
    profile(&engine, "java11");
    let icon = engine.layout().branding_icon();
    fs::create_dir_all(icon.parent().unwrap()).unwrap();
    fs::write(&icon, b"icono de prueba").unwrap();
    prepare(&engine, None).unwrap();
    let images = engine.layout().windows_terminal().join("Images");
    for name in ["terminal_contrast-black.ico", "terminal_contrast-white.ico"] {
        assert_eq!(fs::read(images.join(name)).unwrap(), b"icono de prueba");
    }
    let data = settings(&engine);
    for item in data["profiles"]["list"].as_array().unwrap() {
        if item["hidden"] == json!(false) {
            assert_eq!(item["icon"], json!(icon.display().to_string()));
        }
    }
    // Un Terminal que no es un PE no se puede marcar: el aviso no impide los ajustes.
    assert!(!engine.layout().branded_terminal_exe().exists());
}

#[test]
fn prepare_crea_portable_y_la_copia_unica_de_los_ajustes_previos() {
    let (_dir, engine) = instance();
    let folder = engine.layout().windows_terminal();
    fs::create_dir_all(folder.join("settings")).unwrap();
    fs::write(
        folder.join("settings").join("settings.json"),
        r##"{"keybindings": [{"id": "x"}], "profiles": {"defaults": {"bellStyle": "none", "opacity": 50}},
            "schemes": [{"name": "Mio", "background": "#000000"}, {"name": "MITO CMD", "background": "#111111"}]}"##,
    )
    .unwrap();
    prepare(&engine, None).unwrap();
    assert!(folder.join(".portable").is_file());
    let backup = engine
        .layout()
        .state()
        .join("terminal-settings-before-branding.json");
    let saved: Value = serde_json::from_slice(&fs::read(&backup).unwrap()).unwrap();
    assert_eq!(saved["profiles"]["defaults"]["opacity"], 50);
    let data = settings(&engine);
    assert_eq!(data["keybindings"], json!([{"id": "x"}]));
    assert_eq!(data["profiles"]["defaults"]["bellStyle"], "none");
    assert_eq!(data["profiles"]["defaults"]["opacity"], 94);
    let names: Vec<&str> = data["schemes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scheme| scheme["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Mio", "MITO Forest", "MITO PowerShell", "MITO CMD"]);
    // La copia se crea una sola vez.
    fs::write(&backup, "{\"sentinel\": true}").unwrap();
    profile(&engine, "otro");
    prepare(&engine, None).unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "{\"sentinel\": true}");
}

#[test]
fn sin_windows_terminal_prepare_no_escribe_nada() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(dir.path());
    engine.initialize().unwrap();
    prepare(&engine, None).unwrap();
    assert!(!engine.layout().windows_terminal().exists());
    assert!(
        !engine
            .layout()
            .state()
            .join("terminal-profile.json")
            .exists()
    );
    let error = command(&engine, None, Shell::Cmd, None).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap."
    );
}

#[test]
fn los_comandos_reutilizan_la_ventana_de_la_instancia_y_fijan_el_shell() {
    let (_dir, engine) = instance();
    profile(&engine, "java21");
    let manager = command(&engine, None, Shell::Cmd, None).unwrap();
    let powershell = command(&engine, Some("java21"), Shell::PowerShell, None).unwrap();
    let layout = engine.layout();
    assert_eq!(
        manager[0],
        layout.windows_terminal_exe().display().to_string()
    );
    assert_eq!(manager[1], "--maximized");
    assert_eq!(manager[2], "-w");
    assert_eq!(manager[3], powershell[3]);
    assert_eq!(manager[3], crate::window_name(layout.root()));
    assert_eq!(manager[4], "new-tab");
    assert!(powershell.contains(&guid(Some("java21"), Shell::PowerShell)));
    assert_eq!(
        &powershell[powershell.len() - 5..],
        ["shell", "--type", "powershell", "--profile", "java21"]
    );
    assert_eq!(
        powershell[powershell.len() - 6],
        layout.mito_exe().display().to_string()
    );
}

#[test]
fn la_ventana_depende_de_la_raiz_sin_distinguir_mayusculas() {
    assert_eq!(
        crate::window_name(Path::new(r"C:\MITO")),
        crate::window_name(Path::new(r"c:\mito"))
    );
    assert_ne!(
        crate::window_name(Path::new(r"C:\MITO")),
        crate::window_name(Path::new(r"C:\OTRO"))
    );
    let name = crate::window_name(Path::new(r"C:\MITO"));
    assert_eq!(name.len(), "mito-".len() + 12);
    assert!(name.starts_with("mito-"));
}

#[test]
fn una_aplicacion_acoplada_tiene_pestana_propia_y_runner_owned() {
    let (_dir, engine) = instance();
    profile(&engine, "java21");
    let child = component_child(
        "java21",
        "extras/editor",
        None,
        &["run".to_owned(), "two words".to_owned()],
    );
    let argv = application_command(&engine, "java21", "My Editor", &child).unwrap();
    let title = argv.iter().position(|a| a == "--title").unwrap();
    assert_eq!(argv[title + 1], "My Editor");
    assert_eq!(
        &argv[argv.len() - 9..],
        [
            "run",
            "--owned",
            "--profile",
            "java21",
            "--app",
            "extras/editor",
            "--",
            "run",
            "two words"
        ]
    );
    let mito = engine.layout().mito_exe().display().to_string();
    assert_eq!(argv[argv.len() - 10], mito);
    let with_shortcut = component_child("p", "a/b", Some("Ayuda"), &[]);
    assert_eq!(
        with_shortcut,
        [
            "run",
            "--owned",
            "--profile",
            "p",
            "--app",
            "a/b",
            "--shortcut",
            "Ayuda"
        ]
    );
}

#[test]
fn golden_del_settings_json_generado() {
    let layout = Layout::new(r"C:\MITO");
    let profiles = vec!["java11".to_owned()];
    let data = build_settings(
        &layout,
        &profiles,
        Some("java11"),
        DefaultShell::Cmd,
        &json!({}),
    );
    insta::assert_snapshot!(serde_json::to_string_pretty(&data).unwrap());
}

#[test]
fn el_motor_regenera_los_ajustes_al_crear_y_borrar_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let extensions = mito_engine::Extensions {
        terminal: Box::new(crate::PrivateTerminal),
        ..mito_engine::Extensions::default()
    };
    let engine = Engine::with_extensions(dir.path(), extensions);
    engine.initialize().unwrap();
    let folder = engine.layout().windows_terminal();
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("WindowsTerminal.exe"), b"terminal").unwrap();
    engine.create_profile("java11", None).unwrap();
    assert_eq!(
        visible_names(&settings(&engine)),
        ["CMD (java11)", "PowerShell (java11)", "MITO"]
    );
    engine.create_profile("java21", None).unwrap();
    assert_eq!(settings(&engine)["newTabMenu"].as_array().unwrap().len(), 5);
    engine.delete_profile("java21").unwrap();
    assert_eq!(settings(&engine)["newTabMenu"].as_array().unwrap().len(), 3);
}
