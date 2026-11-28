//! Pruebas de las acciones de F8 del hilo de trabajo, sobre un escritorio, un `AppData` del
//! host y una rama de registro temporales.

use std::fs;

use mito_desktop::open_with::entries as open_with_entries;
use serde_json::json;

use super::*;
use crate::integration::testing::{Fakes, Sandbox, integration};
use crate::model::fixture::{Fx, fx, spanish};
use crate::worker::tests::{Outcome, Script, run_action_with};

/// Instancia con escritorio, `AppData` del host y registro temporales.
struct Rig {
    f: Fx,
    desktop: tempfile::TempDir,
    host: tempfile::TempDir,
    sandbox: Sandbox,
    fakes: Fakes,
    roots: HostRoots,
}

fn rig() -> Rig {
    let f = fx();
    let desktop = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let roots = HostRoots::new(host.path().join("Roaming"), host.path().join("Local"));
    fs::create_dir_all(roots.folder(Kind::Data)).unwrap();
    fs::create_dir_all(roots.folder(Kind::Cache)).unwrap();
    f.catalog(json!({"tool": f.manifest("1.0")}));
    f.engine.create_profile("work", None).unwrap();
    f.install("main/tool", Some("work"));
    Rig {
        f,
        desktop,
        host,
        sandbox: Sandbox::new(),
        fakes: Fakes::default(),
        roots,
    }
}

impl Rig {
    fn run(&self, action: Action, answers: &[bool]) -> Outcome {
        let script = Script {
            answers: answers.to_vec(),
            picks: Vec::new(),
        };
        self.run_with(action, script)
    }

    fn run_with(&self, action: Action, script: Script) -> Outcome {
        let integration = integration(
            self.desktop.path(),
            &self.roots,
            &self.sandbox.classes,
            &self.fakes,
        );
        run_action_with(&self.f, action, Some("work"), script, integration)
    }

    fn root(&self) -> &Path {
        self.f.engine.root()
    }

    /// Crea un ejecutable del host.
    fn host_exe(&self, name: &str) -> PathBuf {
        let folder = self.host.path().join("Apps");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        fs::write(&path, b"MZ").unwrap();
        path
    }

    fn register(&self, name: &str) -> String {
        host_apps::add(self.root(), &self.host_exe(name)).unwrap()
    }
}

/// Preguntas de enlaces del host: texto traducido y etiqueta del botón.
fn texts(out: &Outcome) -> Vec<(String, &'static str)> {
    let tr = spanish();
    out.requests
        .iter()
        .filter_map(|request| match request {
            Request::HostConfirm { message, label } => Some((tr.tr(message), *label)),
            _ => None,
        })
        .collect()
}

