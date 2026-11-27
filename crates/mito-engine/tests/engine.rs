//! Tests de `engine.py` portados: profiles, inventario, entorno, plan e instalación.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use common::{ZERO_HASH, base_env, error_text, fx, fx_with, manifest, zip_of};
use mito_core::{Layout, MutationLock};
use mito_engine::{InstallOptions, Records};
use serde_json::{Value, json};

fn tool_manifest() -> Value {
    json!({
        "version": "1.0",
        "url": "https://example.org/tool.zip",
        "hash": ZERO_HASH,
        "bin": [["bin/tool.exe", "tool"]],
        "env_set": {"JAVA_HOME": "$dir"},
        "env_add_path": "bin"
    })
}

fn with_tool() -> common::Fx {
    let f = fx();
    f.bucket("test", json!({"tool": tool_manifest()}));
    f.serve(
        "https://example.org/tool.zip",
        zip_of(&[("bin/tool.exe", b"exe")]),
    );
    f.profile("test");
    f
}

// --- profiles -------------------------------------------------------------------

#[test]
fn el_profile_default_solo_se_crea_si_no_hay_ninguno() {
    let f = fx();
    assert_eq!(
        f.engine.ensure_default_profile().unwrap().as_deref(),
        Some("default")
    );
    let text = fs::read_to_string(f.path("profiles/default.json")).unwrap();
    assert_eq!(
        text,
        "{\n  \"workspace\": \"default\",\n  \"apps\": [],\n  \"env\": {}\n}\n"
    );
    assert!(f.path("workspaces/default").is_dir());
    assert_eq!(f.engine.ensure_default_profile().unwrap(), None);
    assert_eq!(f.engine.profiles(), ["default"]);
}

#[test]
fn golden_del_profile_con_una_app_y_una_variable() {
    let f = with_tool();
    f.install(&["test/tool"], Some("test")).unwrap();
    let mut profile = f.engine.profile("test").unwrap();
    profile.env.push(("MY_OPTION".into(), "two words".into()));
    f.engine.save_profile("test", &profile).unwrap();
    let text = fs::read_to_string(f.path("profiles/test.json")).unwrap();
    assert_eq!(
        text,
        "{\n  \"workspace\": \"test\",\n  \"apps\": [\n    \"test/tool\"\n  ],\n  \"env\": {\n    \"MY_OPTION\": \"two words\"\n  }\n}\n"
    );
}

#[test]
fn varios_profiles_pueden_compartir_workspace_y_nunca_se_borra_el_ultimo() {
    let f = fx();
    assert_eq!(
        f.engine.create_profile("One", Some("shared")).unwrap(),
        "one"
    );
    f.engine.create_profile("two", Some("shared")).unwrap();
    assert_eq!(
        error_text(f.engine.create_profile("two", None)),
        "El profile two ya existe"
    );
    assert_eq!(f.engine.profile("one").unwrap().workspace, "shared");
    fs::write(f.path("workspaces/shared/keep.txt"), "x").unwrap();
    assert_eq!(f.engine.delete_profile("one").unwrap(), "two");
    // Se conservan el workspace y la copia de seguridad.
    assert!(f.path("workspaces/shared/keep.txt").is_file());
    assert!(f.path("core/state/deleted-profiles/one.json").is_file());
    assert!(f.path("home").is_dir());
    assert_eq!(
        error_text(f.engine.delete_profile("two")),
        "No se puede eliminar el último profile"
    );
    assert_eq!(
        error_text(f.engine.profile("ghost")),
        "No existe el profile ghost"
    );
}

#[test]
fn el_lock_ocupado_falla_y_con_espera_se_reintenta() {
    let f = fx();
    f.profile("one");
    let held = MutationLock::mutation(f.engine.layout(), Duration::ZERO).unwrap();
    assert_eq!(
        error_text(f.engine.set_active("one", "x/y", false)),
        "Otra instancia está modificando MITO. Espera a que termine."
    );
    let waiting = mito_engine::Engine::new(f.root()).with_lock_wait(Duration::from_secs(5));
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    waiting.set_active("one", "x/y", false).unwrap();
    release.join().unwrap();
}

// --- validación de variables ----------------------------------------------------

