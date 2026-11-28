//! Pruebas de los accesos (`test_shortcuts.py`): `.lnk` reales en una carpeta temporal,
//! sin tocar el escritorio, el menú Inicio ni el registro del usuario.

use std::cell::Cell;
use std::fs;
use std::sync::Mutex;

use mito_engine::Engine;
use mito_win::shell::read_link;
use serde_json::json;

use super::*;
use crate::icons::{Image, encode_png_ico, load_icon_frames};

struct Rig {
    dir: tempfile::TempDir,
    engine: Engine,
    desktop: PathBuf,
}

fn icon_file(path: &Path, color: [u8; 4]) {
    let frames = [
        Image::solid(16, 16, color),
        Image::solid(32, 32, color),
        Image::solid(256, 256, color),
    ];
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, encode_png_ico(&frames).unwrap()).unwrap();
}

const BLUE: [u8; 4] = [26, 75, 160, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Instancia con el profile `one`, el icono de MITO y `extras/tool` instalado y activo.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(dir.path());
    engine.initialize().unwrap();
    engine.create_profile("one", None).unwrap();
    icon_file(&engine.layout().branding_icon(), BLUE);
    let rig = Rig {
        desktop: dir.path().join("Desktop"),
        dir,
        engine,
    };
    install(
        &rig,
        "1.0",
        &json!({"version": "1.0", "shortcuts": [
            ["tool.exe", "Tool", "", "icon.ico"],
            ["tool.exe", "Tool Help", "--help"],
        ]}),
    );
    rig.engine.set_active("one", "extras/tool", true).unwrap();
    rig
}

/// Escribe el marcador y los archivos de `extras/tool`.
fn install(rig: &Rig, version: &str, manifest: &serde_json::Value) {
    let path = "core/apps/extras/tool";
    let folder = rig.dir.path().join(path);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("tool.exe"), b"no es un PE").unwrap();
    icon_file(&folder.join("icon.ico"), RED);
    let mut manifest = manifest.clone();
    manifest["version"] = json!(version);
    let record = Record {
        key: "extras/tool".to_owned(),
        version: json!(version),
        manifest_sha256: "0".repeat(64),
        path: path.to_owned(),
        manifest,
        pinned: None,
    };
    fs::write(
        folder.join(".mito.json"),
        serde_json::to_vec(&record.marker()).unwrap(),
    )
    .unwrap();
}

fn record(rig: &Rig) -> Record {
    rig.engine
        .installed()
        .unwrap()
        .get("extras/tool")
        .unwrap()
        .clone()
}

fn desktop(rig: &Rig) -> impl Fn() -> Result<PathBuf> + '_ {
    || Ok(rig.desktop.clone())
}

