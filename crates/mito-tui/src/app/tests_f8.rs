//! Pruebas de F8 de la aplicación: equivalentes de `test_ui.py` y `test_host_data_ui.py`.
//! El escritorio, el `AppData` del host y el registro son temporales (ver `Rig`).
#![allow(clippy::assert_is_empty)]

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::KeyCode;
use mito_desktop::host_apps;
use mito_engine::{Engine, Jobs, Result};
use serde_json::{Value, json};

use super::testing::Rig;
use crate::model::fixture::Fx;

/// Profile `work` con `tool` 1.0 instalado y activo.
fn installed(f: &Fx) {
    f.engine.create_profile("work", None).unwrap();
    f.catalog(json!({"tool": f.manifest("1.0")}));
    f.publish("tool", &f.manifest("1.0"));
    f.install("main/tool", Some("work"));
}

/// Un ejecutable del host en la carpeta de pruebas.
fn host_exe(rig: &Rig, name: &str) -> PathBuf {
    let folder = rig.host.path().join("Apps");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    fs::write(&path, b"MZ").unwrap();
    path
}

/// Registra el ejecutable y devuelve su identidad, y refresca Inicio.
fn register(rig: &mut Rig, name: &str) -> String {
    let exe = host_exe(rig, name);
    let identity = host_apps::add(rig.f.engine.root(), &exe).unwrap();
    rig.app.refresh_home();
    identity
}

fn root(rig: &Rig) -> PathBuf {
    rig.f.engine.root().to_path_buf()
}

/// Espera a que el hilo pida un diálogo.
fn wait_dialog(rig: &mut Rig) {
    rig.pump_until(|app| !app.dialogs.is_empty());
}

/// Contenido de los JSON de una carpeta de `core/state`.
fn state_files(rig: &Rig, folder: &str) -> Vec<Value> {
    let dir = root(rig).join("core/state").join(folder);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .map(|entry| {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            serde_json::from_str(&text).unwrap()
        })
        .collect()
}

/// Lanzamiento acoplado: profile, clave, argumentos y entrada.
type Coupled = (String, String, Vec<String>, Option<String>);

/// `Jobs` falsos que anotan los lanzamientos acoplados.
#[derive(Clone, Default)]
struct FakeJobs {
    coupled: Arc<Mutex<Vec<Coupled>>>,
}

impl Jobs for FakeJobs {
    fn coupled(
        &self,
        _engine: &Engine,
        profile: &str,
        key: &str,
        _title: &str,
        arguments: &[String],
        shortcut: Option<&str>,
    ) -> Result<i32> {
        self.coupled.lock().unwrap().push((
            profile.to_owned(),
            key.to_owned(),
            arguments.to_vec(),
            shortcut.map(str::to_owned),
        ));
        Ok(0)
    }
}

// --- aplicaciones del host ----------------------------------------------------------------

#[test]
fn host_application_add_open_shortcut_info_and_remove() {
    let mut rig = Rig::with(installed);
    let exe = host_exe(&rig, "Foo.exe");
    rig.fakes.files.lock().unwrap().push_back(Some(exe));
    rig.click("host-add");
    rig.settle();
    assert!(rig.logged(
        "Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado."
    ));
    let id = host_apps::inventory(&root(&rig)).unwrap()[0].0.clone();
    assert!(rig.draw().contains("Foo · Host"));
    assert!(rig.draw().contains("1 del host"), "{}", rig.draw());

    // Abrir, acoplado: el proceso `attached` llega al lanzador del host.
    rig.click(&format!("host-open-{id}"));
    assert_eq!(rig.dialogs(), 1);
    rig.type_text("--x");
    rig.dialog_focus_on("application-process");
    rig.press(KeyCode::Enter);
    rig.press(KeyCode::Down);
    rig.press(KeyCode::Enter);
    rig.click("application-open-independent");
    rig.settle();
    assert_eq!(
        *rig.fakes.launched.lock().unwrap(),
        [("work".to_owned(), id.clone(), vec!["--x".to_owned()], true)]
    );

    // Acceso directo con el color por defecto (naranja).
    rig.click(&format!("host-link-{id}"));
    assert!(rig.draw().contains("✓ Naranja"));
    rig.click("shortcut-confirm");
    rig.settle();
    assert!(rig.logged("Acceso directo al host creado: MITO - work - Host - Foo.lnk"));
    assert!(
        rig.desktop
            .path()
            .join("MITO - work - Host - Foo.lnk")
            .is_file()
    );
    let registry = state_files(&rig, "host-shortcuts");
    assert_eq!(registry[0]["color"], "naranja");
    assert_eq!(registry[0]["badge"], "MITO");

    // Info.
    rig.click(&format!("host-info-{id}"));
    let screen = rig.draw();
    assert!(screen.contains("APLICACIÓN DEL HOST"), "{screen}");
    assert!(screen.contains("Foo.exe"));
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);

    // Eliminar pide confirmación; cancelar no toca nada.
    rig.click(&format!("host-remove-{id}"));
    assert!(rig.draw().contains("¿Quitar Foo.exe de MITO?"));
    rig.press(KeyCode::Esc);
    assert_eq!(host_apps::inventory(&root(&rig)).unwrap().len(), 1);
    rig.click(&format!("host-remove-{id}"));
    rig.click("host-remove-confirm");
    rig.settle();
    assert!(rig.logged("Referencia eliminada. El ejecutable y los datos del host se conservan."));
    assert!(host_apps::inventory(&root(&rig)).unwrap().is_empty());
    assert!(!rig.draw().contains("Foo · Host"));
    assert!(rig.host.path().join("Apps/Foo.exe").is_file());
}

