//! Pruebas de F9 de la aplicación: botón INTERNET, sondeo y diálogos de red.
//! Equivalentes de `NetworkSetupUITests` (`test_network_setup.py`). La red es falsa
//! (ver `FakeNetwork`): nunca se toca la red, el proxy ni el registro reales.

use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use mito_engine::network::{Diagnosis, DiagnosisKind};

use super::network::Status;
use super::state::App;
use super::testing::Rig;
use crate::model::fixture::{fx, spanish};
use crate::widgets::{Severity, Variant};

const SECOND: Duration = Duration::from_secs(1);

fn plain(_: &crate::model::fixture::Fx) {}

fn poll_config(seconds: &str) -> impl FnOnce(&crate::model::fixture::Fx) {
    let text = format!("network.poll_seconds={seconds}\n");
    move |f| fs::write(f.engine.root().join("config.properties"), text).unwrap()
}

/// Espera a que no haya ninguna comprobación ni diagnóstico en curso.
fn idle(rig: &mut Rig) {
    rig.pump_until(|app| !app.net.running);
}

fn offline(rig: &Rig, local: bool) {
    rig.fakes.network.script.lock().unwrap().check = (false, local);
}

fn diagnosis(kind: DiagnosisKind) -> Diagnosis {
    Diagnosis {
        kind,
        proxy: "http://proxy.example:8080".into(),
        source: "pac".into(),
        status: 0,
        authenticate: String::new(),
        portal: String::new(),
        changes: Vec::new(),
    }
}

fn portal() -> Diagnosis {
    Diagnosis {
        portal: "http://portal.test/login".into(),
        changes: vec![
            ("https_proxy".into(), "http://proxy.example:8080".into()),
            ("enable_proxy_authentication".into(), "true".into()),
            (
                "proxy_authentication_url".into(),
                "http://portal.test/login".into(),
            ),
        ],
        ..diagnosis(DiagnosisKind::Portal)
    }
}

fn diagnose_with(rig: &Rig, found: Diagnosis) {
    rig.fakes.network.script.lock().unwrap().diagnosis = Ok(found);
}

/// Pulsa INTERNET y espera a que la red quede en reposo.
fn press_internet(rig: &mut Rig) {
    rig.click("network-status");
    idle(rig);
}

fn label(rig: &Rig) -> (String, Variant) {
    rig.app.network_button()
}

fn notice(rig: &Rig, text: &str) -> crate::widgets::Notice {
    rig.app
        .notices
        .iter()
        .find(|n| n.text == text)
        .unwrap_or_else(|| panic!("falta el aviso {text:?}: {:?}", rig.notices()))
        .clone()
}

fn config_text(rig: &Rig) -> Option<String> {
    fs::read_to_string(rig.f.engine.root().join("config.properties")).ok()
}

// --- botón y etiquetas -----------------------------------------------------------------

#[test]
fn el_boton_aparece_tras_configuracion_y_el_foco_lo_incluye() {
    let mut rig = Rig::with(plain);
    let screen = rig.draw();
    assert!(screen.contains("Internet accesible"), "{screen}");
    let at = |text: &str| screen.find(text).unwrap();
    assert!(at("Configuración") < at("Internet accesible"));
    let order = rig.app.focus.order();
    let position = |id: &str| order.iter().position(|known| known == id).unwrap();
    assert_eq!(position("network-status"), position("configuration") + 1);
}

#[test]
fn la_etiqueta_inicial_es_comprobando_internet() {
    let app = App::new(fx().engine, None, spanish());
    assert_eq!(
        app.network_button(),
        ("Comprobando Internet…".to_owned(), Variant::Default)
    );
}

#[test]
fn al_arrancar_se_comprueba_con_registro_y_sin_bloquear() {
    let rig = Rig::with(plain);
    // El primer `check` es el del arranque: con registro (`log = true`).
    assert_eq!(rig.fakes.network.script.lock().unwrap().checks, [true]);
    assert_eq!(rig.app.net.status, Status::Online);
    assert_eq!(
        label(&rig),
        ("Internet accesible".to_owned(), Variant::Default)
    );
}

