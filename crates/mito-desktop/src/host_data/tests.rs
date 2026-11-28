//! Pruebas de `host_data` (`test_host_data.py`), sobre carpetas temporales y junctions
//! reales: nunca se toca el `AppData` del usuario.

use std::cell::Cell;
use std::fs;
use std::io;

use super::*;
use crate::host_apps;

struct Rig {
    _tmp: tempfile::TempDir,
    folder: PathBuf,
    root: PathBuf,
    host: PathBuf,
    roots: HostRoots,
    exe: PathBuf,
    identity: String,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let folder = dunce::canonicalize(tmp.path()).unwrap();
    let root = folder.join("mito");
    let host = folder.join("host");
    fs::create_dir(&host).unwrap();
    let exe = host.join("Programs/Opera/opera.exe");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"fixture").unwrap();
    let identity = host_apps::add(&root, &exe).unwrap();
    Rig {
        roots: HostRoots::new(&host, &host),
        _tmp: tmp,
        folder,
        root,
        host,
        exe,
        identity,
    }
}

impl Rig {
    fn data(&self, name: &str) -> PathBuf {
        let folder = self.host.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("session"), "host data").unwrap();
        folder
    }

    fn link(&self, kind: Kind, folder: &Path, replace: bool) -> Result<()> {
        link(
            &self.root,
            &self.roots,
            &self.identity,
            kind,
            folder,
            replace,
        )
    }

    fn unlink(&self, kind: Kind) -> Result<()> {
        unlink(&self.root, &self.roots, &self.identity, kind)
    }

    fn linked(&self, kind: Kind) -> bool {
        linked(&self.root, &self.roots, &self.identity, kind).unwrap()
    }

    fn forget(&self) {
        // Mismo estado de disco que un `mklink /J` manual: el junction sin registro.
        save(&self.root, &Map::new()).unwrap();
    }
}

fn message(result: Result<impl std::fmt::Debug>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn un_junction_manual_se_detecta_sin_escribir_y_se_quita_con_seguridad() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    rig.forget();
    let before = fs::read(rig.root.join(REGISTRY)).unwrap();
    assert!(rig.linked(Kind::Data));
    let found = entries(&rig.root, &rig.roots, &rig.identity, Kind::Data).unwrap();
    assert_eq!(found[0].target, folder.display().to_string());
    assert_eq!(fs::read(rig.root.join(REGISTRY)).unwrap(), before);
    rig.unlink(Kind::Data).unwrap();
    assert!(!rig.linked(Kind::Data));
    assert_eq!(
        fs::read_to_string(folder.join("session")).unwrap(),
        "host data"
    );
}

#[test]
fn un_junction_manual_se_reutiliza_sin_crear_y_se_quita_con_la_aplicacion() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    rig.forget();
    let created = Cell::new(0);
    let counting = |target: &Path, link: &Path| {
        created.set(created.get() + 1);
        create_real(target, link)
    };
    for _ in 0..2 {
        link_with(
            &rig.root,
            &rig.roots,
            &rig.identity,
            Kind::Data,
            &folder,
            false,
            &counting,
        )
        .unwrap();
    }
    assert_eq!(created.get(), 0);
    rig.forget();
    host_apps::remove(&rig.root, &rig.roots, &rig.identity).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("session")).unwrap(),
        "host data"
    );
}

#[test]
fn un_junction_ajeno_no_se_reconoce_ni_se_reemplaza() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    rig.forget();
    // Mismo nombre portable, pero la carpeta del host esperada está ahora en otro sitio.
    let elsewhere = rig.folder.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let moved = HostRoots::new(&elsewhere, &elsewhere);
    assert!(!linked(&rig.root, &moved, &rig.identity, Kind::Data).unwrap());
    let candidate = elsewhere.join(folder.file_name().unwrap());
    fs::create_dir(&candidate).unwrap();
    assert!(
        link(
            &rig.root,
            &moved,
            &rig.identity,
            Kind::Data,
            &candidate,
            true
        )
        .is_err(),
        "no se reemplaza un enlace ajeno"
    );
    rig.unlink(Kind::Data).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("session")).unwrap(),
        "host data"
    );
}