#[test]
fn host_add_con_hallazgos_abre_la_lista_y_agrega_la_elegida() {
    let mut rig = Rig::with(installed);
    let exe = host_exe(&rig, "Bar.exe");
    rig.fakes
        .found
        .lock()
        .unwrap()
        .push(("Bar".to_owned(), exe));
    rig.click("host-add");
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("AGREGAR APLICACIÓN DEL HOST"));
    rig.type_text("ba");
    rig.click("host-discover-add");
    rig.settle();
    assert_eq!(host_apps::inventory(&root(&rig)).unwrap().len(), 1);

    // Cancelar el diálogo no agrega nada.
    let exe = host_exe(&rig, "Baz.exe");
    rig.fakes
        .found
        .lock()
        .unwrap()
        .push(("Baz".to_owned(), exe));
    rig.click("host-add");
    wait_dialog(&mut rig);
    rig.press(KeyCode::Esc);
    rig.settle();
    assert_eq!(host_apps::inventory(&root(&rig)).unwrap().len(), 1);
}

#[test]
fn abrir_o_acceso_directo_del_host_exigen_profile() {
    let mut rig = Rig::with(|_| {});
    let id = register(&mut rig, "Foo.exe");
    // `Abrir` sin profile está deshabilitado; el acceso avisa.
    assert!(!rig.has(&format!("host-open-{id}")));
    rig.click(&format!("host-link-{id}"));
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.notices(), ["Selecciona un profile"]);
}

#[test]
fn una_aplicacion_del_host_ausente_se_marca_y_no_se_puede_abrir() {
    let mut rig = Rig::with(installed);
    let id = register(&mut rig, "Foo.exe");
    fs::remove_file(rig.host.path().join("Apps/Foo.exe")).unwrap();
    rig.app.refresh_home();
    assert!(rig.draw().contains("Foo · Host · No disponible"));
    assert!(!rig.has(&format!("host-open-{id}")));
    rig.click(&format!("host-info-{id}"));
    assert!(rig.draw().contains("El archivo ya no está disponible."));
}

#[test]
fn las_filas_del_host_tienen_sus_seis_botones_en_la_misma_linea_y_dentro_del_ancho() {
    let mut rig = Rig::with(installed);
    let id = register(&mut rig, "Foo.exe");
    rig.draw();
    let ids =
        ["open", "cache", "data", "link", "info", "remove"].map(|kind| format!("host-{kind}-{id}"));
    let areas: Vec<_> = ids
        .iter()
        .map(|id| rig.app.focus.area_of(id).unwrap_or_else(|| panic!("{id}")))
        .collect();
    assert!(areas.iter().all(|area| area.y == areas[0].y));
    assert!(areas.iter().all(|area| area.right() <= rig.width));
    let screen = rig.draw();
    assert!(
        screen.contains("Cache · ✗") && screen.contains("Data · ✗"),
        "{screen}"
    );
    // Las filas del host llevan el color propio.
    let row = rig.app.focus.area_of(&ids[0]).unwrap();
    let buffer = rig.buffer();
    assert_eq!(
        buffer[(row.x, row.y - 1)].bg,
        crate::theme::HOST_LIGHT,
        "la fila 1 usa la gama clara del host"
    );
}

