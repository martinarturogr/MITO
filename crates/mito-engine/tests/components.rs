//! Tests de `components.py`, `isolation.py`, `test_activation.py` y `test_stable_storage.py`
//! portados. Los de migración de almacenamiento (F11) y los de hooks (F7) no aplican.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::os::windows::fs::OpenOptionsExt as _;

use common::{ZERO_HASH, error_text, fx, fx_with, manifest, zip_of};
use mito_core::Operation;
use mito_engine::{Engine, InstallOptions, Record};
use serde_json::{Value, json};

fn catalog_of(version: &str, extra: &Value) -> Value {
    let mut m = manifest(version);
    for (key, value) in extra.as_object().unwrap() {
        m[key] = value.clone();
    }
    m["url"] = json!(format!("https://example.org/tool-{version}.zip"));
    json!({"tool": m})
}

fn serve_version(f: &common::Fx, version: &str, extra_files: &[(&str, &[u8])]) {
    let tool = format!("tool-{version}");
    let mut files: Vec<(&str, &[u8])> = vec![("tool.exe", tool.as_bytes())];
    files.extend_from_slice(extra_files);
    f.serve(
        &format!("https://example.org/tool-{version}.zip"),
        zip_of(&files),
    );
}

fn quiet(engine: &Engine) -> Vec<String> {
    let _ = engine;
    Vec::new()
}

/// Instala `main/tool` en los profiles indicados con la versión `version`.
fn install_tool(f: &common::Fx, version: &str, extra: &Value, profiles: &[&str]) {
    f.catalog(catalog_of(version, extra));
    serve_version(f, version, &[("data/default.txt", b"default")]);
    for (index, profile) in profiles.iter().enumerate() {
        if f.engine.profile(profile).is_err() {
            f.profile(profile);
        }
        if index == 0 {
            f.install(&["main/tool"], Some(profile)).unwrap();
        } else {
            f.engine.set_active(profile, "main/tool", true).unwrap();
        }
    }
    if profiles.is_empty() {
        f.install(&["main/tool"], None).unwrap();
    }
}

fn read(f: &common::Fx, relative: &str) -> String {
    fs::read_to_string(f.path(relative)).unwrap()
}

fn log_lines() -> RefCell<Vec<String>> {
    RefCell::new(Vec::new())
}

// --- components.py ----------------------------------------------------------------

#[test]
fn remove_conserva_persist_y_quita_el_id_de_todos_los_profiles() {
    let f = fx();
    install_tool(&f, "1", &json!({"persist": "data"}), &["one", "two"]);
    f.env("one");
    fs::write(f.path("home/persist/main/tool/data/keep.txt"), "keep").unwrap();
    let log = log_lines();
    f.engine
        .remove("main/tool", &|line| log.borrow_mut().push(line.to_owned()))
        .unwrap();
    assert!(!f.path("core/apps/main/tool").exists());
    assert!(f.apps("one").is_empty() && f.apps("two").is_empty());
    assert!(f.engine.installed().unwrap().is_empty());
    assert_eq!(read(&f, "home/persist/main/tool/data/keep.txt"), "keep");
    assert!(f.staging_is_empty());
    assert_eq!(
        log.borrow().last().unwrap(),
        "Eliminado main/tool 1. Datos persistentes conservados."
    );
    assert_eq!(
        error_text(f.engine.remove("main/tool", &|_| {})),
        "El componente ya no está instalado"
    );
}

#[test]
fn un_fallo_al_escribir_un_profile_durante_remove_restaura_el_payload() {
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = armed.clone();
    let f = fx_with(move |e| {
        e.fault = Some(Box::new(move |point, subject| {
            if point == "save-profile"
                && subject == "two"
                && flag.load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(std::io::Error::other("write failed"))
            } else {
                Ok(())
            }
        }));
    });
    install_tool(&f, "1", &json!({}), &["one", "two"]);
    armed.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(f.engine.remove("main/tool", &|_| {}).is_err());
    assert!(f.path("core/apps/main/tool/tool.exe").is_file());
    assert_eq!(f.apps("one"), ["main/tool"]);
    assert_eq!(f.apps("two"), ["main/tool"]);
    assert!(f.staging_is_empty());
}

