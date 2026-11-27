//! Pruebas de la aplicación sin terminal: entradas simuladas, `TestBackend` y un reloj
//! propio. Equivalentes de las pruebas de F6 de `test_ui.py`.
#![allow(clippy::assert_is_empty)]

use std::fs;
use std::time::Duration;

use crossterm::event::KeyCode;
use mito_core::Operation;
use serde_json::{Value, json};

use super::state::Page;
use super::testing::Rig;
use crate::input::Input;
use crate::model::fixture::Fx;
use crate::theme;
use crate::worker::{Action, Report};

const SECOND: Duration = Duration::from_secs(1);

/// Un profile `work`.
fn work(f: &Fx) {
    f.engine.create_profile("work", None).unwrap();
}

/// Publica `apps` (nombre -> manifiesto) en el catálogo y en el origen del bucket.
fn publish(f: &Fx, apps: &[(&str, Value)]) {
    let snapshot: serde_json::Map<String, Value> = apps
        .iter()
        .map(|(name, manifest)| ((*name).to_owned(), manifest.clone()))
        .collect();
    f.catalog(Value::Object(snapshot));
    for (name, manifest) in apps {
        f.publish(name, manifest);
    }
}

/// Profile `work` y un catálogo con `tool` 1.0 y `editor` 2.0.
fn two_apps(f: &Fx) {
    work(f);
    publish(
        f,
        &[("tool", f.manifest("1.0")), ("editor", f.manifest("2.0"))],
    );
}

/// Profile `work` con `tool` 1.0 instalado y activo.
fn installed(f: &Fx) {
    work(f);
    publish(f, &[("tool", f.manifest("1.0"))]);
    f.install("main/tool", Some("work"));
}

/// Manifiesto sin hash declarado.
fn without_hash(f: &Fx, version: &str) -> Value {
    let mut manifest = f.manifest(version);
    manifest.as_object_mut().unwrap().remove("hash");
    manifest
}

fn installed_version(rig: &Rig, key: &str) -> Option<String> {
    let records = rig.f.engine.installed().unwrap();
    records.get(key).map(mito_engine::Record::version_text)
}

/// Abre el catálogo, busca `term` y deja el cursor de la tabla en la primera fila ya
/// seleccionada.
fn select_first(rig: &mut Rig, term: &str) {
    rig.ctrl('f');
    rig.draw();
    rig.type_text(term);
    rig.focus_on("apps");
    rig.press(KeyCode::Enter);
}

// --- estructura ------------------------------------------------------------------------