// --- enlaces de datos y caché -------------------------------------------------------------

#[test]
fn test_add_offers_roaming_then_local_and_accepts_independently() {
    let mut rig = Rig::with(installed);
    let exe = host_exe(&rig, "Foo.exe");
    fs::create_dir_all(
        rig.roots
            .folder(mito_desktop::host_data::Kind::Data)
            .join("FooData"),
    )
    .unwrap();
    fs::create_dir_all(
        rig.roots
            .folder(mito_desktop::host_data::Kind::Cache)
            .join("FooCache"),
    )
    .unwrap();
    rig.fakes.files.lock().unwrap().push_back(Some(exe));
    rig.click("host-add");
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("Host data: ¿crear un enlace?"));
    rig.click("host-data-accept");
    assert_eq!(rig.dialogs(), 0);
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("Host cache: ¿crear un enlace?"));
    // Esc rechaza: solo `data` queda enlazado.
    rig.press(KeyCode::Esc);
    rig.settle();
    let screen = rig.draw();
    assert!(screen.contains("Data · ✓"), "{screen}");
    assert!(screen.contains("Cache · ✗"), "{screen}");
}

#[test]
fn test_manual_link_cancel_replace_unlink_and_buttons_remain() {
    let mut rig = Rig::with(installed);
    let id = register(&mut rig, "Foo.exe");
    let data = rig
        .roots
        .folder(mito_desktop::host_data::Kind::Data)
        .join("FooData");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("keep.txt"), b"x").unwrap();
    // Una carpeta de MITO con el mismo nombre se borrará al enlazar.
    let local = root(&rig).join("home/AppData/Roaming/FooData");
    fs::create_dir_all(&local).unwrap();
    let button = format!("host-data-{id}");
    assert!(rig.draw().contains("Data · ✗"));

    // Cancelar la oferta y no elegir otra carpeta no enlaza.
    rig.click(&button);
    wait_dialog(&mut rig);
    rig.press(KeyCode::Esc);
    wait_dialog(&mut rig);
    assert!(
        rig.draw()
            .contains("¿Seleccionar otra carpeta del host manualmente?")
    );
    rig.press(KeyCode::Esc);
    rig.settle();
    assert!(rig.draw().contains("Data · ✗"));

    // Aceptar enlaza, tras confirmar que se borra la carpeta local de MITO.
    rig.click(&button);
    wait_dialog(&mut rig);
    rig.click("host-data-accept");
    wait_dialog(&mut rig);
    assert!(
        rig.draw()
            .contains("Se perderá su contenido local en MITO.")
    );
    rig.click("host-data-accept");
    rig.settle();
    assert!(rig.draw().contains("Data · ✓"));

    // Aunque se olviden los metadatos, el enlace vivo sigue marcándose.
    fs::remove_file(root(&rig).join("core/state/host-data-links.json")).unwrap();
    rig.app.refresh_home();
    assert!(rig.draw().contains("Data · ✓"));

    // Desenlazar pide confirmación y deja intacta la carpeta del host.
    rig.click(&button);
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("¿Eliminar los enlaces de Host data?"));
    rig.click("host-data-accept");
    rig.settle();
    assert!(rig.draw().contains("Data · ✗"));
    assert!(data.join("keep.txt").is_file());
}

// --- Inicio: botones por componente ---------------------------------------------------------

#[test]
fn test_application_buttons_only_visible_for_active_launchable_app() {
    let mut rig = Rig::with(installed);
    for id in [
        "open-0",
        "link-0",
        "openwith-0",
        "protocol-0",
        "expose-0",
        "info-0",
    ] {
        assert!(rig.has(id), "{id}");
    }
    let screen = rig.draw();
    assert!(
        screen.contains("Acc. directo") && screen.contains("Extensiones ✗"),
        "{screen}"
    );
    assert!(
        screen.contains("Protocolos") && screen.contains("Host · ✗"),
        "{screen}"
    );

    // Acceso directo con el color azul.
    rig.click("link-0");
    assert!(rig.draw().contains("Generar acceso directo"));
    rig.click("shortcut-color-azul");
    assert!(rig.draw().contains("✓ Azul"));
    rig.click("shortcut-confirm");
    rig.settle();
    assert!(
        rig.logs_start("Acceso directo creado en el escritorio: "),
        "{:?}",
        rig.log_lines()
    );
    assert_eq!(
        state_files(&rig, "application-shortcuts")[0]["color"],
        "azul"
    );
    assert_eq!(fs::read_dir(rig.desktop.path()).unwrap().count(), 1);

    // Desactivado, solo quedan Host · (si es lanzable), Info y Eliminar.
    rig.click("active-0");
    rig.settle();
    for id in ["open-0", "link-0", "openwith-0", "protocol-0"] {
        assert!(!rig.has(id), "{id} no debería verse inactivo");
    }
    assert!(rig.has("expose-0") && rig.has("info-0") && rig.has("remove-0"));
}

