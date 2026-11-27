//! Tests portados de `core/tests/test_terminal_identity.py`.

use std::fs;
use std::path::PathBuf;

use mito_core::Layout;

use super::*;

fn property(properties: &[(u32, String)], pid: u32) -> &str {
    &properties.iter().find(|(id, _)| *id == pid).unwrap().1
}

#[test]
fn la_identidad_es_por_instancia_y_solo_usa_el_icono_de_mito() {
    let properties = identity_properties(&Layout::new(r"C:\MITO"));
    let pids: Vec<u32> = properties.iter().map(|(pid, _)| *pid).collect();
    assert_eq!(pids, [2, 4, 3, 5]);
    assert_eq!(
        property(&properties, RELAUNCH_ICON),
        r"C:\MITO\core\assets\branding\mito.ico,0"
    );
    assert_eq!(property(&properties, RELAUNCH_DISPLAY_NAME), "MITO");
    assert_eq!(
        property(&properties, RELAUNCH_COMMAND),
        r"C:\MITO\core\bin\mito.exe start"
    );
    assert!(!property(&properties, RELAUNCH_COMMAND).contains("pocketool"));
    let id = |root: &str| property(&identity_properties(&Layout::new(root)), APP_ID).to_owned();
    assert!(id("C:/MITO").starts_with("MITO.Instance."));
    assert_eq!(id("C:/MITO").len(), "MITO.Instance.".len() + 24);
    assert_eq!(id("C:/MITO"), id("c:/mito"));
    assert_ne!(id("C:/MITO"), id("C:/Yito"));
    assert_ne!(id("C:/MITO"), id("C:/other-mito"));
}

#[test]
fn el_comando_de_relanzamiento_entrecomilla_las_rutas_con_espacios() {
    let properties = identity_properties(&Layout::new(r"C:\Mi carpeta\MITO"));
    assert_eq!(
        property(&properties, RELAUNCH_COMMAND),
        r#""C:\Mi carpeta\MITO\core\bin\mito.exe" start"#
    );
}

fn window(hwnd: isize, class: &str, image: Option<&str>) -> WindowInfo {
    WindowInfo {
        hwnd,
        class: class.to_owned(),
        image: image.map(PathBuf::from),
    }
}

#[test]
fn la_enumeracion_excluye_otras_instancias_y_ventanas_ajenas() {
    let layout = Layout::new(r"C:\MITO");
    let windows = [
        window(
            1,
            WINDOW_CLASS,
            Some(r"C:\MITO\core\tools\windows-terminal\YitoTerminal.exe"),
        ),
        window(
            2,
            WINDOW_CLASS,
            Some(r"C:\OTRO\core\tools\windows-terminal\WindowsTerminal.exe"),
        ),
        window(
            3,
            WINDOW_CLASS,
            Some(r"C:\Program Files\WindowsApps\WindowsTerminal.exe"),
        ),
        window(
            4,
            WINDOW_CLASS,
            Some(r"c:\mito\CORE\tools\windows-terminal\windowsterminal.EXE"),
        ),
        window(
            5,
            "Notepad",
            Some(r"C:\MITO\core\tools\windows-terminal\WindowsTerminal.exe"),
        ),
        window(6, WINDOW_CLASS, None),
    ];
    assert_eq!(select_windows(&layout, &windows), [1, 4]);
}

#[test]
fn sin_icono_no_se_toca_nada_y_el_resultado_es_cero() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    assert_eq!(apply(&layout).unwrap(), 0);
}

#[test]
fn apply_safely_registra_un_info_con_las_ventanas_aplicadas() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    fs::create_dir_all(layout.branding_icon().parent().unwrap()).unwrap();
    fs::write(layout.branding_icon(), b"ico").unwrap();
    apply_safely(&layout);
    let log = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
    assert!(log.contains("terminal.identity"), "{log}");
    assert!(log.contains("Taskbar identity applied"), "{log}");
    assert!(log.contains("\"windows\": 0"), "{log}");
}