#[test]
fn la_barra_lateral_sigue_el_orden_del_original_y_el_titulo_lleva_la_version() {
    let mut rig = Rig::with(work);
    let screen = rig.draw();
    let header = screen.lines().next().unwrap();
    assert!(header.contains(&format!("MITO · {}", env!("CARGO_PKG_VERSION"))));
    assert!(header.contains("My Integrated Terminal Orchestrator"));

    let sidebar: String = screen
        .lines()
        .map(|line| line.chars().take(34).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let order = [
        "PROFILES",
        "work",
        "Gestionar profile",
        "ACCIONES",
        "Inicio · Instalados",
        "Buscar actualizaciones",
        "Abrir Home",
        "Configuración",
    ];
    let mut last = 0;
    for label in order {
        let at = sidebar[last..]
            .find(label)
            .unwrap_or_else(|| panic!("{label} no aparece tras la posición {last}\n{sidebar}"));
        last += at + label.len();
    }
    // Sin lo oculto de fases posteriores.
    for hidden in [
        "Pocketools",
        "MCP",
        "Internet",
        "Actualizar MITO",
        "Agregar aplicación",
    ] {
        assert!(!screen.contains(hidden), "{hidden}");
    }
    assert_eq!(
        rig.app.focus.order(),
        [
            "profile",
            "manage-profile",
            "home-button",
            "check-updates",
            "open-home",
            "configuration",
            "browse",
            "log",
            "copy-log"
        ]
    );
    // El pie con los atajos traducidos.
    let footer = screen.lines().last().unwrap();
    assert!(footer.contains("^q Salir") && footer.contains("^f Buscar componentes"));
    assert!(footer.contains("esc Inicio"));
}

#[test]
fn el_profile_inicial_es_el_pedido_si_existe_y_si_no_el_primero() {
    let setup = |f: &Fx| {
        f.engine.create_profile("alpha", None).unwrap();
        f.engine.create_profile("beta", None).unwrap();
    };
    let rig = Rig::build(setup, Some("beta"));
    assert_eq!(rig.app.current_profile().as_deref(), Some("beta"));
    let rig = Rig::build(setup, Some("inexistente"));
    assert_eq!(rig.app.current_profile().as_deref(), Some("alpha"));
    let rig = Rig::build(setup, None);
    assert_eq!(rig.app.current_profile().as_deref(), Some("alpha"));
}

// --- snapshots ----------------------------------------------------------------------------

#[test]
fn aspecto_de_inicio_vacio() {
    let mut rig = Rig::with(work);
    insta::assert_snapshot!(rig.draw());
}

#[test]
fn aspecto_de_inicio_con_componentes() {
    let mut rig = Rig::with(|f| {
        two_apps(f);
        f.install("main/tool", Some("work"));
        f.install("main/editor", None);
        // Una versión posterior de `tool`.
        publish(
            f,
            &[("tool", f.manifest("1.5")), ("editor", f.manifest("2.0"))],
        );
    });
    insta::assert_snapshot!(rig.draw());
}

#[test]
fn aspecto_del_catalogo_con_filas() {
    let mut rig = Rig::with(two_apps);
    select_first(&mut rig, "");
    insta::assert_snapshot!(rig.draw());
}

#[test]
fn aspecto_del_catalogo_sin_resultados() {
    let mut rig = Rig::with(two_apps);
    rig.ctrl('f');
    rig.draw();
    rig.type_text("zzzz");
    let screen = rig.draw();
    assert!(screen.contains("Sin resultados en los buckets seleccionados."));
    insta::assert_snapshot!(screen);
}

#[test]
fn aspecto_con_un_dialogo_abierto_encima() {
    let mut rig = Rig::with(installed);
    rig.click("remove-0");
    assert_eq!(rig.dialogs(), 1);
    insta::assert_snapshot!(rig.draw());
}

// --- profiles -------------------------------------------------------------------------------

#[test]
fn crear_un_profile_desde_gestionar_profile() {
    let mut rig = Rig::with(work);
    rig.click("manage-profile");
    assert_eq!(rig.dialogs(), 1);
    rig.click("new-profile");
    rig.type_text("nuevo");
    rig.press(KeyCode::Enter);
    assert!(rig.app.is_busy());
    rig.settle();
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.f.engine.profiles(), ["nuevo", "work"]);
    assert!(rig.logged("Profile nuevo creado"));
    assert_eq!(rig.app.current_profile().as_deref(), Some("nuevo"));
}

#[test]
fn borrar_un_profile_pide_confirmacion_y_nunca_borra_el_ultimo() {
    let mut rig = Rig::build(
        |f| {
            f.engine.create_profile("alpha", None).unwrap();
            f.engine.create_profile("beta", None).unwrap();
        },
        Some("beta"),
    );
    // Cancelar no borra.
    rig.click("manage-profile");
    rig.click("delete-profile");
    assert!(rig.draw().contains("beta"));
    rig.click("delete-profile-cancel");
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.f.engine.profiles().len(), 2);
    // Confirmar borra y selecciona el que queda.
    rig.click("manage-profile");
    rig.click("delete-profile");
    rig.click("delete-profile-confirm");
    rig.settle();
    assert_eq!(rig.f.engine.profiles(), ["alpha"]);
    assert_eq!(rig.app.current_profile().as_deref(), Some("alpha"));
    // Con uno solo, «Eliminar profile» no se puede pulsar.
    rig.click("manage-profile");
    assert!(!rig.has("delete-profile"));
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.f.engine.profiles(), ["alpha"]);
}