#[test]
fn variables_de_profile_invalidas() {
    let f = fx();
    f.profile("one");
    let write = |env: Value| {
        let value = json!({"workspace": "one", "apps": [], "env": env});
        fs::write(f.path("profiles/one.json"), value.to_string()).unwrap();
        f.engine.profile("one").map(|_| ()).unwrap_err().to_string()
    };
    assert_eq!(
        write(json!({"HOME": "x"})),
        "Variable reservada por MITO: HOME"
    );
    assert_eq!(
        write(json!({"Path": "x"})),
        "Variable reservada por MITO: Path"
    );
    assert_eq!(
        write(json!({"MITO_ROOT": "x"})),
        "Variable reservada por MITO: MITO_ROOT"
    );
    assert_eq!(write(json!({"A": 1})), "Variable de entorno no válida: A");
    assert_eq!(
        write(json!({"A": "1", "a": "2"})),
        "Variable de entorno repetida: a"
    );
    assert_eq!(
        write(json!({"BAD=NAME": "x"})),
        "Variable de entorno no válida: BAD=NAME"
    );
}

#[test]
fn variables_globales_validas_e_invalidas() {
    let f = fx();
    let write = |text: &str| fs::write(f.path("config.properties"), text).unwrap();
    write("# env.IGNORED=no\nenv.JAVA_HOME=global-java\nenv.COMMON=a=b\nenv.EMPTY=\n");
    assert_eq!(
        f.engine.global_env().unwrap(),
        [
            ("JAVA_HOME".to_owned(), "global-java".to_owned()),
            ("COMMON".to_owned(), "a=b".to_owned()),
            ("EMPTY".to_owned(), String::new())
        ]
    );
    for (text, expected) in [
        ("env.PATH=x\n", "Variable reservada por MITO: PATH"),
        (
            "env.MITO_ROOT=x\n",
            "Variable reservada por MITO: MITO_ROOT",
        ),
        ("env.A=1\nenv.A=2\n", "Variable de entorno repetida: A"),
        ("env.MISSING\n", "Variable global sin =: MISSING"),
        ("env.=x\n", "Variable de entorno no válida: "),
    ] {
        write(text);
        assert_eq!(error_text(f.engine.global_env()), expected, "{text}");
    }
}

#[test]
fn las_variables_globales_se_recargan_y_el_profile_gana() {
    let f = fx();
    f.profile("one");
    f.profile("two");
    fs::write(
        f.path("config.properties"),
        "env.JAVA_HOME=global\nenv.COMMON=1\n",
    )
    .unwrap();
    let mut one = f.engine.profile("one").unwrap();
    one.env.push(("java_home".into(), "profile".into()));
    f.engine.save_profile("one", &one).unwrap();
    let env = f.env("one");
    assert_eq!(env["JAVA_HOME"], "profile");
    assert_eq!(env["COMMON"], "1");
    assert!(!env.contains_key("java_home"));
    assert_eq!(f.env("two")["JAVA_HOME"], "global");
    fs::write(f.path("config.properties"), "env.COMMON=2\n").unwrap();
    let env = f.env("two");
    assert_eq!(env["COMMON"], "2");
    assert!(!env.contains_key("JAVA_HOME"));
    assert_eq!(env["MITO_ACTIVE_VARS"], "COMMON");
}

// --- contexto y entorno ---------------------------------------------------------

#[test]
fn el_home_es_unico_y_tiene_las_carpetas_conocidas_de_windows() {
    let f = fx();
    f.profile("one");
    f.profile("two");
    fs::remove_dir_all(f.path("home")).unwrap();
    let env = f.env("one");
    for folder in [
        "Desktop",
        "Documents",
        "Downloads",
        "Music",
        "Pictures",
        "Videos",
        "Favorites",
        "Links",
        "Contacts",
        "Searches",
        "Saved Games",
        "AppData/LocalLow",
        "AppData/Roaming",
        "AppData/Local",
        "tmp",
        "custom-commands",
    ] {
        assert!(f.path(&format!("home/{folder}")).is_dir(), "{folder}");
    }
    let home = f.engine.layout().home().display().to_string();
    assert_eq!(env["HOME"], home);
    assert_eq!(env["USERPROFILE"], home);
    assert_eq!(f.env("two")["HOME"], home);
    assert_eq!(env["TEMP"], format!(r"{home}\tmp"));
    assert_eq!(env["TMP"], env["TEMP"]);
    assert_eq!(env["APPDATA"], format!(r"{home}\AppData\Roaming"));
    assert_eq!(env["LOCALAPPDATA"], format!(r"{home}\AppData\Local"));
    assert_eq!(env["HOMEDRIVE"], &home[..2]);
    assert_eq!(env["HOMEPATH"], &home[2..]);
    assert_eq!(env["MITO_PROFILE"], "one");
    assert_eq!(env["MITO_ROOT"], f.root().display().to_string());
}