#[test]
fn host_add_sin_hallazgos_abre_el_selector_registra_y_ofrece_data_y_cache() {
    let rig = rig();
    let exe = rig.host_exe("Foo.exe");
    fs::create_dir_all(rig.roots.folder(Kind::Data).join("FooData")).unwrap();
    fs::create_dir_all(rig.roots.folder(Kind::Cache).join("FooCache")).unwrap();
    rig.fakes.files.lock().unwrap().push_back(Some(exe));
    // Data se acepta; Cache se rechaza.
    let out = rig.run(Action::HostAdd, &[true, false]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    assert!(out.offered.is_empty(), "sin hallazgos no hay diálogo");
    assert!(out.said("Buscando aplicaciones en el menú Inicio y el escritorio del host…"));
    assert!(
        out.said(
            "Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado."
        )
    );
    assert!(out.said("Buscando carpetas del host: data…"));
    assert!(out.said("Buscando carpetas del host: cache…"));
    let asked = texts(&out);
    assert_eq!(asked.len(), 2);
    assert!(
        asked[0]
            .0
            .starts_with("Host data: ¿crear un enlace?\nHost: "),
        "{}",
        asked[0].0
    );
    assert!(asked[1].0.starts_with("Host cache: ¿crear un enlace?"));
    assert_eq!(asked[0].1, "Enlazar");
    let identity = host_apps::inventory(rig.root()).unwrap()[0].0.clone();
    assert!(host_data::linked(rig.root(), &rig.roots, &identity, Kind::Data).unwrap());
    assert!(!host_data::linked(rig.root(), &rig.roots, &identity, Kind::Cache).unwrap());
}

#[test]
fn host_add_con_hallazgos_ofrece_la_lista_y_acepta_examinar_o_cancelar() {
    let rig = rig();
    let exe = rig.host_exe("Bar.exe");
    rig.fakes
        .found
        .lock()
        .unwrap()
        .push(("Bar".to_owned(), exe.clone()));
    let cancelled = rig.run_with(Action::HostAdd, Script::default());
    assert_eq!(cancelled.offered.len(), 1);
    assert_eq!(cancelled.offered[0][0].0, "Bar");
    assert!(host_apps::inventory(rig.root()).unwrap().is_empty());

    rig.fakes.files.lock().unwrap().push_back(Some(exe));
    let browsed = rig.run_with(
        Action::HostAdd,
        Script {
            answers: vec![],
            picks: vec![Pick::Browse],
        },
    );
    assert_eq!(browsed.failure, None);
    assert_eq!(host_apps::inventory(rig.root()).unwrap().len(), 1);

    let other = rig.host_exe("Baz.exe");
    let picked = rig.run_with(
        Action::HostAdd,
        Script {
            answers: vec![],
            picks: vec![Pick::Path(other)],
        },
    );
    assert_eq!(picked.failure, None);
    assert_eq!(host_apps::inventory(rig.root()).unwrap().len(), 2);
}

#[test]
fn host_add_con_el_selector_cancelado_no_registra_nada() {
    let rig = rig();
    rig.fakes.files.lock().unwrap().push_back(None);
    let out = rig.run(Action::HostAdd, &[]);
    assert_eq!(out.failure, None);
    assert!(host_apps::inventory(rig.root()).unwrap().is_empty());
    assert!(
        !out.said(
            "Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado."
        )
    );
}

#[test]
fn host_add_con_un_archivo_no_valido_falla_con_el_mensaje_del_contrato() {
    let rig = rig();
    let text = rig.host.path().join("notas.txt");
    fs::write(&text, b"x").unwrap();
    rig.fakes.files.lock().unwrap().push_back(Some(text));
    let out = rig.run(Action::HostAdd, &[]);
    assert_eq!(
        out.errors(),
        ["Error: Selecciona un archivo .exe, .bat, .cmd o .ps1 existente"]
    );
}

#[test]
fn enlazar_y_desenlazar_los_datos_del_host() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    fs::create_dir_all(rig.roots.folder(Kind::Data).join("FooData")).unwrap();
    let link = |kind| Action::HostLink {
        identity: identity.clone(),
        kind,
    };
    let linked = rig.run(link(Kind::Data), &[true]);
    assert_eq!(linked.failure, None, "{:?}", linked.logs);
    assert!(linked.said("Buscando carpetas del host: data…"));
    assert!(host_data::linked(rig.root(), &rig.roots, &identity, Kind::Data).unwrap());

    let unlinked = rig.run(link(Kind::Data), &[true]);
    let asked = texts(&unlinked);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0]
            .0
            .starts_with("¿Eliminar los enlaces de Host data?\n"),
        "{}",
        asked[0].0
    );
    assert!(asked[0].0.ends_with("\nLos datos del host se conservan."));
    assert_eq!(asked[0].1, "Desenlazar");
    assert!(!host_data::linked(rig.root(), &rig.roots, &identity, Kind::Data).unwrap());
    assert!(
        rig.roots.folder(Kind::Data).join("FooData").is_dir(),
        "los datos del host se conservan"
    );
}

#[test]
fn rechazar_desenlazar_conserva_el_enlace() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    fs::create_dir_all(rig.roots.folder(Kind::Cache).join("FooCache")).unwrap();
    let link = Action::HostLink {
        identity: identity.clone(),
        kind: Kind::Cache,
    };
    rig.run(link.clone(), &[true]);
    rig.run(link, &[false]);
    assert!(host_data::linked(rig.root(), &rig.roots, &identity, Kind::Cache).unwrap());
}

#[test]
fn sin_carpetas_candidatas_se_elige_una_a_mano_sin_preguntar() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    let manual = rig.roots.folder(Kind::Data).join("Otra");
    fs::create_dir_all(&manual).unwrap();
    rig.fakes.folders.lock().unwrap().push_back(Some(manual));
    let out = rig.run(
        Action::HostLink {
            identity: identity.clone(),
            kind: Kind::Data,
        },
        &[true],
    );
    assert_eq!(out.failure, None, "{:?}", out.logs);
    // Solo la pregunta de enlazar: no se pregunta por «Seleccionar».
    let asked = texts(&out);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1, "Enlazar");
    assert!(host_data::linked(rig.root(), &rig.roots, &identity, Kind::Data).unwrap());
}