#[test]
fn las_casillas_activan_sin_desinstalar() {
    let mut rig = Rig::with(installed);
    assert!(rig.has("open-0"));
    rig.click("active-0");
    rig.settle();
    assert!(rig.f.engine.profile("work").unwrap().apps.is_empty());
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("1.0"));
    // Desactivado, «Abrir» desaparece.
    assert!(!rig.has("open-0"));
    rig.click("active-0");
    rig.settle();
    assert_eq!(rig.f.engine.profile("work").unwrap().apps, ["main/tool"]);
    assert!(rig.has("open-0"));
}

#[test]
fn un_cambio_de_profile_no_edita_el_nuevo_profile() {
    let mut rig = Rig::build(
        |f| {
            installed(f);
            f.engine.create_profile("other", None).unwrap();
        },
        Some("work"),
    );
    assert!(rig.draw().contains("1 activos en work"));
    // Abre el selector (foco inicial), sube de «work» a «other» y elige.
    rig.press(KeyCode::Enter);
    rig.press(KeyCode::Up);
    rig.press(KeyCode::Enter);
    assert_eq!(rig.app.current_profile().as_deref(), Some("other"));
    let screen = rig.draw();
    assert!(screen.contains("0 activos en other"), "{screen}");
    assert!(
        !rig.app.busy.active,
        "cambiar de profile no lanza ninguna acción"
    );
    assert!(rig.f.engine.profile("other").unwrap().apps.is_empty());
    assert_eq!(rig.f.engine.profile("work").unwrap().apps, ["main/tool"]);
    assert_eq!(rig.app.home.shown, [false]);
}

// --- Inicio ---------------------------------------------------------------------------------

#[test]
fn eliminar_pide_confirmacion_y_cancelar_no_toca_el_motor() {
    let mut rig = Rig::with(installed);
    rig.click("remove-0");
    let screen = rig.draw();
    assert!(screen.contains("Eliminar componente") && screen.contains("main/tool"));
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert!(!rig.app.is_busy());
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("1.0"));
    // Confirmar elimina el componente.
    rig.click("remove-0");
    rig.click("remove-confirm");
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool"), None);
    assert!(
        rig.draw()
            .contains("Todavía no hay componentes instalados.")
    );
}

#[test]
fn actualizar_solo_aparece_con_una_version_mas_nueva() {
    let mut rig = Rig::with(installed);
    let screen = rig.draw();
    assert!(!screen.contains("Actualizar") && !rig.has("update-0"));

    let mut rig = Rig::with(|f| {
        installed(f);
        publish(f, &[("tool", f.manifest("2.0"))]);
    });
    let screen = rig.draw();
    assert!(screen.contains("[ Actualizar ]") && rig.has("update-0"));
    // Con un solo pendiente no hay «Actualizar todos».
    assert!(!screen.contains("Actualizar todos"));
    rig.click("update-0");
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("2.0"));
    assert!(!rig.draw().contains("Actualizar"));
}

#[test]
fn actualizar_esta_deshabilitado_si_no_es_compatible() {
    let mut rig = Rig::with(|f| {
        installed(f);
        let mut newer = f.manifest("2.0");
        newer["psmodule"] = json!("x");
        publish(f, &[("tool", newer)]);
    });
    let screen = rig.draw();
    assert!(screen.contains("Actualizar"), "se dibuja deshabilitado");
    assert!(!rig.has("update-0"), "pero no se puede pulsar");
    assert!(rig.has("remove-0") && rig.has("info-0"));
}

#[test]
fn actualizar_todos_aparece_con_mas_de_un_pendiente() {
    let mut rig = Rig::with(|f| {
        two_apps(f);
        f.install("main/tool", Some("work"));
        f.install("main/editor", Some("work"));
        publish(
            f,
            &[("tool", f.manifest("1.5")), ("editor", f.manifest("2.5"))],
        );
    });
    assert!(rig.draw().contains("Actualizar todos (2)"));
    rig.click("bulk-update");
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("1.5"));
    assert_eq!(
        installed_version(&rig, "main/editor").as_deref(),
        Some("2.5")
    );
    assert!(!rig.draw().contains("Actualizar todos"));
}