#[test]
fn el_workspace_sigue_al_profile_y_context_variables_no_crea_nada() {
    let f = fx();
    f.engine.create_profile("one", Some("ws1")).unwrap();
    f.engine.create_profile("two", None).unwrap();
    fs::remove_dir_all(f.path("workspaces/ws1")).unwrap();
    let context = f.engine.context_variables("one").unwrap();
    assert_eq!(
        context["MITO_WORKSPACE"],
        f.path("workspaces/ws1").display().to_string()
    );
    assert!(!f.path("workspaces/ws1").exists());
    assert_eq!(
        f.engine.context_variables("two").unwrap()["MITO_WORKSPACE"],
        f.path("workspaces/two").display().to_string()
    );
    let mut one = f.engine.profile("one").unwrap();
    one.env.push(("mito_workspace".into(), "x".into()));
    f.engine.save_profile("one", &one).unwrap();
    assert!(f.engine.profile("one").is_err());
}

#[test]
fn la_instalacion_verificada_y_el_entorno_con_el_orden_real_del_path() {
    let f = with_tool();
    f.install(&["test/tool"], Some("test")).unwrap();
    let installed = f.engine.installed().unwrap();
    let lock = installed.get("test/tool").unwrap();
    assert_eq!(lock.path, "core/apps/test/tool");
    assert_eq!(lock.version_text(), "1.0");
    let payload = f.path("core/apps/test/tool").display().to_string();
    let env = f.env("test");
    assert_eq!(env["JAVA_HOME"], payload);
    let shims = f.path("core/state/shims/test").display().to_string();
    let expected = [
        shims,
        f.path("home/custom-commands").display().to_string(),
        f.path("core/commands").display().to_string(),
        format!(r"{payload}\bin"),
        r"C:\host".to_owned(),
    ];
    assert_eq!(env["PATH"], expected.join(";"));
    assert_eq!(env["MITO_HOST_PATH"], r"C:\host");
    assert_eq!(env["MITO_ACTIVE_VARS"], "JAVA_HOME");
    // `environment_from` no escribe en el entorno del proceso.
    assert!(std::env::var("MITO_ACTIVE_VARS").is_err());
}

#[test]
fn el_env_del_profile_gana_al_del_componente_sin_filtrarse_al_siguiente() {
    let f = with_tool();
    f.profile("other");
    f.install(&["test/tool"], Some("test")).unwrap();
    f.engine.set_active("other", "test/tool", true).unwrap();
    let mut data = f.engine.profile("test").unwrap();
    data.env.push(("java_home".into(), "override".into()));
    f.engine.save_profile("test", &data).unwrap();
    assert_eq!(f.env("test")["JAVA_HOME"], "override");
    assert_eq!(
        f.env("other")["JAVA_HOME"],
        f.path("core/apps/test/tool").display().to_string()
    );
}

#[test]
fn los_perfiles_anidados_no_apilan_path_ni_variables() {
    let f = with_tool();
    f.install(&["test/tool"], Some("test")).unwrap();
    let parent = f.env("test");
    let mut base = parent.clone();
    base.insert("PATH".into(), "C:\\polluted;C:\\host".into());
    base.insert("EXTRA".into(), "stays".into());
    let nested = f.engine.environment_from("test", base).unwrap();
    // MITO_HOST_PATH manda sobre el PATH del padre y se quitan las variables activas.
    assert_eq!(nested["PATH"], parent["PATH"]);
    assert_eq!(nested["EXTRA"], "stays");
    f.profile("empty");
    let clean = f.engine.environment_from("empty", parent).unwrap();
    assert!(!clean.contains_key("JAVA_HOME"));
    assert!(!clean["PATH"].contains("tool"));
    assert_eq!(clean["MITO_ACTIVE_VARS"], "");
}