#[test]
fn con_candidatas_rechazadas_se_pregunta_por_otra_carpeta() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    fs::create_dir_all(rig.roots.folder(Kind::Data).join("FooData")).unwrap();
    rig.fakes.folders.lock().unwrap().push_back(None);
    let out = rig.run(
        Action::HostLink {
            identity,
            kind: Kind::Data,
        },
        &[false, true],
    );
    let asked = texts(&out);
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1],
        (
            "¿Seleccionar otra carpeta del host manualmente?".to_owned(),
            "Seleccionar"
        )
    );
}

#[test]
fn una_carpeta_de_mito_existente_pide_borrar_antes_de_enlazar() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    fs::create_dir_all(rig.roots.folder(Kind::Data).join("FooData")).unwrap();
    let local = rig.root().join("home/AppData/Roaming/FooData");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("a.txt"), b"x").unwrap();
    let out = rig.run(
        Action::HostLink {
            identity: identity.clone(),
            kind: Kind::Data,
        },
        &[true, true],
    );
    assert_eq!(out.failure, None, "{:?}", out.logs);
    let asked = texts(&out);
    assert_eq!(asked[1].1, "Borrar y enlazar");
    assert!(asked[1].0.contains("ya existe y ha de ser borrada"));
    assert!(host_data::linked(rig.root(), &rig.roots, &identity, Kind::Data).unwrap());
}

#[test]
fn abrir_el_host_pasa_profile_argumentos_y_modo() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    let out = rig.run(
        Action::HostOpen {
            identity: identity.clone(),
            arguments: vec!["--x".into()],
            coupled: true,
        },
        &[],
    );
    assert_eq!(out.failure, None);
    assert_eq!(
        *rig.fakes.launched.lock().unwrap(),
        [("work".to_owned(), identity, vec!["--x".to_owned()], true)]
    );
}

#[test]
fn quitar_el_host_conserva_el_ejecutable() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    let out = rig.run(Action::HostRemove(identity), &[]);
    assert!(out.said("Referencia eliminada. El ejecutable y los datos del host se conservan."));
    assert!(host_apps::inventory(rig.root()).unwrap().is_empty());
    assert!(rig.host.path().join("Apps/Foo.exe").is_file());
}