#[test]
fn info_abre_el_dialogo_del_componente() {
    let mut rig = Rig::with(installed);
    rig.click("info-0");
    assert_eq!(rig.dialogs(), 1);
    assert!(rig.draw().contains("INFORMACIÓN DEL COMPONENTE"));
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
}

#[test]
fn abrir_pide_los_argumentos() {
    let mut rig = Rig::with(installed);
    rig.click("open-0");
    assert_eq!(rig.dialogs(), 1);
    assert!(rig.draw().contains("Argumentos"));
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
}

// --- catálogo -------------------------------------------------------------------------------

#[test]
fn ctrl_f_abre_el_catalogo_y_enfoca_la_busqueda_y_esc_vuelve() {
    let mut rig = Rig::with(two_apps);
    assert_eq!(rig.app.page, Page::Home);
    rig.ctrl('f');
    let screen = rig.draw();
    assert_eq!(rig.app.page, Page::Catalog);
    assert_eq!(rig.app.focus.focused(), Some("search"));
    assert!(screen.contains("BUSCAR E INSTALAR · CATÁLOGOS SCOOP"));
    assert!(screen.contains("2 componentes · 2 disponibles"));
    // Sin «Versiones…» (F11).
    assert!(!screen.contains("Versiones"));
    for button in [
        "Instalar",
        "Manifiesto",
        "Actualizar catálogos",
        "Volver a Inicio",
    ] {
        assert!(screen.contains(button), "{button}");
    }
    rig.press(KeyCode::Esc);
    assert_eq!(rig.app.page, Page::Home);
    rig.click("browse");
    assert_eq!(rig.app.page, Page::Catalog);
    rig.click("back");
    assert_eq!(rig.app.page, Page::Home);
}

#[test]
fn filtrar_y_seleccionar_en_el_catalogo_y_ver_instalado_tras_instalar() {
    let mut rig = Rig::with(two_apps);
    rig.ctrl('f');
    rig.draw();
    rig.type_text("tool");
    let screen = rig.draw();
    assert!(
        screen.contains("main/tool") && !screen.contains("main/editor"),
        "{screen}"
    );
    assert!(screen.contains("1 componentes · 1 disponibles"));
    assert!(screen.contains("Selecciona una aplicación."));
    // Sin selección no se puede instalar ni abrir el manifiesto.
    assert!(!rig.has("install") && !rig.has("manifest"));

    rig.focus_on("apps");
    rig.press(KeyCode::Enter);
    let screen = rig.draw();
    assert!(screen.contains("main/tool · 1.0"), "{screen}");
    assert!(screen.contains("Una herramienta") && screen.contains("Compatible"));
    assert!(rig.has("install") && rig.has("manifest"));

    rig.click("install");
    assert!(rig.app.is_busy());
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("1.0"));
    // El catálogo se recarga y la fila pasa a «Instalado».
    let screen = rig.draw();
    assert!(screen.contains("Instalado"), "{screen}");
    assert!(rig.logged("Comprobando main/tool…"));
}

#[test]
fn la_tabla_se_mueve_con_las_flechas_y_el_clic_selecciona() {
    let mut rig = Rig::with(two_apps);
    select_first(&mut rig, "");
    // Orden por clave: editor, tool.
    assert!(rig.draw().contains("main/editor · 2.0"));
    rig.press(KeyCode::Down);
    assert!(rig.draw().contains("main/tool · 1.0"));
    assert_eq!(rig.app.catalog.selected.as_deref(), Some("main/tool"));
}

#[test]
fn el_filtro_de_bucket_persiste_en_catalog_filters_y_oculta_sus_filas() {
    let mut rig = Rig::with(|f| {
        two_apps(f);
        f.bucket("extras", &json!({"extra": f.manifest("3.0")}));
    });
    rig.ctrl('f');
    let screen = rig.draw();
    assert!(
        screen.contains("[X] extras") && screen.contains("[X] main"),
        "{screen}"
    );
    assert!(screen.contains("main/tool") && screen.contains("extras/extra"));
    rig.click("bucket-extras");
    let screen = rig.draw();
    assert!(
        screen.contains("[ ] extras") && !screen.contains("extras/extra"),
        "{screen}"
    );
    let path = rig.f.engine.layout().state().join("catalog-filters.json");
    let saved: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved, json!(["extras"]));
    // Volver a marcarla la muestra otra vez y vacía la lista.
    rig.click("bucket-extras");
    assert!(rig.draw().contains("extras/extra"));
    let path = rig.f.engine.layout().state().join("catalog-filters.json");
    let saved: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved, json!([]));
}