#[test]
fn abrir_attached_lanza_acoplado_y_detached_no() {
    let jobs = FakeJobs::default();
    let mut rig = Rig::with_jobs(installed, jobs.clone());
    rig.click("open-0");
    rig.dialog_focus_on("application-process");
    rig.press(KeyCode::Enter);
    rig.press(KeyCode::Down);
    rig.press(KeyCode::Enter);
    rig.click("application-open-independent");
    rig.settle();
    let calls = jobs.coupled.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{:?}", rig.log_lines());
    assert_eq!(calls[0].0, "work");
    assert_eq!(calls[0].1, "main/tool");
    assert!(
        rig.log_lines()
            .iter()
            .all(|line| !line.starts_with("Error"))
    );
}

#[test]
fn abrir_attached_sin_soporte_en_el_motor_se_marca_como_error() {
    let mut rig = Rig::with(installed);
    rig.click("open-0");
    rig.dialog_focus_on("application-process");
    rig.press(KeyCode::Enter);
    rig.press(KeyCode::Down);
    rig.press(KeyCode::Enter);
    rig.click("application-open-independent");
    rig.settle();
    assert!(
        rig.log_lines()
            .iter()
            .any(|line| line.starts_with("Error: Modo de lanzamiento no soportado todavía")),
        "{:?}",
        rig.log_lines()
    );
}

#[test]
fn extensiones_registra_y_retira_y_el_boton_lo_refleja() {
    let mut rig = Rig::with(installed);
    assert!(rig.draw().contains("Extensiones ✗"));
    rig.click("openwith-0");
    assert!(rig.draw().contains("Abrir con"));
    rig.type_text("txt");
    rig.press(KeyCode::Enter);
    rig.click("open-with-confirm");
    rig.settle();
    assert!(
        rig.draw().contains("Extensiones ✓"),
        "{:?}",
        rig.log_lines()
    );
    let records = mito_desktop::open_with::entries(&root(&rig), "work", "main/tool").unwrap();
    assert_eq!(records[0].extensions, [".txt"]);

    // Quitar la única extensión retira el registro.
    rig.click("openwith-0");
    assert!(rig.draw().contains(".txt"));
    rig.click("open-with-remove");
    rig.click("open-with-confirm");
    rig.settle();
    assert!(rig.draw().contains("Extensiones ✗"));
    assert!(
        mito_desktop::open_with::entries(&root(&rig), "work", "main/tool")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn protocolos_reclama_vigila_y_al_cerrar_restaura() {
    let mut rig = Rig::with(installed);
    let original = r#""C:\Other\tool.exe" "%1""#;
    rig.sandbox.protocol("toolapp", original);
    rig.click("protocol-0");
    let screen = rig.draw();
    assert!(screen.contains("Reclamar protocolo"), "{screen}");
    assert!(screen.contains("toolapp://  ·  Otra app"), "{screen}");

    rig.click("protocol-toggle-toolapp");
    let claimed = rig.sandbox.command("toolapp").unwrap();
    assert_ne!(claimed, original);
    assert!(rig.draw().contains("Liberar"));

    // Otra aplicación lo sobrescribe: el tic del diálogo lo recupera.
    rig.sandbox.protocol("toolapp", original);
    rig.advance(Duration::from_millis(250));
    assert!(rig.draw().contains("Recuperado a las "), "{}", rig.draw());
    assert_eq!(rig.sandbox.command("toolapp").unwrap(), claimed);

    // Cerrar libera y restaura lo anterior.
    rig.click("protocol-close");
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.sandbox.command("toolapp").unwrap(), original);
}

#[test]
fn abrir_los_protocolos_libera_antes_lo_reclamado_y_esc_cierra_liberando() {
    let mut rig = Rig::with(installed);
    let original = r#""C:\Other\tool.exe" "%1""#;
    rig.sandbox.protocol("toolapp", original);
    rig.click("protocol-0");
    rig.click("protocol-toggle-toolapp");
    assert_ne!(rig.sandbox.command("toolapp").unwrap(), original);
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.sandbox.command("toolapp").unwrap(), original);
}