#[test]
fn update_cambia_de_version_solo_si_la_instalacion_tiene_exito() {
    let f = fx();
    install_tool(&f, "1.9", &json!({}), &["one", "two"]);
    f.catalog(catalog_of("1.10", &json!({})));
    serve_version(&f, "1.10", &[]);
    f.fake.fail.lock().unwrap().push("tool-1.10".into());
    assert!(
        f.engine
            .update("main/tool", None, &|_| {}, &Operation::detached())
            .is_err()
    );
    assert_eq!(
        f.engine
            .installed()
            .unwrap()
            .get("main/tool")
            .unwrap()
            .version_text(),
        "1.9"
    );
    assert_eq!(f.apps("one"), ["main/tool"]);
    f.fake.fail.lock().unwrap().clear();
    let log = log_lines();
    f.engine
        .update(
            "main/tool",
            None,
            &|l| log.borrow_mut().push(l.to_owned()),
            &Operation::detached(),
        )
        .unwrap();
    let installed = f.engine.installed().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed.get("main/tool").unwrap().version_text(), "1.10");
    assert_eq!(read(&f, "core/apps/main/tool/tool.exe"), "tool-1.10");
    assert_eq!(f.apps("two"), ["main/tool"]);
    assert!(
        log.borrow().contains(
            &"Actualizado main/tool: 1.9 → 1.10. Ruta, persistencia y activación conservadas."
                .to_owned()
        )
    );
    assert_eq!(
        error_text(
            f.engine
                .update("main/tool", None, &|_| {}, &Operation::detached())
        ),
        "No hay una versión posterior en el catálogo local"
    );
}

#[test]
fn update_no_toca_otros_componentes_ni_profiles_inactivos() {
    let f = fx();
    f.bucket(
        "main",
        json!({"tool": {"version": "1", "url": "https://example.org/t1.zip", "hash": ZERO_HASH, "bin": "tool.exe"},
               "other": {"version": "1", "url": "https://example.org/o1.zip", "hash": ZERO_HASH, "bin": "tool.exe"}}),
    );
    f.profile("one");
    f.profile("two");
    f.install(&["main/tool", "main/other"], Some("one"))
        .unwrap();
    f.engine.set_active("two", "main/other", true).unwrap();
    let before = fs::read(f.path("core/apps/main/other/tool.exe")).unwrap();
    f.bucket(
        "main",
        json!({"tool": {"version": "2", "url": "https://example.org/t2.zip", "hash": ZERO_HASH, "bin": "tool.exe"},
               "other": {"version": "1", "url": "https://example.org/o1.zip", "hash": ZERO_HASH, "bin": "tool.exe"}}),
    );
    f.engine
        .update("main/tool", None, &|_| {}, &Operation::detached())
        .unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/other/tool.exe")).unwrap(),
        before
    );
    assert_eq!(f.apps("two"), ["main/other"]);
    assert_eq!(f.apps("one"), ["main/tool", "main/other"]);
}

#[test]
fn pending_updates_solo_lista_versiones_posteriores_compatibles() {
    let f = fx();
    let app = |version: &str, extra: Value| {
        let mut m = json!({"version": version, "url": format!("https://example.org/{version}.zip"),
                           "hash": ZERO_HASH, "bin": "tool.exe"});
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        m
    };
    f.catalog(
        json!({"a": app("1", json!({})), "b": app("1", json!({})), "c": app("1", json!({}))}),
    );
    f.install(&["main/a", "main/b", "main/c"], None).unwrap();
    f.catalog(json!({
        "a": app("2", json!({})),
        "b": app("2", json!({"url": "https://example.org/b.iso"})),
        "c": app("1", json!({})),
    }));
    assert_eq!(f.engine.pending_updates().unwrap(), ["main/a"]);
    let installed = f.engine.installed().unwrap();
    let candidate = f
        .engine
        .available_update(installed.get("main/b").unwrap(), None)
        .unwrap()
        .unwrap();
    assert_eq!(candidate.version, "2");
    assert_ne!(candidate.status, "Compatible");
    assert!(
        f.engine
            .available_update(installed.get("main/c").unwrap(), None)
            .unwrap()
            .is_none()
    );
}