#[test]
fn varios_runtimes_pueden_estar_activos_a_la_vez() {
    let f = fx();
    f.bucket(
        "test",
        json!({"a": {"version": "1", "url": "https://example.org/a.zip", "hash": ZERO_HASH,
                     "bin": "a.exe", "env_set": {"A_HOME": "$dir"}},
               "b": {"version": "1", "url": "https://example.org/b.zip", "hash": ZERO_HASH,
                     "bin": "b.exe", "env_set": {"B_HOME": "$dir"}}}),
    );
    f.serve("https://example.org/a.zip", zip_of(&[("a.exe", b"a")]));
    f.serve("https://example.org/b.zip", zip_of(&[("b.exe", b"b")]));
    f.profile("p");
    f.install(&["test/a", "test/b"], Some("p")).unwrap();
    assert_eq!(f.apps("p"), ["test/a", "test/b"]);
    let env = f.env("p");
    assert!(env.contains_key("A_HOME") && env.contains_key("B_HOME"));
    assert_eq!(env["MITO_ACTIVE_VARS"], "A_HOME;B_HOME");
}

#[test]
fn una_seleccion_inexistente_se_informa_y_se_puede_desactivar() {
    let f = fx();
    f.profile("p");
    let mut data = f.engine.profile("p").unwrap();
    data.apps.push("missing/app@123".into());
    f.engine.save_profile("p", &data).unwrap();
    assert_eq!(
        error_text(f.engine.environment_from("p", base_env())),
        "Falta la instalación: missing/app@123"
    );
    f.engine.set_active("p", "missing/app@123", false).unwrap();
    assert!(f.apps("p").is_empty());
    assert_eq!(
        error_text(f.engine.set_active("p", "missing/app", true)),
        "El componente no está instalado"
    );
}