fn lnk_files(rig: &Rig) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&rig.desktop)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn registry(rig: &Rig, folder: &str) -> Vec<(String, Value)> {
    let dir = rig.engine.layout().state().join(folder);
    let mut found: Vec<(String, Value)> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        read_json(&e.path()).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn frame_pixel(icon: &Path, index: usize, x: u32, y: u32) -> [u8; 4] {
    load_icon_frames(&fs::read(icon).unwrap(), icon).unwrap()[index].pixel(x, y)
}

#[test]
fn crear_un_profile_no_publica_accesos() {
    let rig = rig();
    rig.engine.create_profile("two", None).unwrap();
    assert!(!rig.desktop.exists());
    assert!(
        !rig.engine
            .layout()
            .state()
            .join("application-shortcuts")
            .exists()
    );
}

#[test]
fn el_acceso_fija_el_profile_y_usa_el_icono_con_insignia() {
    let rig = rig();
    let path = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    assert_eq!(path, rig.desktop.join("one - extras-tool.lnk"));
    let link = read_link(&path).unwrap();
    assert_eq!(link.target, rig.engine.layout().mitow_exe());
    assert_eq!(link.arguments, "run --profile one --app extras/tool");
    assert_eq!(link.working_directory.as_deref(), Some(rig.dir.path()));
    let (icon, index) = link.icon.unwrap();
    assert_eq!(index, 0);
    assert!(icon.starts_with(rig.engine.layout().state().join("icons")));
    // El icono declarado (rojo) lleva la insignia y conserva el 256.
    assert_eq!(frame_pixel(&icon, 2, 0, 0), RED);
    assert!(
        load_icon_frames(&fs::read(&icon).unwrap(), &icon)
            .unwrap()
            .iter()
            .any(|f| f.width == 256)
    );
    // El registro, con las claves y el orden del original.
    let tag = sha256_hex(&[b"one/extras/tool"])[..16].to_owned();
    let records = registry(&rig, "application-shortcuts");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, format!("{tag}.json"));
    let text = fs::read_to_string(
        rig.engine
            .layout()
            .state()
            .join("application-shortcuts")
            .join(&records[0].0),
    )
    .unwrap();
    assert!(text.starts_with("{\n  \"path\": "), "{text}");
    let keys: Vec<&str> = records[0]
        .1
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "path",
            "profile",
            "app",
            "badge",
            "color",
            "coupled",
            "arguments",
            "shortcut"
        ]
    );
    assert_eq!(records[0].1["badge"], "MITO");
    assert_eq!(records[0].1["color"], "naranja");
    assert_eq!(records[0].1["shortcut"], Value::Null);
}

#[test]
fn el_identificador_del_registro_es_el_del_original() {
    // `sha256("test/extras/app")[:16]` del ejemplo del contrato.
    assert_eq!(tag("test/extras/app"), "25caa424deb27105");
    // Insignia `DEV`: `app-25caa424deb27105-4030fe15`.
    assert_eq!(badge_tag("DEV"), "4030fe15");
}

#[test]
fn el_color_de_config_se_usa_y_cambia_el_icono() {
    let rig = rig();
    let first = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    let before = read_link(&first).unwrap().icon.unwrap().0;
    fs::write(
        rig.engine.layout().config_file(),
        "shortcut.badgeColor=Azul\n",
    )
    .unwrap();
    application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    let after = read_link(&first).unwrap().icon.unwrap().0;
    assert_ne!(before, after);
    assert_eq!(
        registry(&rig, "application-shortcuts")[0].1["color"],
        "azul"
    );
    // Color fuera de la paleta: error en cada uso, pero un valor explícito gana.
    fs::write(
        rig.engine.layout().config_file(),
        "shortcut.badgeColor=fucsia\n",
    )
    .unwrap();
    let error = badge_color(&rig.engine, None).unwrap_err().to_string();
    assert_eq!(error, "Elige un color de la paleta del icono");
    assert_eq!(badge_color(&rig.engine, Some(" ROJO ")).unwrap(), "rojo");
}

#[test]
fn el_modo_acoplado_y_los_argumentos_van_al_destino_y_al_registro() {
    let rig = rig();
    let arguments = ["run".to_owned(), "two words".to_owned()];
    let options = LinkOptions {
        coupled: true,
        arguments: &arguments,
        ..LinkOptions::default()
    };
    let path =
        application_link(&rig.engine, &desktop(&rig), "one", "extras/tool", &options).unwrap();
    assert_eq!(
        read_link(&path).unwrap().arguments,
        r#"run --coupled --profile one --app extras/tool -- run "two words""#
    );
    let stored = &registry(&rig, "application-shortcuts")[0].1;
    assert_eq!(stored["coupled"], true);
    assert_eq!(stored["arguments"], json!(["run", "two words"]));
}

#[test]
fn el_nombre_de_la_aplicacion_sale_de_la_etiqueta_del_manifiesto() {
    let rig = rig();
    assert_eq!(
        rig.engine.application_name(&record(&rig), None).unwrap(),
        "Tool"
    );
}