#[test]
fn las_notas_se_ven_en_el_detalle() {
    let mut rig = Rig::with(|f| {
        work(f);
        let mut manifest = f.manifest("1.0");
        manifest["notes"] = json!(["Primera nota", "Segunda nota"]);
        publish(f, &[("tool", manifest)]);
    });
    select_first(&mut rig, "");
    let screen = rig.draw();
    assert!(screen.contains("NOTAS"), "{screen}");
    assert!(screen.contains("Primera nota") && screen.contains("Segunda nota"));
}

#[test]
fn el_manifiesto_se_abre_aunque_haya_una_operacion() {
    let mut rig = Rig::with(two_apps);
    select_first(&mut rig, "tool");
    rig.app.begin(Action::CheckUpdates);
    assert!(rig.app.is_busy());
    // Ocupado, «Instalar» queda deshabilitado pero «Manifiesto» no.
    assert!(!rig.has("install") && !rig.has("refresh"));
    rig.click("manifest");
    assert_eq!(rig.dialogs(), 1);
    assert!(rig.draw().contains("\"version\": \"1.0\""));
    rig.press(KeyCode::Esc);
    rig.settle();
}

#[test]
fn actualizar_catalogos_recarga_las_entradas() {
    let mut rig = Rig::with(two_apps);
    rig.ctrl('f');
    rig.draw();
    // El origen del bucket gana un componente que el catálogo cargado no conoce.
    fs::write(
        rig.f.engine.layout().config_file(),
        format!("bucket.main={}\n", rig.f.source.path().display()),
    )
    .unwrap();
    rig.f.publish("newcomer", &rig.f.manifest("4.0"));
    assert!(!rig.draw().contains("main/newcomer"));
    rig.click("refresh");
    rig.settle();
    assert!(rig.logged("Actualizando main…"));
    assert!(rig.draw().contains("main/newcomer"));
}

// --- ocupado, progreso y paso largo ---------------------------------------------------------

#[test]
fn el_indicador_de_ocupado_aparece_a_los_dos_segundos() {
    let mut rig = Rig::with(work);
    rig.app.begin(Action::CheckUpdates);
    assert!(!rig.draw().contains('⏳'));
    rig.advance(Duration::from_millis(1900));
    assert!(!rig.draw().contains('⏳'));
    rig.advance(Duration::from_millis(200));
    assert!(rig.draw().contains("⏳ …"));
    rig.settle();
    assert!(!rig.draw().contains('⏳'));
}

#[test]
fn una_secuencia_antigua_no_muestra_el_indicador() {
    let mut rig = Rig::with(work);
    rig.app.begin(Action::CheckUpdates);
    rig.settle();
    // La primera operación terminó a los 0 s; la segunda empieza a los 1,5 s.
    rig.advance(Duration::from_millis(1500));
    rig.app.begin(Action::CheckUpdates);
    // Vence el temporizador de la primera (a los 2 s), no el de la segunda (3,5 s).
    rig.advance(Duration::from_millis(600));
    assert!(!rig.app.busy.indicator, "el temporizador viejo no cuenta");
    rig.advance(Duration::from_millis(1500));
    assert!(rig.app.busy.indicator);
    rig.settle();
    assert!(!rig.app.busy.indicator);
}

#[test]
fn ocupado_deshabilita_todo_salvo_lo_permitido() {
    let mut rig = Rig::with(installed);
    rig.app.begin(Action::CheckUpdates);
    rig.draw();
    for id in [
        "manage-profile",
        "check-updates",
        "configuration",
        "active-0",
        "open-0",
        "info-0",
        "remove-0",
        "profile",
    ] {
        assert!(!rig.has(id), "{id} debería estar deshabilitado");
    }
    for id in ["copy-log", "open-home", "home-button", "browse"] {
        assert!(rig.has(id), "{id} debería seguir habilitado");
    }
    rig.settle();
    for id in [
        "manage-profile",
        "check-updates",
        "configuration",
        "active-0",
    ] {
        assert!(rig.has(id), "{id} debería volver a habilitarse");
    }
}