#[test]
fn un_lock_alterado_se_rechaza_al_activar() {
    let f = with_tool();
    f.install(&["test/tool"], Some("test")).unwrap();
    let marker = f.path("core/apps/test/tool/.mito.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&marker).unwrap()).unwrap();
    value["record"]["manifest"]["env_set"]["JAVA_HOME"] = json!("C:\\evil");
    fs::write(&marker, value.to_string()).unwrap();
    assert_eq!(
        error_text(f.engine.environment_from("test", base_env())),
        "Lock o instalación incoherente: test/tool"
    );
}

#[test]
fn activar_y_desactivar_actualiza_los_shims() {
    let f = with_tool();
    let id = f.install(&["test/tool"], None).unwrap();
    assert_eq!(f.apps("test"), Vec::<String>::new());
    assert_eq!(id.len(), 1);
    f.engine.set_active("test", "test/tool", true).unwrap();
    f.engine.set_active("test", "test/tool", true).unwrap();
    assert_eq!(f.apps("test"), ["test/tool"]);
    f.env("test");
    let shim = f.path("core/state/shims/test/tool.cmd");
    assert_eq!(
        fs::read_to_string(&shim).unwrap(),
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\n\"%~dp0..\\..\\..\\bin\\mito.exe\" run --profile test --app test/tool --bin=\"tool\" -- %*\r\n"
    );
    assert_eq!(
        fs::read_to_string(f.path("core/state/shims/test/generated.json")).unwrap(),
        "[\n  \"tool.cmd\"\n]\n"
    );
    f.engine.set_active("test", "test/tool", false).unwrap();
    f.env("test");
    assert!(!shim.exists());
    assert_eq!(f.engine.installed().unwrap().len(), 1);
}

#[test]
fn los_alias_de_bin_se_validan_y_se_escapan_en_el_shim() {
    let f = fx();
    f.bucket(
        "test",
        json!({"odd": {"version": "1", "url": "https://example.org/odd.zip", "hash": ZERO_HASH,
                       "bin": [["tool.exe", "My Tool&%x"]]},
               "bad": {"version": "1", "url": "https://example.org/bad.zip", "hash": ZERO_HASH,
                       "bin": [["tool.exe", "a/b"]]}}),
    );
    f.profile("p");
    f.install(&["test/odd"], Some("p")).unwrap();
    f.env("p");
    let shim = fs::read_to_string(f.path("core/state/shims/p/my tool&%x.cmd")).unwrap();
    assert!(shim.contains("--bin=\"my tool&%%x\""));
    assert!(f.install(&["test/bad"], Some("p")).is_err());
    assert_eq!(f.apps("p"), ["test/odd"]);
}

#[test]
fn mover_la_raiz_reconstruye_el_entorno() {
    let f = with_tool();
    f.install(&["test/tool"], Some("test")).unwrap();
    let parent = f.root().parent().unwrap().to_path_buf();
    let moved = parent.join(format!(
        "{}-moved",
        f.root().file_name().unwrap().to_string_lossy()
    ));
    fs::rename(f.root(), &moved).unwrap();
    let engine = mito_engine::Engine::new(&moved);
    let env = engine.environment_from("test", base_env()).unwrap();
    assert!(env["JAVA_HOME"].starts_with(&engine.root().display().to_string()));
    assert!(env["JAVA_HOME"].contains("-moved"));
    fs::rename(&moved, f.root()).unwrap();
}

// --- manifiestos: componentes, cabeceras, pasos no soportados ---------------------

#[test]
fn arquitectura_x64_sustituye_los_campos_comunes() {
    let f = fx();
    f.bucket(
        "test",
        json!({"arch": {"version": "1", "url": "https://example.org/wrong.zip", "hash": ZERO_HASH,
                        "bin": "tool.exe", "env_set": {"KEEP": "yes"},
                        "architecture": {"64bit": {"url": "https://example.org/right.zip"}}}}),
    );
    f.install(&["test/arch"], None).unwrap();
    assert_eq!(
        f.fake.hits.lock().unwrap().as_slice(),
        ["https://example.org/right.zip"]
    );
    let lock = f
        .engine
        .installed()
        .unwrap()
        .get("test/arch")
        .cloned()
        .unwrap();
    let (vars, _) = f.engine.component_environment(&lock, Some("x")).unwrap();
    assert_eq!(vars, [("KEEP".to_owned(), "yes".to_owned())]);
}

#[test]
fn pre_install_e_installer_fallan_antes_de_descargar() {
    let f = fx();
    f.bucket(
        "test",
        json!({
            "pre": {"version": "1", "url": "https://example.org/a.zip", "hash": ZERO_HASH,
                    "bin": "tool.exe", "pre_install": "Write-Host hi"},
            "script": {"version": "1", "url": "https://example.org/b.zip", "hash": ZERO_HASH,
                       "bin": "tool.exe", "installer": {"script": "Write-Host hi"}},
            "file": {"version": "1", "url": "https://example.org/c.zip", "hash": ZERO_HASH,
                     "bin": "tool.exe", "installer": {"file": "setup.ps1"}},
            "dep": {"version": "1", "url": "https://example.org/d.zip", "hash": ZERO_HASH,
                    "bin": "tool.exe", "depends": "test/pre"},
        }),
    );
    for (app, field) in [
        ("pre", "pre_install"),
        ("script", "installer.script"),
        ("file", "installer.file"),
        ("dep", "pre_install"),
    ] {
        assert_eq!(
            error_text(f.install(&[&format!("test/{app}")], None)),
            format!("Pasos no soportados: {field}"),
            "{app}"
        );
    }
    assert!(f.fake.hits.lock().unwrap().is_empty());
    assert!(f.engine.installed().unwrap().is_empty());
}

#[test]
fn post_install_se_omite_con_un_aviso() {
    let f = fx();
    let mut m = manifest("1");
    m["post_install"] = json!("Write-Host later");
    f.catalog(json!({"tool": m}));
    let lines = std::cell::RefCell::new(Vec::new());
    let options = InstallOptions::default();
    f.engine
        .install(
            &["main/tool"],
            &options,
            &|line| lines.borrow_mut().push(line.message.to_string()),
            &mito_core::Operation::detached(),
        )
        .unwrap();
    assert!(
        lines.borrow().contains(
            &"main/tool: post_install omitido por la política general de MITO".to_owned()
        )
    );
}

#[test]
fn el_movimiento_de_archivos_abre_y_cierra_un_paso_largo() {
    let f = with_tool();
    let lines = std::cell::RefCell::new(Vec::new());
    let options = InstallOptions::default();
    f.engine
        .install(
            &["test/tool"],
            &options,
            &|line| lines.borrow_mut().push(line),
            &mito_core::Operation::detached(),
        )
        .unwrap();
    let lines = lines.into_inner();
    let steps: Vec<(usize, bool)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.long_step.map(|active| (index, active)))
        .collect();
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert!(steps[0].1 && !steps[1].1);
    let open = &lines[steps[0].0].message;
    assert!(
        open.template()
            .starts_with("Moviendo {p1} archivos de {p2}")
    );
    assert_eq!(
        open.args()
            .iter()
            .map(|(name, value)| (name.as_ref(), value.as_str()))
            .collect::<Vec<_>>(),
        [("p1", "1"), ("p2", "test/tool")]
    );
    let close = &lines[steps[1].0].message;
    assert_eq!(close.template(), "Archivos {p1}: {p2}/{p3}");
    assert_eq!(close.to_string(), "Archivos test/tool: 1/1");
    assert!(steps[0].0 < steps[1].0);
}