#[test]
fn un_acceso_declarado_usa_su_etiqueta_en_el_nombre_el_registro_y_los_argumentos() {
    let rig = rig();
    let options = LinkOptions {
        shortcut: Some("Tool Help"),
        ..LinkOptions::default()
    };
    let path =
        application_link(&rig.engine, &desktop(&rig), "one", "extras/tool", &options).unwrap();
    assert_eq!(path, rig.desktop.join("one - extras-tool - Tool Help.lnk"));
    assert_eq!(
        read_link(&path).unwrap().arguments,
        r#"run --profile one --app extras/tool --shortcut "Tool Help""#
    );
    let tag = sha256_hex(&[b"one/extras/tool/Tool Help"])[..16].to_owned();
    assert_eq!(
        registry(&rig, "application-shortcuts")[0].0,
        format!("{tag}.json")
    );
    assert_eq!(
        registry(&rig, "application-shortcuts")[0].1["shortcut"],
        "Tool Help"
    );
    // Etiqueta inexistente: error sin traducir del motor y nada escrito.
    let missing = LinkOptions {
        shortcut: Some("Otra"),
        ..LinkOptions::default()
    };
    let error = application_link(&rig.engine, &desktop(&rig), "one", "extras/tool", &missing)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Shortcut unavailable or ambiguous: Otra");
}

#[test]
fn las_etiquetas_se_sanean_para_el_nombre_de_archivo() {
    assert_eq!(
        sanitize_label(r#"  a/b:c*d?"e<f>g|h\i. "#),
        "a_b_c_d__e_f_g_h_i"
    );
    assert_eq!(sanitize_label("ok\u{1}x"), "ok_x");
    assert_eq!(sanitize_label(&"x".repeat(100)).chars().count(), 80);
    assert_eq!(sanitize_label("..."), "");
}

#[test]
fn el_icono_declarado_se_refresca_al_migrar_y_queda_un_solo_acceso() {
    let rig = rig();
    let path = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    let old = read_link(&path).unwrap().icon.unwrap().0;
    assert_eq!(frame_pixel(&old, 0, 0, 0), RED);
    // El componente cambia su icono; la publicación reescribe el acceso registrado.
    icon_file(
        &rig.dir.path().join("core/apps/extras/tool/icon.ico"),
        GREEN,
    );
    let backups = Mutex::new(BTreeMap::new());
    let logged = Mutex::new(Vec::new());
    let log = |line: LogLine| logged.lock().unwrap().push(line.message.to_string());
    migrate_application_links(
        &rig.engine,
        "extras/tool",
        &record(&rig),
        &mut backups.lock().unwrap(),
        &log,
    )
    .unwrap();
    let new = read_link(&path).unwrap().icon.unwrap().0;
    assert_ne!(old, new);
    assert_eq!(frame_pixel(&new, 0, 0, 0), GREEN);
    assert_eq!(lnk_files(&rig), ["one - extras-tool.lnk"]);
    assert_eq!(registry(&rig, "application-shortcuts").len(), 1);
    // El contenido previo del `.lnk` y del registro queda para el rollback.
    let backups = backups.into_inner().unwrap();
    assert!(backups.contains_key(&path));
    assert!(
        backups[&path]
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty())
    );
    assert!(logged.lock().unwrap().is_empty());
}