#[test]
fn update_all_ejecuta_en_orden_omite_los_sin_cambios_y_se_detiene_en_el_primer_fallo() {
    let f = fx();
    let app = |version: &str, bin: &str| {
        json!({"version": version, "url": format!("https://example.org/{version}.zip"),
               "hash": ZERO_HASH, "bin": bin})
    };
    f.catalog(json!({"a": app("1", "tool.exe"), "done": app("1", "tool.exe"), "broken": app("1", "tool.exe")}));
    f.install(&["main/a", "main/done", "main/broken"], None)
        .unwrap();
    f.catalog(json!({"a": app("2", "tool.exe"), "done": app("1", "tool.exe"), "broken": app("2", "missing.exe")}));
    let log = log_lines();
    let ids: Vec<String> = ["main/a", "main/done", "main/broken"]
        .map(str::to_owned)
        .into();
    let error = f
        .engine
        .update_all(
            &ids,
            None,
            &|l| log.borrow_mut().push(l.to_owned()),
            &Operation::detached(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "Comando ausente: missing.exe");
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|l| l.starts_with("Actualizando"))
            .collect::<Vec<_>>(),
        [
            "Actualizando 1 de 3: main/a",
            "Actualizando 3 de 3: main/broken"
        ]
    );
    let installed = f.engine.installed().unwrap();
    assert_eq!(installed.get("main/a").unwrap().version_text(), "2");
    assert_eq!(installed.get("main/broken").unwrap().version_text(), "1");
}

#[test]
fn un_cambio_de_persist_se_rechaza_antes_de_instalar() {
    let f = fx();
    install_tool(&f, "1", &json!({"persist": "data"}), &["one"]);
    f.catalog(catalog_of("2", &json!({"persist": "other-data"})));
    let hits = f.fake.hits.lock().unwrap().len();
    let text = error_text(
        f.engine
            .update("main/tool", None, &|_| {}, &Operation::detached()),
    );
    assert_eq!(
        text,
        "La actualización cambia persist o el aislamiento por profile; requiere una migración de datos explícita."
    );
    assert_eq!(f.fake.hits.lock().unwrap().len(), hits);
}

#[test]
fn un_fallo_al_guardar_un_profile_restaura_perfiles_y_payload() {
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = armed.clone();
    let f = fx_with(move |e| {
        e.fault = Some(Box::new(move |point, subject| {
            if point == "save-profile"
                && subject == "two"
                && flag.load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(std::io::Error::other("write failed"))
            } else {
                Ok(())
            }
        }));
    });
    install_tool(&f, "1.9", &json!({}), &["one", "two"]);
    f.catalog(catalog_of("2.0", &json!({})));
    serve_version(&f, "2.0", &[]);
    armed.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        f.engine
            .update("main/tool", None, &|_| {}, &Operation::detached())
            .is_err()
    );
    assert_eq!(
        f.engine
            .installed()
            .unwrap()
            .get("main/tool")
            .unwrap()
            .version_text(),
        "1.9"
    );
    assert_eq!(read(&f, "core/apps/main/tool/tool.exe"), "tool-1.9");
    assert_eq!(f.apps("one"), ["main/tool"]);
    assert_eq!(f.apps("two"), ["main/tool"]);
    assert!(f.staging_is_empty());
}

fn locked_open(path: &std::path::Path) -> fs::File {
    // Sin FILE_SHARE_DELETE, como una aplicación en ejecución.
    fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(path)
        .unwrap()
}

#[test]
fn un_binario_en_uso_bloquea_actualizar_y_eliminar() {
    let f = fx();
    install_tool(&f, "1", &json!({}), &["one"]);
    let handle = locked_open(&f.path("core/apps/main/tool/tool.exe"));
    f.catalog(catalog_of("2", &json!({})));
    serve_version(&f, "2", &[]);
    let text = error_text(
        f.engine
            .update("main/tool", None, &|_| {}, &Operation::detached()),
    );
    assert!(
        text.starts_with("Cierra la aplicación antes de actualizar; archivo en uso"),
        "{text}"
    );
    let text = error_text(f.engine.remove("main/tool", &|_| {}));
    assert!(
        text.starts_with("Cierra la aplicación antes de eliminar; archivo en uso"),
        "{text}"
    );
    assert_eq!(f.apps("one"), ["main/tool"]);
    assert!(f.staging_is_empty());
    assert_eq!(read(&f, "core/apps/main/tool/tool.exe"), "tool-1");
    drop(handle);
    f.engine.remove("main/tool", &|_| {}).unwrap();
    assert!(!f.path("core/apps/main/tool").exists());
}