#[test]
fn nombre_ambiguo_entre_buckets_exige_espacio_de_nombres() {
    let f = fx();
    f.bucket("one", json!({"tool": manifest("1")}));
    f.bucket("two", json!({"tool": manifest("1")}));
    let text = error_text(f.install(&["tool"], None));
    assert!(
        text.starts_with("Componente no encontrado o ambiguo: tool"),
        "{text}"
    );
    f.install(&["two/tool"], None).unwrap();
}

#[test]
fn una_dependencia_incompatible_o_circular_falla_antes_de_descargar() {
    let f = fx();
    f.bucket(
        "main",
        json!({
            "a": {"version": "1", "url": "https://example.org/a.zip", "hash": ZERO_HASH,
                  "bin": "tool.exe", "depends": "b"},
            "b": {"version": "1", "url": "https://example.org/b.zip", "hash": ZERO_HASH,
                  "bin": "tool.exe", "depends": "a"},
            "c": {"version": "1", "url": "https://example.org/c.zip", "hash": ZERO_HASH,
                  "bin": "tool.exe", "depends": "d"},
            "d": {"version": "1", "url": "https://example.org/d.iso", "hash": ZERO_HASH,
                  "bin": "tool.exe"},
        }),
    );
    assert_eq!(
        error_text(f.install(&["main/a"], None)),
        "Dependencia circular: main/a"
    );
    let text = error_text(f.install(&["main/c"], None));
    assert!(text.starts_with("main/d: "), "{text}");
    assert!(f.fake.hits.lock().unwrap().is_empty());
}

// --- instalación ----------------------------------------------------------------