#[test]
fn un_icono_invalido_cae_al_de_mito_y_uno_ausente_al_ejecutable() {
    let rig = rig();
    let folder = rig.dir.path().join("core/apps/extras/tool");
    // Icono ilegible: insignia sobre el icono de MITO (azul).
    fs::write(folder.join("icon.ico"), b"basura").unwrap();
    let source = component_icon_source(&rig.engine, &record(&rig), "one", None).unwrap();
    assert_eq!(source, folder.join("icon.ico"));
    let icon = application_icon(&rig.engine, "x", "MITO", Some(&source), "naranja").unwrap();
    assert_eq!(frame_pixel(&icon, 0, 0, 0), BLUE);
    // Icono ausente: el origen es el ejecutable, que tampoco es un PE y cae al de MITO.
    fs::remove_file(folder.join("icon.ico")).unwrap();
    let source = component_icon_source(&rig.engine, &record(&rig), "one", None).unwrap();
    assert_eq!(source, folder.join("tool.exe"));
    let icon = application_icon(&rig.engine, "y", "MITO", Some(&source), "naranja").unwrap();
    assert_eq!(frame_pixel(&icon, 0, 0, 0), BLUE);
    // Sin icono de MITO ni base válida, el error es el del original.
    fs::remove_file(rig.engine.layout().branding_icon()).unwrap();
    let error = application_icon(&rig.engine, "z", "MITO", Some(&source), "naranja")
        .unwrap_err()
        .to_string();
    assert!(error.contains("No existe el icono del launcher"), "{error}");
}

#[test]
fn un_icono_fuera_del_componente_es_un_error() {
    let rig = rig();
    install(
        &rig,
        "1.0",
        &json!({"shortcuts": [["tool.exe", "Tool", "", "C:/outside.ico"]]}),
    );
    assert!(component_icon_source(&rig.engine, &record(&rig), "one", None).is_err());
    install(
        &rig,
        "1.0",
        &json!({"shortcuts": [["tool.exe", "Tool", "", "../../outside.ico"]]}),
    );
    assert!(component_icon_source(&rig.engine, &record(&rig), "one", None).is_err());
}

#[test]
fn un_componente_inactivo_o_sin_ejecutable_se_rechaza_antes_del_escritorio() {
    let rig = rig();
    let calls = Cell::new(0);
    let counted = || {
        calls.set(calls.get() + 1);
        Ok(rig.desktop.clone())
    };
    rig.engine.set_active("one", "extras/tool", false).unwrap();
    let error = application_link(
        &rig.engine,
        &counted,
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "El componente debe estar activo en el profile");
    assert_eq!(calls.get(), 0);
    assert!(!rig.desktop.exists());
    // Activo pero sin ejecutable utilizable.
    install(&rig, "1.0", &json!({"shortcuts": [["falta.exe", "X"]]}));
    rig.engine.set_active("one", "extras/tool", true).unwrap();
    assert!(!rig.engine.can_open(&record(&rig)));
    let error = application_link(
        &rig.engine,
        &counted,
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "La app no declara un ejecutable disponible; úsala desde la terminal"
    );
    assert_eq!(calls.get(), 0);
    assert!(!rig.engine.layout().state().join("icons").exists());
}

#[test]
fn la_identidad_del_acceso_no_cambia_con_la_version() {
    let rig = rig();
    let first = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    install(
        &rig,
        "2.0",
        &json!({"shortcuts": [["tool.exe", "Tool", "", "icon.ico"]]}),
    );
    let second = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(lnk_files(&rig), ["one - extras-tool.lnk"]);
    let records = registry(&rig, "application-shortcuts");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].1["app"], "extras/tool");
    assert!(!read_link(&second).unwrap().arguments.contains('@'));
}

#[test]
fn un_lnk_ajeno_no_se_sobrescribe_y_el_propio_se_reutiliza() {
    let rig = rig();
    fs::create_dir_all(&rig.desktop).unwrap();
    fs::write(rig.desktop.join("one - extras-tool.lnk"), b"ajeno").unwrap();
    let first = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    assert_eq!(first, rig.desktop.join("one - extras-tool (2).lnk"));
    assert_eq!(
        fs::read(rig.desktop.join("one - extras-tool.lnk")).unwrap(),
        b"ajeno"
    );
    let again = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    assert_eq!(again, first);
    assert_eq!(lnk_files(&rig).len(), 2);
    // Otro `.lnk` ajeno en el segundo nombre: pasa al (3) la primera vez.
    assert_eq!(
        free_path(&rig.desktop, "one - extras-tool", None),
        rig.desktop.join("one - extras-tool (3).lnk")
    );
}