#[test]
fn update_conserva_la_persistencia_compartida() {
    let f = fx();
    install_tool(&f, "1", &json!({"persist": "data"}), &["one"]);
    f.env("one");
    fs::write(f.path("home/persist/main/tool/data/user.txt"), "keep").unwrap();
    f.catalog(catalog_of("2", &json!({"persist": "data"})));
    serve_version(&f, "2", &[("data/default.txt", b"default2")]);
    f.engine
        .update("main/tool", None, &|_| {}, &Operation::detached())
        .unwrap();
    f.env("one");
    assert_eq!(read(&f, "core/apps/main/tool/tool.exe"), "tool-2");
    assert_eq!(read(&f, "core/apps/main/tool/data/user.txt"), "keep");
    assert!(mito_win::links::is_junction(
        &f.path("core/apps/main/tool/data")
    ));
}

#[test]
fn install_version_retiene_hasta_liberar_y_sobrevive_a_los_dependientes() {
    let f = fx();
    f.profile("one");
    let tool = |version: &str| {
        json!({"version": version, "url": format!("https://example.org/t{version}.zip"),
               "hash": ZERO_HASH, "bin": "tool.exe"})
    };
    f.bucket(
        "main",
        json!({"tool": tool("2.0"), "app": {
        "version": "1", "url": "https://example.org/app.zip", "hash": ZERO_HASH,
        "bin": "tool.exe", "depends": "tool"}}),
    );
    let pin = json!({"version": "1.5", "source": "git", "commit": "a".repeat(40)});
    let log = log_lines();
    f.engine
        .install_version(
            "main/tool",
            tool("1.5"),
            pin.clone(),
            None,
            &|l| log.borrow_mut().push(l.to_owned()),
            &Operation::detached(),
        )
        .unwrap();
    assert!(log.borrow().contains(
        &"main/tool 1.5 instalado y retenido: Actualizar no lo sustituirá hasta liberarlo en Info.".to_owned()
    ));
    let lock = f
        .engine
        .installed()
        .unwrap()
        .get("main/tool")
        .cloned()
        .unwrap();
    assert_eq!(lock.pinned.as_ref(), Some(&pin));
    assert!(f.engine.available_update(&lock, None).unwrap().is_none());
    // Instalar un dependiente no cambia la versión retenida.
    f.install(&["main/app"], Some("one")).unwrap();
    let lock = f
        .engine
        .installed()
        .unwrap()
        .get("main/tool")
        .cloned()
        .unwrap();
    assert_eq!(lock.version_text(), "1.5");
    assert_eq!(lock.pinned.as_ref(), Some(&pin));
    // Liberar.
    assert!(f.engine.release_version("main/tool", &|_| {}).unwrap());
    assert!(!f.engine.release_version("main/tool", &|_| {}).unwrap());
    let lock = f
        .engine
        .installed()
        .unwrap()
        .get("main/tool")
        .cloned()
        .unwrap();
    assert_eq!(lock.version_text(), "1.5");
    let candidate = f.engine.available_update(&lock, None).unwrap().unwrap();
    assert_eq!(candidate.version, "2.0");
}

// --- test_component_environment ---------------------------------------------------

fn record(manifest: Value) -> Record {
    Record {
        key: "main/tool".into(),
        version: json!("1"),
        manifest_sha256: String::new(),
        path: "core/apps/main/tool".into(),
        manifest,
        pinned: None,
    }
}