#[test]
fn varias_descargas_sin_hash_se_extraen_todas_con_confirmacion() {
    let f = fx();
    f.catalog(json!({"multi": {
        "version": "1", "bin": "a.exe",
        "url": ["https://example.org/a.zip", "https://example.org/b.zip"],
        "extract_to": ["one", "two"]}}));
    f.serve("https://example.org/a.zip", zip_of(&[("a.exe", b"a")]));
    f.serve("https://example.org/b.zip", zip_of(&[("b.exe", b"b")]));
    let seen = std::cell::RefCell::new(Vec::new());
    let confirm = |items: &[String]| {
        seen.borrow_mut().extend(items.iter().cloned());
        true
    };
    let options = InstallOptions {
        confirm_unverified: Some(&confirm),
        ..InstallOptions::default()
    };
    // `bin` apunta a `a.exe` en la raíz: con `extract_to` no existe, así que falla.
    let error = f
        .engine
        .install(
            &["main/multi"],
            &options,
            &|_| {},
            &mito_core::Operation::detached(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "Comando ausente: a.exe");
    assert_eq!(
        seen.borrow().as_slice(),
        ["main/multi 1 · descarga 1", "main/multi 1 · descarga 2"]
    );
    assert!(f.staging_is_empty());
    f.catalog(json!({"multi": {
        "version": "1", "bin": "one/a.exe",
        "url": ["https://example.org/a.zip", "https://example.org/b.zip"],
        "extract_to": ["one", "two"]}}));
    f.engine
        .install(
            &["main/multi"],
            &options,
            &|_| {},
            &mito_core::Operation::detached(),
        )
        .unwrap();
    assert!(f.path("core/apps/main/multi/one/a.exe").is_file());
    assert!(f.path("core/apps/main/multi/two/b.exe").is_file());
}

#[test]
fn una_descarga_sin_hash_sin_confirmar_cancela_antes_de_descargar() {
    let f = fx();
    f.profile("p");
    f.bucket(
        "main",
        json!({"tool": {"version": "1", "url": "https://example.org/tool.zip", "bin": "tool.exe",
                        "depends": "dep"},
               "dep": {"version": "1", "url": "https://example.org/dep.zip", "bin": "tool.exe"}}),
    );
    let no = |_: &[String]| false;
    for confirm in [None, Some(&no as mito_engine::Confirm<'_>)] {
        let options = InstallOptions {
            profile: Some("p"),
            confirm_unverified: confirm,
            ..InstallOptions::default()
        };
        let error = f
            .engine
            .install(
                &["main/tool"],
                &options,
                &|_| {},
                &mito_core::Operation::detached(),
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Instalación cancelada: descargas sin hash no confirmadas"
        );
    }
    assert!(f.fake.hits.lock().unwrap().is_empty());
    assert!(f.apps("p").is_empty());
    assert!(f.engine.installed().unwrap().is_empty());
}

#[test]
fn confirmar_el_plan_se_puede_rechazar() {
    let f = fx();
    f.catalog(json!({"tool": manifest("1")}));
    let no = |_: &Records| false;
    let options = InstallOptions {
        confirm_plan: Some(&no),
        ..InstallOptions::default()
    };
    let error = f
        .engine
        .install(
            &["main/tool"],
            &options,
            &|_| {},
            &mito_core::Operation::detached(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "Importación cancelada");
}

#[test]
fn un_fallo_de_descarga_conserva_el_profile_y_deja_el_staging_vacio() {
    let f = fx();
    f.profile("p");
    f.catalog(json!({"tool": manifest("1")}));
    f.fake.fail.lock().unwrap().push("tool.zip".into());
    assert!(f.install(&["main/tool"], Some("p")).is_err());
    assert!(f.apps("p").is_empty());
    assert!(f.staging_is_empty());
    assert!(f.engine.installed().unwrap().is_empty());
}

#[test]
fn instalar_mueve_los_archivos_extraidos_en_lugar_de_copiarlos() {
    let f = fx();
    f.catalog(json!({"tool": manifest("1")}));
    f.serve(
        "https://example.org/tool.zip",
        zip_of(&[("tool.exe", b"big")]),
    );
    f.install(&["main/tool"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/tool/tool.exe")).unwrap(),
        b"big"
    );
    let log = fs::read_to_string(f.path("logs/mito.log")).unwrap();
    assert!(
        log.lines()
            .any(|l| l.contains("install.move-whole") && l.contains("\"moved\"")),
        "{log}"
    );
}

#[test]
fn un_zip_con_traversal_se_rechaza_sin_dejar_nada() {
    let f = fx();
    f.catalog(
        json!({"bad": {"version": "1", "url": "https://example.org/bad", "hash": ZERO_HASH,
                             "bin": "tool.exe"}}),
    );
    f.serve(
        "https://example.org/bad",
        zip_of(&[("tool.exe", b"x"), ("../escape.txt", b"x")]),
    );
    assert!(f.install(&["main/bad"], None).is_err());
    assert!(!f.path("core/apps/main/bad").exists());
    assert!(!f.root().join("core").join("escape.txt").exists());
    assert!(f.staging_is_empty());
    // Un ZIP sin extensión legítimo sí se extrae.
    f.serve("https://example.org/bad", zip_of(&[("tool.exe", b"ok")]));
    f.install(&["main/bad"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/bad/tool.exe")).unwrap(),
        b"ok"
    );
}

#[test]
fn dos_artefactos_no_pueden_escribir_el_mismo_archivo() {
    let f = fx();
    f.catalog(json!({"dup": {"version": "1", "bin": "tool.exe",
        "url": ["https://example.org/a.zip", "https://example.org/b.zip"]}}));
    f.serve("https://example.org/a.zip", zip_of(&[("tool.exe", b"a")]));
    f.serve("https://example.org/b.zip", zip_of(&[("tool.exe", b"b")]));
    assert_eq!(
        error_text(f.install(&["main/dup"], None)),
        "Dos artefactos intentan escribir el mismo archivo"
    );
}

#[test]
fn extract_dir_y_comando_ausente() {
    let f = fx();
    f.catalog(json!({
        "nested": {"version": "1", "url": "https://example.org/n.zip", "hash": ZERO_HASH,
                   "bin": "tool.exe", "extract_dir": "inner"},
        "nodir": {"version": "1", "url": "https://example.org/n.zip", "hash": ZERO_HASH,
                  "bin": "tool.exe", "extract_dir": "missing"},
        "nobin": {"version": "1", "url": "https://example.org/n.zip", "hash": ZERO_HASH,
                  "bin": "absent.exe", "extract_dir": "inner"},
    }));
    f.serve(
        "https://example.org/n.zip",
        zip_of(&[("inner/tool.exe", b"x")]),
    );
    f.install(&["main/nested"], None).unwrap();
    assert!(f.path("core/apps/main/nested/tool.exe").is_file());
    assert_eq!(
        error_text(f.install(&["main/nodir"], None)),
        "extract_dir no encontrado: missing"
    );
    assert_eq!(
        error_text(f.install(&["main/nobin"], None)),
        "Comando ausente: absent.exe"
    );
    assert!(!f.path("core/apps/main/nobin").exists());
}

#[test]
fn el_marcador_mito_json_tiene_el_formato_exacto() {
    let f = with_tool();
    f.install(&["test/tool"], None).unwrap();
    let text = fs::read_to_string(f.path("core/apps/test/tool/.mito.json")).unwrap();
    let digest = mito_catalog::fingerprint(&tool_manifest());
    let expected = json!({
        "key": "test/tool",
        "manifest_sha256": digest,
        "record": {
            "key": "test/tool",
            "version": "1.0",
            "manifest_sha256": digest,
            "path": "core/apps/test/tool",
            "manifest": tool_manifest(),
        }
    });
    assert_eq!(
        text,
        format!("{}\n", serde_json::to_string_pretty(&expected).unwrap())
    );
    assert!(text.starts_with("{\n  \"key\": \"test/tool\",\n  \"manifest_sha256\": "));
}

#[test]
fn el_hash_incorrecto_descarta_la_descarga_con_el_descargador_http() {
    // Con el descargador real contra un servidor local.
    let server = common::http::serve(b"contenido".to_vec());
    let f = fx_with(|e| {
        e.downloader = Box::new(mito_engine::HttpDownloader::new(
            mito_transfer::DownloadOptions {
                attempts: 1,
                delay: Duration::ZERO,
                ..mito_transfer::DownloadOptions::default()
            },
        ));
    });
    f.catalog(
        json!({"tool": {"version": "1", "url": server.url("tool.exe"),
                              "hash": ZERO_HASH, "bin": "tool.exe"}}),
    );
    let text = error_text(f.install(&["main/tool"], None));
    assert!(text.to_lowercase().contains("hash"), "{text}");
    assert!(!f.path("core/apps/main/tool").exists());
    assert!(f.staging_is_empty());
    // Con el hash correcto se instala el archivo suelto.
    let digest = {
        use sha2_free::sha256_hex;
        sha256_hex(b"contenido")
    };
    f.catalog(
        json!({"tool": {"version": "1", "url": server.url("tool.exe"),
                              "hash": digest, "bin": "tool.exe"}}),
    );
    f.install(&["main/tool"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/tool/tool.exe")).unwrap(),
        b"contenido"
    );
}

mod sha2_free {
    /// SHA-256 vía el catálogo (misma implementación que la huella de manifiestos).
    pub fn sha256_hex(bytes: &[u8]) -> String {
        mito_catalog::pyjson::sha256_hex(bytes)
    }
}

#[test]
fn el_adaptador_fetcher_mapea_el_404() {
    let server = common::http::serve_status(404);
    let downloader = mito_engine::HttpDownloader::new(mito_transfer::DownloadOptions {
        attempts: 1,
        delay: Duration::ZERO,
        ..mito_transfer::DownloadOptions::default()
    });
    let dir = tempfile::tempdir().unwrap();
    let result = mito_catalog::Fetcher::download(
        &downloader,
        &server.url("x.json"),
        &dir.path().join("x.json"),
        None,
        &mito_core::Operation::detached(),
    );
    assert!(
        matches!(result, Err(mito_catalog::FetchError::HttpStatus(404))),
        "{result:?}"
    );
}

#[test]
fn los_origenes_de_bucket_salen_de_config_properties() {
    let f = fx();
    fs::write(f.path("config.properties"), "bucket.extra=C:\\somewhere\n").unwrap();
    let catalog = f.engine.catalog().unwrap();
    assert_eq!(catalog.sources().get("extra"), Some("C:\\somewhere"));
    let _ = (BTreeMap::<String, String>::new(), Layout::new(f.root()));
}