#[test]
fn el_acceso_del_host_se_crea_en_el_escritorio_de_prueba() {
    let rig = rig();
    let identity = rig.register("Foo.exe");
    let shortcut = |identity: &str| Action::HostShortcut {
        identity: identity.to_owned(),
        badge: "FOO".into(),
        color: "naranja".into(),
        coupled: true,
        arguments: vec!["--x".into()],
    };
    let out = rig.run(shortcut(&identity), &[]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    assert!(out.said("Acceso directo al host creado: MITO - work - Host - Foo.lnk"));
    assert!(
        rig.desktop
            .path()
            .join("MITO - work - Host - Foo.lnk")
            .is_file()
    );
    let missing = rig.run(shortcut("nada"), &[]);
    assert_eq!(
        missing.errors(),
        ["Error: La aplicación del host ya no está registrada en MITO"]
    );
    fs::remove_file(rig.host.path().join("Apps/Foo.exe")).unwrap();
    let gone = rig.run(shortcut(&identity), &[]);
    assert_eq!(
        gone.errors(),
        ["Error: El archivo de la aplicación del host ya no está disponible"]
    );
}

#[test]
fn el_acceso_de_aplicacion_y_el_de_mito_se_crean_en_el_escritorio_de_prueba() {
    let rig = rig();
    let out = rig.run(
        Action::ApplicationShortcut {
            installation: "main/tool".into(),
            badge: "TL".into(),
            color: "azul".into(),
            coupled: false,
            arguments: vec![],
            shortcut: None,
        },
        &[],
    );
    assert_eq!(out.failure, None, "{:?}", out.logs);
    let line = out
        .logs
        .iter()
        .map(|(line, _)| line.as_str())
        .find(|line| line.starts_with("Acceso directo creado en el escritorio: "))
        .unwrap();
    assert!(
        line.contains(rig.desktop.path().to_str().unwrap()),
        "{line}"
    );

    let terminal = rig.f.engine.layout().windows_terminal_exe();
    fs::create_dir_all(terminal.parent().unwrap()).unwrap();
    fs::write(&terminal, b"wt").unwrap();
    let mito = rig.run(
        Action::MitoShortcut {
            badge: "DEV".into(),
            color: "naranja".into(),
        },
        &[],
    );
    assert_eq!(mito.failure, None, "{:?}", mito.logs);
    assert!(
        mito.logs
            .iter()
            .any(|(line, _)| line.starts_with("Acceso a MITO creado en el escritorio: ")),
        "{:?}",
        mito.logs
    );
    assert!(rig.desktop.path().join("MITO - work.lnk").is_file());
}

#[test]
fn una_insignia_invalida_no_crea_el_acceso() {
    let rig = rig();
    let out = rig.run(
        Action::ApplicationShortcut {
            installation: "main/tool".into(),
            badge: "TOOLONG".into(),
            color: "azul".into(),
            coupled: false,
            arguments: vec![],
            shortcut: None,
        },
        &[],
    );
    assert_eq!(
        out.errors(),
        ["Error: La etiqueta admite de 1 a 4 letras o números"]
    );
    assert_eq!(fs::read_dir(rig.desktop.path()).unwrap().count(), 0);
}

#[test]
fn extensiones_registra_y_retira_en_la_rama_de_prueba() {
    let rig = rig();
    let record = rig
        .f
        .engine
        .installed()
        .unwrap()
        .get("main/tool")
        .unwrap()
        .clone();
    let label = rig.f.engine.application_entries(&record).unwrap()[0]
        .label
        .clone();
    let apply = |extensions: Vec<String>| Action::OpenWith {
        installation: "main/tool".into(),
        wanted: vec![(label.clone(), extensions)],
    };
    let out = rig.run(apply(vec![".txt".into(), ".md".into()]), &[]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    let records = open_with_entries(rig.root(), "work", "main/tool").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].extensions, [".txt", ".md"]);
    let out = rig.run(apply(vec![]), &[]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    assert!(out.said("Retirado de «Abrir con»."));
    assert!(
        open_with_entries(rig.root(), "work", "main/tool")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exponer_el_appdata_enlaza_y_desenlaza_con_confirmacion() {
    let rig = rig();
    let source = rig.root().join("home/AppData/Roaming/tool");
    fs::create_dir_all(&source).unwrap();
    let expose = Action::HostExpose("main/tool".into());
    let out = rig.run(expose.clone(), &[true]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    let asked = texts(&out);
    assert!(
        asked[0]
            .0
            .starts_with("Roaming en el host: ¿crear un enlace?\nHost: ")
    );
    assert!(
        out.logs
            .iter()
            .any(|(line, _)| line.starts_with("Enlace creado en el host: "))
    );
    assert!(host_exposure::linked(rig.root(), &rig.roots, "main/tool").unwrap());

    let again = rig.run(expose, &[true]);
    let asked = texts(&again);
    assert!(asked[0].0.starts_with("¿Eliminar los enlaces del host?\n"));
    assert_eq!(asked[0].1, "Desenlazar");
    assert!(again.said("Enlaces del host retirados. Los datos de MITO se conservan."));
    assert!(!host_exposure::linked(rig.root(), &rig.roots, "main/tool").unwrap());
}

#[test]
fn exponer_sin_carpetas_lo_dice_y_una_carpeta_del_host_se_aparta() {
    let rig = rig();
    let none = rig.run(Action::HostExpose("main/tool".into()), &[]);
    assert!(none.said(
        "No hay carpetas en el AppData de MITO que coincidan con el ejecutable. Abre antes la aplicación desde MITO."
    ));
    fs::create_dir_all(rig.root().join("home/AppData/Roaming/tool")).unwrap();
    fs::create_dir_all(rig.roots.folder(Kind::Data).join("tool")).unwrap();
    let out = rig.run(Action::HostExpose("main/tool".into()), &[true, true]);
    assert_eq!(out.failure, None, "{:?}", out.logs);
    let asked = texts(&out);
    assert_eq!(asked[1].1, "Apartar y enlazar");
    assert!(asked[1].0.contains(".mito-backup-"), "{}", asked[1].0);
    assert!(host_exposure::linked(rig.root(), &rig.roots, "main/tool").unwrap());
}