#[test]
fn la_inspeccion_resuelve_alcance_arquitectura_y_bin_sin_escribir() {
    let f = fx();
    let root = f.root().to_path_buf();
    fs::remove_dir_all(root.join("core")).unwrap();
    let engine = &f.engine;
    let m = json!({
        "version": "1", "bin": [["bin/tool.exe", "tool"]],
        "env_add_path": "wrong",
        "env_set": {"A": "$dir", "B": "${dir}", "C": "$persist_dir", "D": "${persist_dir}", "E": "LITERAL"},
        "architecture": {"64bit": {"env_add_path": ["bin", "other"]}}
    });
    let payload = root.join("core").join("apps").join("main").join("tool");
    let persist = root.join("home").join("persist").join("main").join("tool");
    let (vars, paths) = engine
        .component_environment(&record(m.clone()), Some("one"))
        .unwrap();
    let get = |name: &str| vars.iter().find(|(k, _)| k == name).unwrap().1.clone();
    assert_eq!(get("A"), payload.display().to_string());
    assert_eq!(get("B"), payload.display().to_string());
    assert_eq!(get("C"), persist.display().to_string());
    assert_eq!(get("D"), persist.display().to_string());
    assert_eq!(get("E"), "LITERAL");
    assert_eq!(
        paths,
        [
            payload.join("bin").display().to_string(),
            payload.join("other").display().to_string()
        ]
    );
    // Con `persist` y `shortcuts` está aislado: copia y datos por profile.
    let mut isolated = m;
    isolated["persist"] = json!("data");
    isolated["shortcuts"] = json!([["bin/tool.exe", "Tool"]]);
    let (vars, paths) = engine
        .component_environment(&record(isolated), Some("one"))
        .unwrap();
    let copy = root
        .join("core")
        .join("profile-apps")
        .join("one")
        .join("main")
        .join("tool");
    assert_eq!(paths[0], copy.join("bin").display().to_string());
    let home = root
        .join("home")
        .join("profiles")
        .join("one")
        .join("persist")
        .join("main")
        .join("tool");
    assert_eq!(
        vars.iter().find(|(k, _)| k == "C").unwrap().1,
        home.display().to_string()
    );
    assert!(!root.join("core").exists());
    // Un manifiesto vacío no aporta nada.
    let (vars, paths) = engine
        .component_environment(&record(json!({})), None)
        .unwrap();
    assert!(vars.is_empty() && paths.is_empty());
    assert!(!root.join("core").exists());
}

// --- isolation.py ------------------------------------------------------------------

fn editor(version: &str) -> Value {
    json!({"version": version, "url": format!("https://example.org/editor-{version}.zip"),
           "hash": ZERO_HASH, "bin": "editor.exe", "persist": "data",
           "shortcuts": [["editor.exe", "Editor"]], "env_add_path": ".",
           "env_set": {"APP_ROOT": "$dir", "APP_DATA": "$persist_dir"}})
}

fn serve_editor(f: &common::Fx, version: &str) {
    f.serve(
        &format!("https://example.org/editor-{version}.zip"),
        zip_of(&[
            ("editor.exe", format!("editor-{version}").as_bytes()),
            ("data/default.txt", b"default"),
        ]),
    );
}

fn isolated_fixture() -> common::Fx {
    let f = fx();
    f.bucket("main", json!({"editor": editor("1")}));
    serve_editor(&f, "1");
    f.profile("one");
    f.profile("two");
    f.install(&["main/editor"], Some("one")).unwrap();
    f.engine.set_active("two", "main/editor", true).unwrap();
    f
}