#[test]
fn el_progreso_muestra_los_bytes_y_se_cancela() {
    let mut rig = Rig::with(work);
    let operation = Operation::new(|_| {});
    operation.phase("DESCARGANDO", "Descargando tool", true);
    operation.transfer(5 * 1024 * 1024, Some(20 * 1024 * 1024), "bytes");
    rig.app
        .on_report(Report::Started(operation.clone()), rig.now);
    // Antes de los 2 s no hay diálogo.
    rig.advance(Duration::from_millis(1900));
    assert_eq!(rig.dialogs(), 0);
    rig.advance(Duration::from_millis(200));
    assert_eq!(rig.dialogs(), 1);
    let screen = rig.draw();
    assert!(screen.contains("5.0 MB de 20.0 MB"), "{screen}");
    assert!(screen.contains("Cancelar instalación"));
    // Esc no cierra el progreso ni cambia de página.
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 1);
    rig.click("operation-cancel");
    assert!(operation.is_cancelled());
    // Al terminar la operación el diálogo se cierra.
    rig.app.on_report(
        Report::Done {
            profile: None,
            failure: None,
        },
        rig.now,
    );
    assert_eq!(rig.dialogs(), 0);
}

#[test]
fn cancelar_una_instalacion_lo_registra_y_no_publica_nada() {
    let mut rig = Rig::with(two_apps);
    rig.f
        .fake
        .cancel
        .lock()
        .unwrap()
        .push("tool.exe".to_owned());
    select_first(&mut rig, "tool");
    rig.click("install");
    rig.settle();
    assert!(rig.logged("Instalación cancelada. No se ha publicado nada."));
    assert_eq!(installed_version(&rig, "main/tool"), None);
    // La línea sale en rojo y negrita.
    let buffer = rig.buffer();
    let text = crate::widgets::testing::buffer_text(&buffer);
    let row = text
        .lines()
        .position(|line| line.contains("Instalación cancelada. No se ha publicado nada."))
        .unwrap();
    let cell = &buffer[(0, u16::try_from(row).unwrap())];
    assert_eq!(cell.fg, theme::ERROR_TEXT);
}

#[test]
fn el_paso_largo_muestra_preparando_archivos_y_se_cierra() {
    let mut rig = Rig::with(work);
    let step = |active| Report::Log {
        message: mito_core::Message::new("Extrayendo archivos…"),
        error: false,
        long_step: Some(active),
    };
    rig.app.on_report(step(true), rig.now);
    rig.advance(Duration::from_millis(1900));
    assert_eq!(rig.dialogs(), 0);
    rig.advance(Duration::from_millis(200));
    assert_eq!(rig.dialogs(), 1);
    let screen = rig.draw();
    assert!(screen.contains("PREPARANDO ARCHIVOS") && screen.contains("Extrayendo archivos…"));
    // Esc no hace nada con el aviso abierto.
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 1);
    rig.app.on_report(step(false), rig.now);
    rig.advance(Duration::from_millis(300));
    assert_eq!(rig.dialogs(), 0);
}

#[test]
fn un_paso_largo_que_termina_antes_de_los_dos_segundos_no_se_muestra() {
    let mut rig = Rig::with(work);
    let step = |active| Report::Log {
        message: mito_core::Message::new("Extrayendo archivos…"),
        error: false,
        long_step: Some(active),
    };
    rig.app.on_report(step(true), rig.now);
    rig.advance(SECOND);
    rig.app.on_report(step(false), rig.now);
    rig.advance(Duration::from_secs(3));
    assert_eq!(rig.dialogs(), 0);
}

// --- confirmaciones del hilo ----------------------------------------------------------------