#[test]
fn la_insignia_por_defecto_el_limite_y_los_cambios_de_profile_conservan_el_icono() {
    let rig = rig();
    assert_eq!(badge_text(&rig.engine, None).unwrap(), "MITO");
    assert_eq!(badge_text(&rig.engine, Some("ab12")).unwrap(), "AB12");
    for bad in ["ABCDE", "A B", "!"] {
        assert_eq!(
            badge_text(&rig.engine, Some(bad)).unwrap_err().to_string(),
            "La etiqueta admite de 1 a 4 letras o números",
            "{bad}"
        );
    }
    fs::write(
        rig.engine.layout().config_file(),
        "shortcut.badgeText=dev\n",
    )
    .unwrap();
    assert_eq!(badge_text(&rig.engine, None).unwrap(), "DEV");
    assert_eq!(badge_text(&rig.engine, Some("x")).unwrap(), "X");
    fs::write(
        rig.engine.layout().config_file(),
        "shortcut.badgeText=ABCDE\n",
    )
    .unwrap();
    assert!(badge_text(&rig.engine, None).is_err());
    fs::write(rig.engine.layout().config_file(), "").unwrap();
    // Desactivar el componente no cambia el icono ya generado.
    let path = application_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "extras/tool",
        &LinkOptions::default(),
    )
    .unwrap();
    let icon = read_link(&path).unwrap().icon.unwrap().0;
    let stamp = fs::metadata(&icon).unwrap().modified().unwrap();
    rig.engine.set_active("one", "extras/tool", false).unwrap();
    rig.engine.set_active("one", "extras/tool", true).unwrap();
    assert_eq!(fs::metadata(&icon).unwrap().modified().unwrap(), stamp);
}

#[test]
fn el_acceso_de_mito_abre_la_terminal_con_el_profile_seleccionado() {
    let rig = rig();
    // Sin Windows Terminal portable, el error es el del original.
    let error = mito_link(&rig.engine, &desktop(&rig), "one", None, None)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap."
    );
    let terminal = rig.engine.layout().windows_terminal_exe();
    fs::create_dir_all(terminal.parent().unwrap()).unwrap();
    fs::write(&terminal, b"wt").unwrap();
    let path = mito_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        Some("dev"),
        Some("verde"),
    )
    .unwrap();
    assert_eq!(path, rig.desktop.join("MITO - one.lnk"));
    let link = read_link(&path).unwrap();
    assert_eq!(link.target.file_name().unwrap(), "WindowsTerminal.exe");
    assert!(
        link.arguments.ends_with(" ui --profile one"),
        "{}",
        link.arguments
    );
    assert!(link.arguments.contains("new-tab"), "{}", link.arguments);
    assert_eq!(link.working_directory.as_deref(), Some(rig.dir.path()));
    assert!(link.icon.unwrap().0.is_file());
    let stored = &registry(&rig, "mito-shortcuts")[0];
    assert_eq!(stored.0, "one.json");
    assert_eq!(
        stored.1,
        json!({
            "path": path.display().to_string(),
            "profile": "one",
            "badge": "DEV",
            "color": "verde",
        })
    );
    // Un profile inexistente no crea nada.
    assert!(mito_link(&rig.engine, &desktop(&rig), "nada", None, None).is_err());
    assert_eq!(lnk_files(&rig), ["MITO - one.lnk"]);
}