#[test]
fn la_regla_de_aislamiento_y_los_datos_independientes() {
    let f = isolated_fixture();
    let lock = f
        .engine
        .installed()
        .unwrap()
        .get("main/editor")
        .cloned()
        .unwrap();
    assert!(mito_engine::isolation::isolated(&lock).unwrap());
    for field in ["persist", "shortcuts"] {
        let mut without = lock.clone();
        without.manifest.as_object_mut().unwrap().remove(field);
        assert!(
            !mito_engine::isolation::isolated(&without).unwrap(),
            "{field}"
        );
    }
    let (one, two) = (f.env("one"), f.env("two"));
    assert_ne!(one["APP_ROOT"], two["APP_ROOT"]);
    assert_ne!(one["APP_DATA"], two["APP_DATA"]);
    assert_eq!(one["HOME"], two["HOME"]);
    let payload =
        mito_engine::isolation::payload_path(f.engine.layout(), &lock, Some("one")).unwrap();
    assert_eq!(one["APP_ROOT"], payload.display().to_string());
    fs::write(payload.join("editor.exe"), "changed").unwrap();
    fs::write(payload.join("data").join("one.txt"), "one").unwrap();
    assert_eq!(
        read(&f, "core/profile-apps/two/main/editor/editor.exe"),
        "editor-1"
    );
    assert!(
        !f.path("core/profile-apps/two/main/editor/data/one.txt")
            .exists()
    );
    assert_eq!(read(&f, "core/apps/main/editor/editor.exe"), "editor-1");
    // Desactivar y activar no reescribe los datos de la copia.
    f.engine.set_active("one", "main/editor", false).unwrap();
    f.engine.set_active("one", "main/editor", true).unwrap();
    f.env("one");
    assert_eq!(
        read(&f, "core/profile-apps/one/main/editor/data/one.txt"),
        "one"
    );
    assert_eq!(
        read(&f, "home/profiles/one/persist/main/editor/data/one.txt"),
        "one"
    );
    // Un profile inválido no construye ruta.
    assert!(
        mito_engine::isolation::payload_path(f.engine.layout(), &lock, Some("bad name")).is_err()
    );
}

#[test]
fn remove_limpia_todas_las_copias_y_conserva_los_datos_de_cada_profile() {
    let f = isolated_fixture();
    f.env("one");
    f.env("two");
    f.engine.remove("main/editor", &|_| {}).unwrap();
    assert!(!f.path("core/profile-apps/one/main/editor").exists());
    assert!(!f.path("core/profile-apps/two/main/editor").exists());
    for profile in ["one", "two"] {
        assert_eq!(
            read(
                &f,
                &format!("home/profiles/{profile}/persist/main/editor/data/default.txt")
            ),
            "default"
        );
    }
}

#[test]
fn update_reemplaza_copias_activas_e_inactivas_conservando_los_datos() {
    let f = isolated_fixture();
    f.env("one");
    f.env("two");
    f.engine.set_active("two", "main/editor", false).unwrap();
    fs::write(
        f.path("core/profile-apps/one/main/editor/data/user.txt"),
        "keep-one",
    )
    .unwrap();
    fs::write(
        f.path("core/profile-apps/two/main/editor/data/user.txt"),
        "keep-two",
    )
    .unwrap();
    f.bucket("main", json!({"editor": editor("2")}));
    serve_editor(&f, "2");
    f.engine
        .update("main/editor", None, &|_| {}, &Operation::detached())
        .unwrap();
    assert_eq!(f.engine.installed().unwrap().len(), 1);
    f.engine.set_active("two", "main/editor", true).unwrap();
    for (profile, keep) in [("one", "keep-one"), ("two", "keep-two")] {
        f.env(profile);
        let copy = format!("core/profile-apps/{profile}/main/editor");
        assert_eq!(read(&f, &format!("{copy}/editor.exe")), "editor-2");
        assert_eq!(read(&f, &format!("{copy}/data/user.txt")), keep);
        assert!(mito_win::links::is_junction(
            &f.path(&format!("{copy}/data"))
        ));
    }
}

#[test]
fn un_junction_compartido_de_la_plantilla_no_se_copia_a_los_profiles() {
    let f = fx();
    f.bucket("main", json!({"editor": editor("1")}));
    serve_editor(&f, "1");
    f.profile("one");
    f.install(&["main/editor"], None).unwrap();
    // La plantilla tiene `data` como junction a una persistencia compartida.
    let shared = f.path("home/persist/main/editor/data");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("private.txt"), "private").unwrap();
    fs::remove_dir_all(f.path("core/apps/main/editor/data")).unwrap();
    mito_win::links::create_junction(&shared, &f.path("core/apps/main/editor/data")).unwrap();
    f.engine.set_active("one", "main/editor", true).unwrap();
    f.env("one");
    assert!(
        !f.path("core/profile-apps/one/main/editor/data/private.txt")
            .exists()
    );
    assert!(shared.join("private.txt").is_file());
    // Un enlace en otro sitio de la plantilla sí se rechaza.
    let target = f.path("home/tmp");
    mito_win::links::create_junction(&target, &f.path("core/apps/main/editor/link")).unwrap();
    f.profile("two");
    f.engine.set_active("two", "main/editor", true).unwrap();
    let text = error_text(f.engine.environment_from("two", common::base_env()));
    assert!(
        text.starts_with("Enlace no admitido en la copia del componente: "),
        "{text}"
    );
}

