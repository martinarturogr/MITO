//! Pruebas de `host_exposure` (`test_host_exposure.py`), con junctions reales en carpetas
//! temporales: el `AppData` del usuario nunca se toca.

use std::cell::RefCell;
use std::fs;

use mito_engine::Record;
use serde_json::json;

use super::*;

const KEY: &str = "main/mattermost";

struct Rig {
    _tmp: tempfile::TempDir,
    folder: PathBuf,
    root: PathBuf,
    host: HostRoots,
    engine: Engine,
    record: Record,
    source: PathBuf,
}

/// Instancia con `main/mattermost` instalado (declara `Mattermost.exe`).
fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let folder = dunce::canonicalize(tmp.path()).unwrap();
    let engine = Engine::new(folder.join("mito"));
    engine.initialize().unwrap();
    let root = engine.root().to_path_buf();
    let record = Record {
        key: KEY.to_owned(),
        version: json!("1.0"),
        manifest_sha256: "0".repeat(64),
        path: format!("core/apps/{KEY}"),
        manifest: json!({
            "version": "1.0",
            "shortcuts": [["Mattermost.exe", "Mattermost"]],
        }),
        pinned: None,
    };
    let payload = root.join("core/apps").join(KEY);
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("Mattermost.exe"), b"exe").unwrap();
    fs::write(
        payload.join(".mito.json"),
        serde_json::to_vec(&record.marker()).unwrap(),
    )
    .unwrap();
    let host = HostRoots::new(folder.join("host/Roaming"), folder.join("host/Local"));
    fs::create_dir_all(host.folder(Kind::Data)).unwrap();
    fs::create_dir_all(host.folder(Kind::Cache)).unwrap();
    let source = mito_data(&root);
    Rig {
        _tmp: tmp,
        folder,
        root,
        host,
        engine,
        record,
        source,
    }
}

fn mito_data(root: &Path) -> PathBuf {
    let source = root.join("home/AppData/Roaming/Mattermost");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("config.json"), "mito").unwrap();
    source
}

impl Rig {
    fn host_path(&self) -> PathBuf {
        self.host.folder(Kind::Data).join("Mattermost")
    }

    fn link(&self, aside: bool) -> Result<Option<PathBuf>> {
        link(
            &self.engine,
            &self.host,
            KEY,
            Kind::Data,
            "Mattermost",
            aside,
        )
    }

    fn linked(&self) -> bool {
        linked(&self.root, &self.host, KEY).unwrap()
    }
}