#[test]
fn descargas_sin_hash_esc_no_descarga_y_confirmar_instala() {
    let mut rig = Rig::with(|f| {
        work(f);
        publish(f, &[("tool", without_hash(f, "1.0"))]);
    });
    select_first(&mut rig, "tool");
    rig.click("install");
    rig.pump_until(|app| !app.dialogs.is_empty());
    let screen = rig.draw();
    assert!(screen.contains("DESCARGAS SIN HASH"), "{screen}");
    rig.press(KeyCode::Esc);
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool"), None);
    assert!(
        rig.f.fake.hits.lock().unwrap().is_empty(),
        "no se descargó nada"
    );

    // La lista se recarga al terminar (`entries = []`): hay que volver a seleccionar.
    assert!(!rig.has("install"));
    rig.focus_on("apps");
    rig.press(KeyCode::Enter);
    rig.click("install");
    rig.pump_until(|app| !app.dialogs.is_empty());
    rig.click("unverified-confirm");
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("1.0"));
    assert!(!rig.f.fake.hits.lock().unwrap().is_empty());
}

#[test]
fn una_version_nueva_pide_confirmacion_antes_de_instalar() {
    let mut rig = Rig::with(two_apps);
    select_first(&mut rig, "tool");
    // Mientras el usuario mira 1.0, el origen publica 2.0.
    rig.f.publish("tool", &rig.f.manifest("2.0"));
    rig.click("install");
    rig.pump_until(|app| !app.dialogs.is_empty());
    let screen = rig.draw();
    assert!(
        screen.contains("Nueva versión disponible") && screen.contains("1.0 → 2.0"),
        "{screen}"
    );
    rig.click("install-version-confirm");
    rig.settle();
    assert_eq!(installed_version(&rig, "main/tool").as_deref(), Some("2.0"));
}

#[test]
fn una_descarga_alterada_abre_su_dialogo() {
    let mut rig = Rig::with(two_apps);
    rig.f
        .fake
        .tamper
        .lock()
        .unwrap()
        .push("tool.exe".to_owned());
    select_first(&mut rig, "tool");
    rig.click("install");
    rig.settle();
    assert_eq!(rig.dialogs(), 1);
    let screen = rig.draw();
    assert!(screen.contains("DESCARGA ALTERADA POR LA RED"), "{screen}");
    assert!(screen.contains("proxy.local"));
    assert!(
        rig.log_lines()
            .iter()
            .any(|line| line.starts_with("Error:"))
    );
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(installed_version(&rig, "main/tool"), None);
}

// --- log y avisos ---------------------------------------------------------------------------

#[test]
fn copiar_log_envia_el_texto_plano_y_avisa() {
    let mut rig = Rig::with(work);
    rig.app.write_log("uno".to_owned(), false);
    rig.app.write_log("dos".to_owned(), true);
    let all = rig.log_lines().join("\n");
    rig.click("copy-log");
    assert_eq!(rig.copies.lock().unwrap().as_slice(), [all]);
    assert!(
        rig.notices()
            .contains(&"Log copiado al portapapeles".to_owned())
    );
}

#[test]
fn copiar_un_log_vacio_avisa_y_un_fallo_se_avisa_como_error() {
    let mut rig = Rig::with(work);
    rig.app.log.entries.clear();
    rig.click("copy-log");
    assert!(rig.copies.lock().unwrap().is_empty());
    assert!(rig.notices().contains(&"El log está vacío".to_owned()));

    rig.app.write_log("algo".to_owned(), false);
    rig.failing_clipboard();
    rig.click("copy-log");
    assert!(
        rig.notices()
            .contains(&"No se pudo copiar el log al portapapeles".to_owned())
    );
    assert_eq!(
        rig.app.notices.last().unwrap().severity,
        crate::widgets::Severity::Error
    );
}

#[test]
fn copiar_el_log_funciona_durante_una_operacion() {
    let mut rig = Rig::with(work);
    rig.app.begin(Action::CheckUpdates);
    rig.click("copy-log");
    assert_eq!(rig.copies.lock().unwrap().len(), 1);
    rig.settle();
}

#[test]
fn los_avisos_caducan() {
    let mut rig = Rig::with(work);
    rig.app.log.entries.clear();
    rig.click("copy-log");
    assert_eq!(rig.notices().len(), 1);
    assert!(rig.draw().contains("El log está vacío"));
    rig.advance(Duration::from_secs(6));
    assert!(rig.notices().is_empty());
}