// --- test_activation -----------------------------------------------------------------

#[test]
fn instalar_reemplaza_la_version_sin_activar_el_componente() {
    let f = fx();
    f.profile("test");
    let versions = |v: &str| catalog_of(v, &json!({"env_set": {"JAVA_HOME": "$dir"}}));
    f.catalog(versions("1"));
    let a = f.install(&["main/tool"], None).unwrap();
    f.catalog(versions("2"));
    let b = f.install(&["main/tool"], None).unwrap();
    assert_eq!(a.keys().collect::<Vec<_>>(), ["main/tool"]);
    assert_eq!(b.keys().collect::<Vec<_>>(), ["main/tool"]);
    let installed = f.engine.installed().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed.get("main/tool").unwrap().version_text(), "2");
    assert_eq!(
        installed.get("main/tool").unwrap().path,
        "core/apps/main/tool"
    );
    assert!(f.apps("test").is_empty());
}

#[test]
fn persist_de_archivo_y_directorio_sobrevive_a_mover_la_raiz() {
    let f = fx();
    f.profile("test");
    f.catalog(catalog_of("1", &json!({"persist": ["conf", "user.ini"]})));
    f.serve(
        "https://example.org/tool-1.zip",
        zip_of(&[
            ("tool.exe", b"x"),
            ("conf/settings.ini", b"default"),
            ("user.ini", b"file default"),
        ]),
    );
    f.install(&["main/tool"], None).unwrap();
    f.engine.set_active("test", "main/tool", true).unwrap();
    f.env("test");
    fs::write(f.path("home/persist/main/tool/conf/settings.ini"), "custom").unwrap();
    fs::write(f.path("home/persist/main/tool/user.ini"), "custom file").unwrap();
    let parent = f.root().parent().unwrap().to_path_buf();
    let moved = parent.join(format!(
        "{}-moved",
        f.root().file_name().unwrap().to_string_lossy()
    ));
    fs::rename(f.root(), &moved).unwrap();
    let engine = Engine::new(&moved);
    engine.environment_from("test", common::base_env()).unwrap();
    let payload = engine
        .root()
        .join("core")
        .join("apps")
        .join("main")
        .join("tool");
    assert_eq!(
        fs::read_to_string(payload.join("conf").join("settings.ini")).unwrap(),
        "custom"
    );
    assert_eq!(
        fs::read_to_string(payload.join("user.ini")).unwrap(),
        "custom file"
    );
    let target = mito_win::links::junction_target(&payload.join("conf")).unwrap();
    assert!(
        target.starts_with(engine.root().join("home")),
        "{}",
        target.display()
    );
    fs::rename(&moved, f.root()).unwrap();
}

// --- test_stable_storage -----------------------------------------------------------------

#[test]
fn un_manifiesto_distinto_sustituye_la_misma_version_y_sin_hash_pide_confirmacion() {
    let f = fx();
    f.profile("one");
    let tool = |url: &str, hash: Value| json!({"tool": {"version": "1", "url": url, "hash": hash, "bin": "tool.exe"}});
    f.catalog(tool("https://example.org/old.zip", json!(ZERO_HASH)));
    f.install(&["main/tool"], Some("one")).unwrap();
    let old = f.engine.installed().unwrap();
    f.catalog(tool("https://example.org/new.zip", Value::Null));
    let options = InstallOptions::default();
    let error = f
        .engine
        .install(&["main/tool"], &options, &|_| {}, &Operation::detached())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Instalación cancelada: descargas sin hash no confirmadas"
    );
    assert_eq!(f.engine.installed().unwrap(), old);
    f.install(&["main/tool"], None).unwrap();
    let installed = f.engine.installed().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(
        installed.get("main/tool").unwrap().path,
        old.get("main/tool").unwrap().path
    );
    assert_eq!(f.apps("one"), ["main/tool"]);
    assert_eq!(
        read(&f, "core/apps/main/tool/tool.exe"),
        "https://example.org/new.zip"
    );
}