#[test]
fn el_selector_empieza_en_el_appdata_de_la_categoria() {
    let rig = rig();
    let script = native::choose_folder_script(&|text| text.to_owned());
    assert!(script.contains("$dialog.SelectedPath = [Console]::In.ReadToEnd()"));
    assert_eq!(rig.roots.folder(Kind::Cache), rig.host);
    assert_eq!(Kind::Cache.folder(), "Local");
    assert_eq!(Kind::Data.folder(), "Roaming");
}

#[test]
fn la_busqueda_conserva_las_coincidencias_externas_y_excluye_el_arbol_del_ejecutable() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    fs::create_dir_all(folder.join("Opera Stable/Opera")).unwrap();
    let nested = rig.host.join("Vendor/OPERA Data");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        candidates(&rig.root, &rig.roots, &rig.identity, Kind::Data).unwrap(),
        [folder, nested]
    );
}

#[test]
fn los_junctions_reales_se_quitan_y_quitar_la_aplicacion_conserva_el_host() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    for kind in Kind::ALL {
        rig.link(kind, &folder, false).unwrap();
        assert!(rig.linked(kind));
        let (path, _, _) = plan(&rig.root, &rig.roots, kind, &folder).unwrap();
        assert!(mito_win::links::is_junction(&path));
        assert_eq!(
            fs::read_to_string(path.join("session")).unwrap(),
            "host data"
        );
    }
    rig.unlink(Kind::Data).unwrap();
    assert!(!rig.linked(Kind::Data));
    host_apps::remove(&rig.root, &rig.roots, &rig.identity).unwrap();
    assert!(!rig.linked(Kind::Cache));
    assert_eq!(
        fs::read_to_string(folder.join("session")).unwrap(),
        "host data"
    );
    assert!(rig.exe.is_file());
}

#[test]
fn reemplazar_exige_confirmacion_y_un_fallo_restaura_los_datos_locales() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    let (path, _, _) = plan(&rig.root, &rig.roots, Kind::Data, &folder).unwrap();
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("local"), "portable").unwrap();
    let error = message(rig.link(Kind::Data, &folder, false));
    assert_eq!(
        error,
        format!(
            "La carpeta {} ya existe y ha de ser borrada para generar el enlace",
            path.display()
        )
    );
    let failing = |_: &Path, _: &Path| Err(io::Error::other("junction"));
    let result = link_with(
        &rig.root,
        &rig.roots,
        &rig.identity,
        Kind::Data,
        &folder,
        true,
        &failing,
    );
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(path.join("local")).unwrap(), "portable");
    assert!(!mito_win::links::is_junction(&path));
    rig.link(Kind::Data, &folder, true).unwrap();
    assert!(!path.join("local").exists());
    // El staging no deja restos.
    let staging = rig.root.join("core/staging");
    assert!(!staging.exists() || fs::read_dir(staging).unwrap().next().is_none());
    host_apps::remove(&rig.root, &rig.roots, &rig.identity).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("session")).unwrap(),
        "host data"
    );
}

#[test]
fn una_carpeta_fuera_del_appdata_y_una_aplicacion_en_conflicto() {
    let rig = rig();
    let folder = rig.folder.join("Manual data");
    fs::create_dir(&folder).unwrap();
    let (path, _, relative) = plan(&rig.root, &rig.roots, Kind::Data, &folder).unwrap();
    assert_eq!(relative, "Manual data");
    rig.link(Kind::Data, &folder, false).unwrap();
    let exe = rig.host.join("other.exe");
    fs::write(&exe, b"fixture").unwrap();
    let other = host_apps::add(&rig.root, &exe).unwrap();
    assert_eq!(
        message(link(
            &rig.root,
            &rig.roots,
            &other,
            Kind::Data,
            &folder,
            true
        )),
        "Esta ruta ya está gestionada por una aplicación del host"
    );
    host_apps::remove(&rig.root, &rig.roots, &other).unwrap();
    assert!(mito_win::links::is_junction(&path));
    host_apps::remove(&rig.root, &rig.roots, &rig.identity).unwrap();
    assert!(folder.is_dir());
}