#[test]
fn con_red_local_pero_sin_internet_la_variante_es_error() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    rig.app.check_network();
    idle(&mut rig);
    assert_eq!(
        label(&rig),
        ("Internet no accesible".to_owned(), Variant::Error)
    );
    assert!(rig.notices().is_empty(), "sin pulsar no se avisa");
}

#[test]
fn sin_red_local_dice_sin_conexion() {
    let mut rig = Rig::with(plain);
    offline(&rig, false);
    rig.app.check_network();
    idle(&mut rig);
    assert_eq!(
        label(&rig),
        ("Sin conexión de red".to_owned(), Variant::Error)
    );
}

#[test]
fn un_error_de_la_comprobacion_deja_internet_no_accesible() {
    let mut rig = Rig::with(plain);
    rig.fakes.network.script.lock().unwrap().check_error = Some("config rota".into());
    rig.app.check_network();
    idle(&mut rig);
    assert_eq!(
        label(&rig),
        ("Internet no accesible".to_owned(), Variant::Error)
    );
}

#[test]
fn repinta_solo_si_cambia() {
    let mut rig = Rig::with(plain);
    let before = rig.app.net.repaints;
    rig.app.check_network();
    idle(&mut rig);
    assert_eq!(
        rig.app.net.repaints, before,
        "mismo resultado, sin repintar"
    );
    offline(&rig, true);
    rig.app.check_network();
    idle(&mut rig);
    assert_eq!(rig.app.net.repaints, before + 1);
}

// --- pulsación y avisos ---------------------------------------------------------------

#[test]
fn pulsar_con_internet_solo_repinta() {
    let mut rig = Rig::with(plain);
    press_internet(&mut rig);
    assert_eq!(rig.fakes.network.script.lock().unwrap().diagnoses, 0);
    assert_eq!(rig.notices().len(), 0);
    assert_eq!(rig.dialogs(), 0);
}

#[test]
fn pulsar_sin_red_local_avisa_y_no_diagnostica() {
    let mut rig = Rig::with(plain);
    offline(&rig, false);
    press_internet(&mut rig);
    assert_eq!(rig.fakes.network.script.lock().unwrap().diagnoses, 0);
    let aviso = notice(&rig, "Conecta el equipo a una red");
    assert_eq!(aviso.severity, Severity::Warning);
}

#[test]
fn pulsar_sin_internet_muestra_diagnosticando_hasta_el_resultado() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Unreachable));
    rig.fakes.network.hold(true);
    rig.click("network-status");
    // Primera comprobación retenida: sigue «Internet accesible» hasta que responda.
    rig.fakes.network.hold(false);
    rig.pump_until(|app| app.net.status == Status::Diagnosing || !app.net.running);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.script.lock().unwrap().diagnoses, 1);
}

#[test]
fn el_diagnostico_offline_avisa() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Offline));
    press_internet(&mut rig);
    let aviso = notice(&rig, "Conecta el equipo a una red");
    assert_eq!(aviso.severity, Severity::Warning);
}

#[test]
fn el_diagnostico_auth_avisa_con_diez_segundos() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Auth));
    let start = Instant::now();
    press_internet(&mut rig);
    let aviso = notice(
        &rig,
        "El proxy http://proxy.example:8080 exige autenticación HTTP 407 (Basic), que MITO no admite.",
    );
    assert_eq!(aviso.severity, Severity::Warning);
    assert!(aviso.until > start + Duration::from_secs(9));
    let mut with = diagnosis(DiagnosisKind::Auth);
    with.authenticate = "NTLM, Negotiate".into();
    diagnose_with(&rig, with);
    rig.app.notices.clear();
    press_internet(&mut rig);
    notice(
        &rig,
        "El proxy http://proxy.example:8080 exige autenticación HTTP 407 (NTLM, Negotiate), que MITO no admite.",
    );
}