#[test]
fn el_acceso_de_host_usa_mitow_y_la_carpeta_del_ejecutable() {
    let rig = rig();
    let host = rig
        .dir
        .path()
        .join("Apps")
        .join("Editor")
        .join("editor.exe");
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    fs::write(&host, b"no es un PE").unwrap();
    // Un `.lnk` ajeno con el nombre natural se respeta.
    fs::create_dir_all(&rig.desktop).unwrap();
    fs::write(rig.desktop.join("MITO - one - Host - editor.lnk"), b"ajeno").unwrap();
    let arguments = ["--port".to_owned(), "8 080".to_owned()];
    let options = LinkOptions {
        badge: Some("test"),
        coupled: true,
        arguments: &arguments,
        ..LinkOptions::default()
    };
    let path = host_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "c853055b51894dc9",
        &host,
        &options,
    )
    .unwrap();
    assert_eq!(path, rig.desktop.join("MITO - one - Host - editor (2).lnk"));
    let link = read_link(&path).unwrap();
    assert_eq!(link.target, rig.engine.layout().mitow_exe());
    assert_eq!(
        link.arguments,
        r#"host-run c853055b51894dc9 --profile one --coupled -- --port "8 080""#
    );
    assert_eq!(link.working_directory.as_deref(), host.parent());
    let tag = sha256_hex(&[b"one/c853055b51894dc9"])[..16].to_owned();
    let (name, stored) = registry(&rig, "host-shortcuts").remove(0);
    assert_eq!(name, format!("{tag}.json"));
    assert_eq!(stored["path"], path.display().to_string());
    let keys: Vec<&str> = stored
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "path",
            "badge",
            "color",
            "profile",
            "application",
            "coupled",
            "arguments"
        ]
    );
    assert_eq!(stored["badge"], "TEST");
    assert_eq!(stored["application"], "c853055b51894dc9");
    let icon = link.icon.unwrap().0;
    assert!(
        icon.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!("host-{tag}-{}-mito-", badge_tag("TEST")))
    );
    // Repetir reutiliza el mismo acceso.
    let again = host_link(
        &rig.engine,
        &desktop(&rig),
        "one",
        "c853055b51894dc9",
        &host,
        &options,
    )
    .unwrap();
    assert_eq!(again, path);
}

fn write_registry(rig: &Rig, name: &str, value: &Value) {
    write_json(
        &rig.engine
            .layout()
            .state()
            .join("application-shortcuts")
            .join(name),
        value,
    )
    .unwrap();
}

#[test]
fn migrar_acepta_registros_antiguos_con_installation_y_deja_el_nuevo_formato() {
    let rig = rig();
    fs::create_dir_all(&rig.desktop).unwrap();
    let path = rig.desktop.join("one - extras-tool.lnk");
    write_link(
        &path,
        &Link {
            target: PathBuf::from(r"C:\old\python.exe"),
            arguments: "-m mito".to_owned(),
            ..Link::default()
        },
    )
    .unwrap();
    let tag = tag("one/extras/tool");
    write_registry(
        &rig,
        &format!("{tag}.json"),
        &json!({
            "path": path.display().to_string(),
            "profile": "one",
            "installation": "extras/tool",
            "badge": "DEV",
            "color": "rojo",
            "coupled": true,
            "arguments": ["a b"],
            "shortcut": null,
        }),
    );
    let mut backups = BTreeMap::new();
    migrate_application_links(
        &rig.engine,
        "extras/tool",
        &record(&rig),
        &mut backups,
        &|_| {},
    )
    .unwrap();
    let link = read_link(&path).unwrap();
    assert_eq!(link.target, rig.engine.layout().mitow_exe());
    assert_eq!(
        link.arguments,
        r#"run --coupled --profile one --app extras/tool -- "a b""#
    );
    let stored = &registry(&rig, "application-shortcuts")[0].1;
    assert_eq!(stored["app"], "extras/tool");
    assert!(stored.get("installation").is_none());
    assert_eq!(stored["badge"], "DEV");
    assert_eq!(stored["color"], "rojo");
    assert_eq!(backups.len(), 2, "{backups:?}");
}