#[test]
fn se_rechazan_raices_rutas_internas_y_recorridos() {
    let rig = rig();
    for folder in [&rig.host, &rig.root] {
        fs::create_dir_all(folder).unwrap();
        assert!(plan(&rig.root, &rig.roots, Kind::Data, folder).is_err());
    }
    assert_eq!(
        message(plan(&rig.root, &rig.roots, Kind::Data, &rig.host)),
        "Selecciona una carpeta de aplicación, no todo AppData"
    );
    assert_eq!(
        message(plan(&rig.root, &rig.roots, Kind::Data, &rig.root)),
        "Selecciona datos externos a MITO, sin enlaces circulares"
    );
    assert_eq!(
        message(plan(
            &rig.root,
            &rig.roots,
            Kind::Data,
            Path::new("relativa")
        )),
        "Selecciona una carpeta existente"
    );
    assert!(destination(&rig.root, Kind::Data, "../outside").is_err());
    assert_eq!(
        message(destination(&rig.root, Kind::Data, "")),
        "Destino fuera del AppData de MITO"
    );
}

#[test]
fn un_junction_cambiado_no_se_quita() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    let (path, _, _) = plan(&rig.root, &rig.roots, Kind::Data, &folder).unwrap();
    mito_win::links::remove_junction(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("foreign"), "keep").unwrap();
    let error = message(host_apps::remove(&rig.root, &rig.roots, &rig.identity));
    assert_eq!(
        error,
        format!("Se esperaba una junction gestionada: {}", path.display())
    );
    assert_eq!(fs::read_to_string(path.join("foreign")).unwrap(), "keep");
    assert!(
        host_apps::inventory(&rig.root)
            .unwrap()
            .iter()
            .any(|(id, _)| *id == rig.identity)
    );
}

#[test]
fn un_junction_redirigido_falla_con_el_mensaje_de_enlace_cambiado() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    let (path, _, _) = plan(&rig.root, &rig.roots, Kind::Data, &folder).unwrap();
    let other = rig.folder.join("other");
    fs::create_dir(&other).unwrap();
    mito_win::links::remove_junction(&path).unwrap();
    mito_win::links::create_junction(&other, &path).unwrap();
    assert_eq!(
        message(rig.unlink(Kind::Data)),
        format!("El enlace ha cambiado fuera de MITO: {}", path.display())
    );
    assert!(mito_win::links::is_junction(&path));
    assert!(other.is_dir());
}

#[test]
fn el_registro_tiene_el_formato_del_original() {
    let rig = rig();
    let folder = rig.data("Opera Software");
    rig.link(Kind::Data, &folder, false).unwrap();
    let text = fs::read_to_string(rig.root.join(REGISTRY)).unwrap();
    let expected = format!(
        "{{\n  \"{}\": {{\n    \"data\": [\n      {{\n        \"relative\": \"Opera Software\",\n        \"target\": {}\n      }}\n    ]\n  }}\n}}\n",
        rig.identity,
        serde_json::to_string(&folder.display().to_string()).unwrap()
    );
    assert_eq!(text, expected);
}

#[test]
fn las_comparaciones_de_rutas_ignoran_mayusculas() {
    assert!(same_path(
        Path::new(r"C:\Datos\App"),
        Path::new(r"c:\datos\APP")
    ));
    assert!(is_relative_to(
        Path::new(r"C:\Datos\App\x"),
        Path::new(r"c:\DATOS")
    ));
    assert!(!is_relative_to(
        Path::new(r"C:\Datos2"),
        Path::new(r"C:\Datos")
    ));
    assert_eq!(
        relative_to(Path::new(r"C:\Datos\App\Sub"), Path::new(r"c:\datos")),
        Some(PathBuf::from(r"App\Sub"))
    );
}