#[test]
fn el_enlace_del_host_de_un_componente_se_crea_y_se_retira() {
    let mut rig = Rig::with(installed);
    fs::create_dir_all(root(&rig).join("home/AppData/Roaming/tool")).unwrap();
    rig.click("expose-0");
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("Roaming en el host: ¿crear un enlace?"));
    rig.click("host-data-accept");
    rig.settle();
    assert!(rig.draw().contains("Host · ✓"));
    // Sin profile no hace falta: la acción no lo exige, y desenlazar pide confirmación.
    rig.click("expose-0");
    wait_dialog(&mut rig);
    assert!(rig.draw().contains("¿Eliminar los enlaces del host?"));
    rig.click("host-data-accept");
    rig.settle();
    assert!(rig.draw().contains("Host · ✗"));
}

#[test]
fn un_estado_ilegible_se_muestra_como_cruz_sin_romper_inicio() {
    let mut rig = Rig::with(installed);
    fs::write(
        root(&rig).join("core/state/host-exposed-links.json"),
        b"{ no es json",
    )
    .unwrap();
    rig.app.refresh_home();
    let screen = rig.draw();
    assert!(
        screen.contains("Host · ✗") && screen.contains("main/tool"),
        "{screen}"
    );
}

#[test]
fn ocupado_deshabilita_los_botones_de_f8() {
    let mut rig = Rig::with(installed);
    let id = register(&mut rig, "Foo.exe");
    rig.click("check-updates");
    for button in [
        "host-add".to_owned(),
        "link-0".to_owned(),
        "openwith-0".to_owned(),
        "protocol-0".to_owned(),
        "expose-0".to_owned(),
        format!("host-cache-{id}"),
        format!("host-info-{id}"),
    ] {
        assert!(!rig.has(&button), "{button} debería estar deshabilitado");
    }
    rig.settle();
    assert!(rig.has("host-add") && rig.has("link-0"));
}

// --- gestionar profile ------------------------------------------------------------------------

#[test]
fn test_mito_shortcut_and_delete_selected_profile() {
    let mut rig = Rig::with(installed);
    // Hace falta la terminal portable para el destino del acceso.
    let terminal = rig.f.engine.layout().windows_terminal_exe();
    fs::create_dir_all(terminal.parent().unwrap()).unwrap();
    fs::write(&terminal, b"wt").unwrap();

    rig.click("manage-profile");
    rig.click("mito-shortcut");
    let screen = rig.draw();
    assert!(screen.contains("Generar acceso directo"), "{screen}");
    assert!(
        screen.contains("MITO") && screen.contains("✓ Naranja"),
        "{screen}"
    );
    assert!(
        !screen.contains("Proceso"),
        "el acceso a MITO no lleva proceso"
    );
    rig.click("shortcut-confirm");
    rig.settle();
    assert!(rig.logs_start("Acceso a MITO creado en el escritorio: "));
    assert!(rig.desktop.path().join("MITO - work.lnk").is_file());
    let stored = state_files(&rig, "mito-shortcuts");
    assert_eq!(stored[0]["badge"], "MITO");
    assert_eq!(stored[0]["color"], "naranja");
}

#[test]
fn un_valor_invalido_en_config_properties_se_avisa_al_abrir_el_acceso() {
    let mut rig = Rig::with(installed);
    fs::write(
        rig.f.engine.layout().config_file(),
        "shortcut.badgeColor=fucsia\n",
    )
    .unwrap();
    rig.click("link-0");
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.notices(), ["Elige un color de la paleta del icono"]);
}

// --- aspecto -------------------------------------------------------------------------------

#[test]
fn aspecto_de_inicio_con_una_aplicacion_del_host() {
    let mut rig = Rig::with(installed);
    register(&mut rig, "Foo.exe");
    insta::assert_snapshot!(rig.draw());
}