#[test]
fn el_diagnostico_blocked_y_unreachable_avisan() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    let mut blocked = diagnosis(DiagnosisKind::Blocked);
    blocked.status = 403;
    diagnose_with(&rig, blocked);
    press_internet(&mut rig);
    let aviso = notice(&rig, "La comprobación de Internet respondió HTTP 403.");
    assert_eq!(aviso.severity, Severity::Warning);
    assert!(aviso.until > Instant::now() + Duration::from_secs(8));
    rig.app.notices.clear();
    diagnose_with(&rig, diagnosis(DiagnosisKind::Unreachable));
    press_internet(&mut rig);
    let aviso = notice(
        &rig,
        "No se pudo llegar a Internet ni con el proxy de Windows ni directamente.",
    );
    assert_eq!(aviso.severity, Severity::Warning);
}

#[test]
fn ok_sin_cambios_y_un_fallo_del_diagnostico_no_avisan() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Ok));
    press_internet(&mut rig);
    assert!(rig.notices().is_empty(), "{:?}", rig.notices());
    rig.fakes.network.script.lock().unwrap().diagnosis = Err("PowerShell roto".into());
    press_internet(&mut rig);
    assert_eq!(rig.notices().len(), 0);
    assert_eq!(rig.dialogs(), 0);
}

#[test]
fn portal_sin_cambios_pide_login_solo_si_esta_activado() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Portal));
    press_internet(&mut rig);
    assert_eq!(rig.dialogs(), 0, "sin portal configurado no hay diálogo");
    fs::write(
        rig.f.engine.root().join("config.properties"),
        "enable_proxy_authentication=true\nproxy_authentication_url=http://portal.test/login\n",
    )
    .unwrap();
    press_internet(&mut rig);
    assert_eq!(rig.dialogs(), 1);
    assert!(rig.draw().contains("AUTENTICACIÓN DE RED"));
}

#[test]
fn una_configuracion_de_red_invalida_se_avisa_al_pedir_el_login_f2() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, diagnosis(DiagnosisKind::Portal));
    fs::write(
        rig.f.engine.root().join("config.properties"),
        "network.timeout_seconds=abc\n",
    )
    .unwrap();
    press_internet(&mut rig);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(rig.app.notices.len(), 1, "{:?}", rig.notices());
    assert_eq!(rig.app.notices[0].severity, Severity::Error);
}

// --- NetworkSetupUITests ---------------------------------------------------------------

#[test]
fn un_portal_detectado_se_guarda_y_luego_se_piden_las_credenciales() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, portal());
    assert_eq!(config_text(&rig), None);

    press_internet(&mut rig);
    assert_eq!(rig.dialogs(), 1);
    rig.click("network-setup-cancel");
    idle(&mut rig);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(config_text(&rig), None, "cancelar no escribe nada");

    press_internet(&mut rig);
    let screen = rig.draw();
    assert!(
        screen.contains("proxy_authentication_url=http://portal.test/login"),
        "{screen}"
    );
    assert!(screen.contains("CONFIGURACIÓN DE RED"));
    rig.click("network-setup-save");
    assert!(rig.draw().contains("AUTENTICACIÓN DE RED"));
    let saved = config_text(&rig).unwrap();
    for line in [
        "https_proxy=http://proxy.example:8080",
        "enable_proxy_authentication=true",
        "proxy_authentication_url=http://portal.test/login",
    ] {
        assert!(saved.contains(line), "{line}\n{saved}");
    }
    assert!(rig.logged("Configuración de red guardada en config.properties"));

    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    idle(&mut rig);
}

#[test]
fn escape_cierra_el_dialogo_de_cambios_sin_guardar() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, portal());
    press_internet(&mut rig);
    rig.press(KeyCode::Esc);
    assert_eq!(rig.dialogs(), 0);
    assert_eq!(config_text(&rig), None);
    idle(&mut rig);
}