fn message(result: Result<impl std::fmt::Debug>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn los_candidatos_usan_los_nombres_declarados_y_omiten_junctions() {
    let rig = rig();
    fs::create_dir_all(
        rig.root
            .join("home/AppData/Local/mattermost-updater/nested"),
    )
    .unwrap();
    fs::create_dir_all(rig.root.join("home/AppData/Roaming/Other/Mattermost")).unwrap();
    let outside = rig.folder.join("outside");
    fs::create_dir(&outside).unwrap();
    let linked = rig.root.join("home/AppData/Roaming/Mattermost-linked");
    mito_win::links::create_junction(&outside, &linked).unwrap();
    let found = candidates(&rig.engine, &rig.record).unwrap();
    assert!(found.contains(&(Kind::Data, "Mattermost".to_owned())));
    assert!(found.contains(&(Kind::Data, r"Other\Mattermost".to_owned())));
    assert!(found.contains(&(Kind::Cache, "mattermost-updater".to_owned())));
    assert!(!found.contains(&(Kind::Data, "Mattermost-linked".to_owned())));
    assert_eq!(words(&rig.engine, &rig.record).unwrap(), ["mattermost"]);
}

#[test]
fn compartir_los_datos_de_mito_y_desenlazar_los_conserva() {
    let rig = rig();
    let host = rig.host_path();
    assert_eq!(rig.link(false).unwrap(), None);
    assert!(mito_win::links::is_junction(&host));
    assert_eq!(
        fs::read_to_string(host.join("config.json")).unwrap(),
        "mito"
    );
    assert!(rig.linked());
    // Repetir es idempotente y no crea nada.
    let panics = |_: &Path, _: &Path| -> io::Result<()> { panic!("no debe crear") };
    link_with(
        &rig.engine,
        &rig.host,
        KEY,
        Kind::Data,
        "Mattermost",
        false,
        &panics,
    )
    .unwrap();
    unlink(&rig.root, &rig.host, KEY).unwrap();
    assert!(!host.exists());
    assert!(!rig.linked());
    assert_eq!(
        fs::read_to_string(rig.source.join("config.json")).unwrap(),
        "mito"
    );
    assert!(registry(&rig.root).unwrap().is_empty());
}

#[test]
fn una_carpeta_existente_del_host_se_aparta_y_se_restaura() {
    let rig = rig();
    let host = rig.host_path();
    fs::create_dir(&host).unwrap();
    fs::write(host.join("config.json"), "host").unwrap();
    assert_eq!(
        message(rig.link(false)),
        format!("La carpeta del host ya existe: {}", host.display())
    );
    assert!(!mito_win::links::is_junction(&host));
    let moved = rig.link(true).unwrap().unwrap();
    assert!(
        moved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Mattermost.mito-backup-")
    );
    assert_eq!(
        fs::read_to_string(moved.join("config.json")).unwrap(),
        "host"
    );
    assert_eq!(
        fs::read_to_string(host.join("config.json")).unwrap(),
        "mito"
    );
    unlink(&rig.root, &rig.host, KEY).unwrap();
    assert!(!mito_win::links::is_junction(&host));
    assert_eq!(
        fs::read_to_string(host.join("config.json")).unwrap(),
        "host"
    );
    assert!(!moved.exists());
}

#[test]
fn los_junctions_ajenos_o_modificados_nunca_se_reemplazan() {
    let rig = rig();
    let other = rig.folder.join("other");
    fs::create_dir(&other).unwrap();
    let host = rig.host_path();
    mito_win::links::create_junction(&other, &host).unwrap();
    assert_eq!(
        message(rig.link(true)),
        "No se reemplazan enlaces ajenos a MITO"
    );
    mito_win::links::remove_junction(&host).unwrap();
    rig.link(false).unwrap();
    mito_win::links::remove_junction(&host).unwrap();
    mito_win::links::create_junction(&other, &host).unwrap();
    assert_eq!(
        message(unlink(&rig.root, &rig.host, KEY)),
        format!("El enlace ha cambiado fuera de MITO: {}", host.display())
    );
    assert!(mito_win::links::is_junction(&host));
}

#[test]
fn un_fallo_al_crear_el_junction_restaura_la_carpeta_del_host() {
    let rig = rig();
    let host = rig.host_path();
    fs::create_dir(&host).unwrap();
    fs::write(host.join("config.json"), "host").unwrap();
    let denied = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
    let result = link_with(
        &rig.engine,
        &rig.host,
        KEY,
        Kind::Data,
        "Mattermost",
        true,
        &denied,
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(host.join("config.json")).unwrap(),
        "host"
    );
    assert!(registry(&rig.root).unwrap().is_empty());
}

#[test]
fn reubicar_reapunta_tras_mover_y_suelta_tras_copiar() {
    let rig = rig();
    let old = rig.folder.join("old");
    let old_source = mito_data(&old);
    let host = rig.host_path();
    mito_win::links::create_junction(&old_source, &host).unwrap();
    let mut records = Map::new();
    records.insert(
        KEY.to_owned(),
        json!({"data": [{"relative": "Mattermost", "backup": null}]}),
    );
    save(&rig.root, &records).unwrap();
    let log = RefCell::new(Vec::new());
    let record = |message: Message| log.borrow_mut().push(message.to_string());
    assert_eq!(relocate(&rig.root, &rig.host, &old, false, &record), 1);
    assert!(points_to(&host, &rig.source));
    assert!(rig.linked());
    assert_eq!(
        log.borrow()[0],
        format!(
            "Enlaces del host actualizados a la nueva ruta: {}",
            host.display()
        )
    );
    mito_win::links::remove_junction(&host).unwrap();
    mito_win::links::create_junction(&old_source, &host).unwrap();
    assert_eq!(relocate(&rig.root, &rig.host, &old, true, &record), 1);
    assert!(points_to(&host, &old_source));
    assert!(registry(&rig.root).unwrap().is_empty());
}

#[test]
fn el_plan_valida_la_carpeta_de_mito_y_el_destino() {
    let rig = rig();
    assert_eq!(
        message(plan(&rig.root, &rig.host, Kind::Data, "Falta")),
        format!(
            "Se esperaba una carpeta de datos en MITO: {}",
            rig.root.join(r"home\AppData\Roaming\Falta").display()
        )
    );
    assert!(plan(&rig.root, &rig.host, Kind::Data, "../x").is_err());
    let (source, host) = plan(&rig.root, &rig.host, Kind::Data, "Mattermost").unwrap();
    assert_eq!(source, rig.source);
    assert_eq!(host, rig.host_path());
    // Un componente de la ruta portable que es un junction no se acepta.
    let outside = rig.folder.join("outside");
    fs::create_dir_all(outside.join("Inner")).unwrap();
    mito_win::links::create_junction(&outside, &rig.root.join("home/AppData/Roaming/Out")).unwrap();
    assert!(plan(&rig.root, &rig.host, Kind::Data, "Out/Inner").is_err());
}

#[test]
fn solo_se_exponen_componentes_instalados() {
    let rig = rig();
    assert_eq!(
        message(link(
            &rig.engine,
            &rig.host,
            "main/other",
            Kind::Data,
            "Mattermost",
            false
        )),
        "El componente ya no está instalado"
    );
}

#[test]
fn quitar_la_exposicion_de_una_instalacion_eliminada() {
    let rig = rig();
    rig.link(false).unwrap();
    assert!(rig.linked());
    unregister_installation(&rig.root, &rig.host, KEY).unwrap();
    assert!(!rig.host_path().exists());
    assert!(!rig.linked());
    assert!(rig.source.is_dir());
    // Sin exposiciones, no hace nada.
    unregister_installation(&rig.root, &rig.host, KEY).unwrap();
}

#[test]
fn el_registro_tiene_el_formato_del_original() {
    let rig = rig();
    rig.link(false).unwrap();
    assert_eq!(
        fs::read_to_string(rig.root.join(REGISTRY)).unwrap(),
        "{\n  \"main/mattermost\": {\n    \"data\": [\n      {\n        \"relative\": \"Mattermost\",\n        \"backup\": null\n      }\n    ]\n  }\n}\n"
    );
}

#[test]
fn la_ruta_de_apartado_lleva_la_marca_de_tiempo() {
    assert_eq!(
        backup_path_at(Path::new(r"C:\h\Roaming\Mattermost"), "20261127-101500"),
        PathBuf::from(r"C:\h\Roaming\Mattermost.mito-backup-20261127-101500")
    );
    let name = backup_path(Path::new(r"C:\h\App")).display().to_string();
    assert!(name.starts_with(r"C:\h\App.mito-backup-"));
    assert_eq!(
        name.len(),
        r"C:\h\App.mito-backup-".len() + "20261127-101500".len()
    );
}

#[test]
fn normal_quita_prefijos_y_normaliza() {
    assert_eq!(
        normal(Path::new(r"\\?\C:\Users\Ana\.\x\..\Datos")),
        r"c:\users\ana\datos"
    );
    assert_eq!(normal(Path::new(r"\??\C:\A\B\")), r"c:\a\b");
    assert_eq!(normal(Path::new("C:/A//B")), r"c:\a\b");
}