#[test]
fn migrar_conserva_los_accesos_de_profiles_borrados_y_salta_los_que_no_existen() {
    let rig = rig();
    fs::create_dir_all(&rig.desktop).unwrap();
    let path = rig.desktop.join("two - extras-tool.lnk");
    write_link(
        &path,
        &Link {
            target: PathBuf::from(r"C:\old\x.exe"),
            ..Link::default()
        },
    )
    .unwrap();
    let before = fs::read(&path).unwrap();
    write_registry(
        &rig,
        "aaaa.json",
        &json!({
            "path": path.display().to_string(), "profile": "two", "app": "extras/tool",
            "badge": "MITO", "color": "naranja", "coupled": false, "arguments": [], "shortcut": null,
        }),
    );
    // Un registro de otro componente y uno cuyo `.lnk` ya no existe.
    write_registry(
        &rig,
        "bbbb.json",
        &json!({
            "path": path.display().to_string(), "profile": "one", "app": "extras/otro",
        }),
    );
    write_registry(
        &rig,
        "cccc.json",
        &json!({
            "path": rig.desktop.join("no existe.lnk").display().to_string(), "profile": "one", "app": "extras/tool",
        }),
    );
    let logged = Mutex::new(Vec::new());
    let log = |line: LogLine| logged.lock().unwrap().push(line.message.to_string());
    let mut backups = BTreeMap::new();
    migrate_application_links(
        &rig.engine,
        "extras/tool",
        &record(&rig),
        &mut backups,
        &log,
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(backups.is_empty());
    assert_eq!(
        logged.into_inner().unwrap(),
        ["Acceso de profile eliminado conservado sin modificar: two"]
    );
}

#[test]
fn migrar_rechaza_rutas_registradas_que_no_son_lnk_y_argumentos_que_no_son_texto() {
    let rig = rig();
    fs::create_dir_all(&rig.desktop).unwrap();
    let other = rig.desktop.join("nota.txt");
    fs::write(&other, b"x").unwrap();
    write_registry(
        &rig,
        "aaaa.json",
        &json!({
            "path": other.display().to_string(), "profile": "one", "app": "extras/tool",
        }),
    );
    let mut backups = BTreeMap::new();
    let error = migrate_application_links(
        &rig.engine,
        "extras/tool",
        &record(&rig),
        &mut backups,
        &|_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "Acceso registrado no válido");
    fs::remove_file(
        rig.engine
            .layout()
            .state()
            .join("application-shortcuts/aaaa.json"),
    )
    .unwrap();
    let path = rig.desktop.join("one - extras-tool.lnk");
    write_link(
        &path,
        &Link {
            target: PathBuf::from(r"C:\old\x.exe"),
            ..Link::default()
        },
    )
    .unwrap();
    for arguments in [json!("texto"), json!([1, 2])] {
        write_registry(
            &rig,
            "bbbb.json",
            &json!({
                "path": path.display().to_string(), "profile": "one", "app": "extras/tool",
                "badge": "MITO", "color": "naranja", "arguments": arguments,
            }),
        );
        let error = migrate_application_links(
            &rig.engine,
            "extras/tool",
            &record(&rig),
            &mut BTreeMap::new(),
            &|_| {},
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "Acceso registrado no válido");
    }
}

#[test]
fn el_host_de_accesos_se_conecta_al_motor() {
    let rig = rig();
    fs::create_dir_all(&rig.desktop).unwrap();
    let path = rig.desktop.join("one - extras-tool.lnk");
    write_link(
        &path,
        &Link {
            target: PathBuf::from(r"C:\old\x.exe"),
            ..Link::default()
        },
    )
    .unwrap();
    write_registry(
        &rig,
        "aaaa.json",
        &json!({
            "path": path.display().to_string(), "profile": "one", "app": "extras/tool",
            "badge": "MITO", "color": "naranja", "coupled": false, "arguments": [], "shortcut": null,
        }),
    );
    let mut links = BTreeMap::new();
    ShortcutHost
        .migrate_application_links(
            &rig.engine,
            "extras/tool",
            &record(&rig),
            &mut links,
            &|_| {},
        )
        .unwrap();
    assert_eq!(
        read_link(&path).unwrap().target,
        rig.engine.layout().mitow_exe()
    );
    assert!(links.contains_key(&path));
}