#[test]
fn abrir_home_abre_la_carpeta_de_la_instancia() {
    let mut rig = Rig::with(work);
    rig.click("open-home");
    let opened = rig.opened.lock().unwrap().clone();
    assert_eq!(opened, [rig.f.engine.root().join("home")]);
}

#[test]
fn configuracion_abre_el_dialogo_global() {
    let mut rig = Rig::with(work);
    rig.click("configuration");
    assert_eq!(rig.dialogs(), 1);
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert!(rig.notices().is_empty());
}

// --- salir -----------------------------------------------------------------------------------

#[test]
fn ctrl_q_no_sale_durante_una_operacion() {
    let mut rig = Rig::with(work);
    rig.app.begin(Action::CheckUpdates);
    rig.ctrl('q');
    assert!(!rig.app.should_quit());
    assert!(
        rig.notices()
            .contains(&"Espera a que termine la operación antes de salir".to_owned())
    );
    rig.settle();
    rig.ctrl('q');
    assert!(rig.app.should_quit());
}

#[test]
fn ctrl_q_sale_tambien_con_un_dialogo_abierto() {
    let mut rig = Rig::with(work);
    rig.click("manage-profile");
    assert_eq!(rig.dialogs(), 1);
    rig.input(&Input::ctrl('q'));
    assert!(rig.app.should_quit());
}

// --- teclado y ratón ---------------------------------------------------------------------------

#[test]
fn tab_y_mayus_tab_recorren_los_widgets_de_forma_ciclica() {
    let mut rig = Rig::with(work);
    rig.draw();
    assert_eq!(rig.app.focus.focused(), Some("profile"));
    rig.press(KeyCode::Tab);
    assert_eq!(rig.app.focus.focused(), Some("manage-profile"));
    rig.press(KeyCode::BackTab);
    rig.press(KeyCode::BackTab);
    assert_eq!(rig.app.focus.focused(), Some("copy-log"));
    rig.press(KeyCode::Tab);
    assert_eq!(rig.app.focus.focused(), Some("profile"));
}

#[test]
fn intro_activa_el_boton_enfocado() {
    let mut rig = Rig::with(work);
    rig.focus_on("manage-profile");
    rig.press(KeyCode::Enter);
    assert_eq!(rig.dialogs(), 1);
}

#[test]
fn la_rueda_desplaza_la_lista_de_inicio_y_tab_la_sigue() {
    let mut rig = Rig::with(|f| {
        work(f);
        let apps: Vec<(String, Value)> = (0..12)
            .map(|i| (format!("app{i:02}"), f.manifest("1.0")))
            .collect();
        let apps: Vec<(&str, Value)> = apps.iter().map(|(n, m)| (n.as_str(), m.clone())).collect();
        publish(f, &apps);
        for (name, _) in &apps {
            f.install(&format!("main/{name}"), Some("work"));
        }
    });
    rig.height = 24;
    let screen = rig.draw();
    assert!(
        screen.contains("main/app00") && !screen.contains("main/app11"),
        "{screen}"
    );
    let area = rig.app.focus.area_of("installed-list").unwrap();
    rig.input(&Input::Scroll {
        column: area.x + 2,
        row: area.y + 1,
        down: true,
    });
    rig.input(&Input::Scroll {
        column: area.x + 2,
        row: area.y + 1,
        down: true,
    });
    let screen = rig.draw();
    assert!(!screen.contains("main/app00"), "{screen}");
    // Tab hasta una fila fuera de la vista la hace visible.
    rig.focus_on("remove-11");
    let screen = rig.draw();
    assert!(screen.contains("main/app11"), "{screen}");
}

#[test]
fn con_poca_altura_no_hay_panico() {
    let mut rig = Rig::with(installed);
    for (width, height) in [(30, 10), (80, 24), (10, 5), (1, 1)] {
        rig.width = width;
        rig.height = height;
        rig.draw();
        rig.ctrl('f');
        rig.draw();
        rig.press(KeyCode::Esc);
    }
}