#[test]
fn si_guardar_falla_se_avisa_y_no_se_pide_el_login() {
    let mut rig = Rig::with(plain);
    offline(&rig, true);
    diagnose_with(&rig, portal());
    press_internet(&mut rig);
    // `config.properties` como carpeta: no se puede guardar.
    fs::create_dir(rig.f.engine.root().join("config.properties")).unwrap();
    rig.click("network-setup-save");
    assert_eq!(rig.dialogs(), 0);
    assert!(
        rig.app
            .notices
            .iter()
            .any(|n| n.severity == Severity::Error),
        "{:?}",
        rig.notices()
    );
    idle(&mut rig);
}

// --- ProxyDialog desde la aplicación ------------------------------------------------------

/// Deja abierto el `ProxyDialog` (portal ya configurado).
fn open_login(rig: &mut Rig) {
    offline(rig, true);
    diagnose_with(rig, portal());
    press_internet(rig);
    rig.click("network-setup-save");
    assert!(rig.draw().contains("AUTENTICACIÓN DE RED"));
}

/// Da tics hasta que `done` se cumpla.
fn tick_until(rig: &mut Rig, mut done: impl FnMut(&mut Rig) -> bool) {
    let limit = Instant::now() + Duration::from_secs(10);
    while !done(rig) {
        assert!(Instant::now() < limit, "tiempo agotado");
        rig.advance(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn el_login_envia_en_segundo_plano_enmascara_y_no_registra_la_contrasena() {
    let mut rig = Rig::with(plain);
    open_login(&mut rig);
    rig.type_text("ana");
    rig.press(KeyCode::Tab);
    rig.type_text("clave-secreta");
    let screen = rig.draw();
    assert!(screen.contains("•••••••••••••"), "{screen}");
    assert!(!screen.contains("clave-secreta"));
    rig.fakes.network.script.lock().unwrap().check = (true, true);
    rig.press(KeyCode::Enter);
    let screen = rig.draw();
    assert!(!screen.contains("clave-secreta"), "{screen}");
    tick_until(&mut rig, |rig| rig.app.dialogs.is_empty());
    idle(&mut rig);
    assert_eq!(
        rig.fakes.network.script.lock().unwrap().auths,
        [("ana".to_owned(), "clave-secreta".to_owned())]
    );
    // Al cerrar se vuelve a comprobar Internet.
    assert_eq!(label(&rig).0, "Internet accesible");
    let log = fs::read_to_string(rig.f.engine.root().join("logs/mito.log")).unwrap_or_default();
    assert!(!log.contains("clave-secreta"), "{log}");
    assert!(!rig.log_lines().iter().any(|l| l.contains("clave-secreta")));
}

#[test]
fn si_el_login_falla_se_puede_reintentar_o_cancelar() {
    let mut rig = Rig::with(plain);
    open_login(&mut rig);
    rig.fakes.network.script.lock().unwrap().auth = Ok(false);
    rig.type_text("ana");
    rig.press(KeyCode::Enter);
    tick_until(&mut rig, |rig| {
        rig.draw()
            .contains("No se pudo verificar Internet. Puedes reintentar o cancelar.")
    });
    rig.click("proxy-cancel");
    assert_eq!(rig.dialogs(), 0);
    idle(&mut rig);
}

#[test]
fn el_error_de_configuracion_del_login_llega_al_usuario_f1() {
    let mut rig = Rig::with(plain);
    open_login(&mut rig);
    rig.fakes.network.script.lock().unwrap().auth =
        Err("El formulario apunta a otro origen".into());
    rig.type_text("ana");
    rig.press(KeyCode::Enter);
    tick_until(&mut rig, |rig| {
        rig.draw().contains("El formulario apunta a otro origen")
    });
    assert!(!rig.draw().contains("No se pudo verificar Internet"));
    let log = fs::read_to_string(rig.f.engine.root().join("logs/mito.log")).unwrap_or_default();
    assert!(log.contains("network.authenticate"), "{log}");
}

// --- sondeo --------------------------------------------------------------------------------

#[test]
fn el_intervalo_sigue_la_tabla_del_original() {
    let table: [(Option<&str>, Option<u64>); 11] = [
        (None, Some(20)),
        (Some("20"), Some(20)),
        (Some("3"), Some(5)),
        (Some("5"), Some(5)),
        (Some("0"), None),
        (Some("-1"), None),
        (Some("inf"), None),
        (Some("nan"), None),
        (Some("abc"), Some(20)),
        (Some("  12  "), Some(12)),
        (Some("12,5"), Some(20)),
    ];
    for (value, expected) in table {
        let rig = match value {
            Some(value) => Rig::with(poll_config(value)),
            None => Rig::with(plain),
        };
        assert_eq!(
            rig.app.net.poll,
            expected.map(Duration::from_secs),
            "{value:?}"
        );
    }
}

#[test]
fn el_sondeo_es_silencioso_y_periodico() {
    let mut rig = Rig::with(poll_config("5"));
    assert_eq!(rig.fakes.network.checks(), 1);
    rig.advance(4 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 1, "aún no toca");
    rig.advance(SECOND);
    idle(&mut rig);
    let checks = rig.fakes.network.script.lock().unwrap().checks.clone();
    assert_eq!(checks, [true, false], "el sondeo no registra (log = false)");
    assert_eq!(rig.notices().len(), 0);
    rig.advance(5 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 3);
}

#[test]
fn el_sondeo_desactivado_no_comprueba() {
    let mut rig = Rig::with(poll_config("0"));
    rig.advance(120 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 1);
}

#[test]
fn el_sondeo_no_se_solapa_con_una_comprobacion_en_curso() {
    let mut rig = Rig::with(poll_config("5"));
    rig.fakes.network.hold(true);
    rig.app.check_network();
    let network = rig.fakes.network.clone();
    rig.pump_until(|_| network.checks() == 2);
    assert!(rig.app.net.running);
    rig.advance(5 * SECOND);
    rig.advance(5 * SECOND);
    assert_eq!(rig.fakes.network.checks(), 2, "el sondeo esperó");
    rig.fakes.network.hold(false);
    idle(&mut rig);
    rig.advance(5 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 3);
}

#[test]
fn el_sondeo_no_interrumpe_un_dialogo_abierto() {
    let mut rig = Rig::with(poll_config("5"));
    rig.click("manage-profile");
    assert_eq!(rig.dialogs(), 1);
    rig.advance(5 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 1);
    rig.press(KeyCode::Esc);
    rig.advance(5 * SECOND);
    idle(&mut rig);
    assert_eq!(rig.fakes.network.checks(), 2);
}

#[test]
fn el_sondeo_no_repite_el_mismo_error_f3() {
    let mut rig = Rig::with(poll_config("5"));
    rig.fakes.network.script.lock().unwrap().check_error = Some("configuración inválida".into());
    for _ in 0..4 {
        rig.advance(5 * SECOND);
        idle(&mut rig);
    }
    assert_eq!(rig.fakes.network.checks(), 5);
    let log = fs::read_to_string(rig.f.engine.root().join("logs/mito.log")).unwrap_or_default();
    let lines = log
        .lines()
        .filter(|line| line.contains("configuración inválida"))
        .count();
    assert!(lines <= 1, "{lines} líneas\n{log}");
    assert_eq!(
        label(&rig),
        ("Internet no accesible".to_owned(), Variant::Error)
    );
}

#[test]
fn guardar_la_configuracion_global_vuelve_a_comprobar() {
    let mut rig = Rig::with(plain);
    let before = rig.fakes.network.checks();
    rig.click("configuration");
    rig.dialog_focus_on("configuration-save");
    rig.press(KeyCode::Enter);
    idle(&mut rig);
    assert_eq!(rig.dialogs(), 0, "{}", rig.draw());
    assert_eq!(rig.fakes.network.checks(), before + 1);
    assert_eq!(
        rig.fakes.network.script.lock().unwrap().checks.last(),
        Some(&true)
    );
}