#[test]
fn reemplazar_una_dependencia_no_activa_al_solicitante_en_otros_profiles() {
    let f = fx();
    let app = |version: &str, depends: Option<&str>| {
        let mut m = json!({"version": version, "url": format!("https://example.org/{version}.zip"),
                           "hash": ZERO_HASH, "bin": "tool.exe"});
        if let Some(depends) = depends {
            m["depends"] = json!(depends);
        }
        m
    };
    f.catalog(json!({"runtime": app("1", None)}));
    f.profile("one");
    f.profile("two");
    f.install(&["main/runtime"], Some("one")).unwrap();
    f.engine.set_active("two", "main/runtime", false).unwrap();
    f.catalog(json!({
        "runtime": app("2", Some("helper")),
        "helper": app("1", None),
        "editor": app("1", Some("runtime")),
    }));
    f.install(&["main/editor"], Some("two")).unwrap();
    assert_eq!(f.apps("one"), ["main/runtime", "main/helper"]);
    let two: std::collections::BTreeSet<String> = f.apps("two").into_iter().collect();
    assert_eq!(
        two,
        ["main/helper", "main/runtime", "main/editor"]
            .map(str::to_owned)
            .into()
    );
    assert_eq!(
        f.engine
            .installed()
            .unwrap()
            .get("main/runtime")
            .unwrap()
            .version_text(),
        "2"
    );
}

#[test]
fn el_plan_entero_se_prepara_antes_de_reemplazar_nada() {
    let f = fx();
    f.profile("one");
    let app = |version: &str, bin: &str| {
        json!({"version": version, "url": format!("https://example.org/{version}.zip"),
               "hash": ZERO_HASH, "bin": bin})
    };
    f.catalog(json!({"tool": app("1", "tool.exe")}));
    f.install(&["main/tool"], Some("one")).unwrap();
    let old = f.engine.installed().unwrap();
    f.catalog(json!({"tool": app("2", "tool.exe"), "broken": app("1", "missing.exe")}));
    let text = error_text(f.install(&["main/tool", "main/broken"], None));
    assert!(text.starts_with("Comando ausente"), "{text}");
    assert_eq!(f.engine.installed().unwrap(), old);
    assert_eq!(f.apps("one"), ["main/tool"]);
    assert!(f.staging_is_empty());
}

#[test]
fn un_fallo_al_preparar_una_copia_restaura_binarios_y_datos() {
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = armed.clone();
    let f = fx_with(move |e| {
        e.fault = Some(Box::new(move |point, subject| {
            if point == "prepare-payload"
                && subject == "two"
                && flag.load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(std::io::Error::other("copy failed"))
            } else {
                Ok(())
            }
        }));
    });
    f.bucket("main", json!({"tool": editor("1")}));
    f.profile("one");
    f.profile("two");
    serve_editor(&f, "1");
    f.install(&["main/tool"], Some("one")).unwrap();
    f.engine.set_active("two", "main/tool", true).unwrap();
    let old = f.engine.installed().unwrap();
    for name in ["one", "two"] {
        f.env(name);
        fs::write(
            f.path(&format!("core/profile-apps/{name}/main/tool/editor.exe")),
            name,
        )
        .unwrap();
        fs::write(
            f.path(&format!("core/profile-apps/{name}/main/tool/data/user.txt")),
            name,
        )
        .unwrap();
    }
    f.bucket("main", json!({"tool": editor("2")}));
    serve_editor(&f, "2");
    armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = f.install(&["main/tool"], None).unwrap_err();
    assert!(error.to_string().contains("copy failed"), "{error}");
    armed.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(f.engine.installed().unwrap(), old);
    for name in ["one", "two"] {
        let copy = format!("core/profile-apps/{name}/main/tool");
        assert_eq!(read(&f, &format!("{copy}/editor.exe")), name);
        assert_eq!(read(&f, &format!("{copy}/data/user.txt")), name);
        assert_eq!(f.apps(name), ["main/tool"]);
    }
    assert!(f.staging_is_empty());
    let _ = (BTreeMap::<String, String>::new(), quiet(&f.engine));
}
