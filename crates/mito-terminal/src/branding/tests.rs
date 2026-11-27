//! Tests de `terminal_branding` (cobertura que faltaba en el original).

use std::cell::Cell;
use std::fs;
use std::path::Path;

use mito_core::Layout;

use super::*;

const REAL_ICON: &[u8] = include_bytes!("../../../../assets/branding/mito.ico");

fn ico(entries: &[(usize, usize)], total: usize) -> Vec<u8> {
    let mut bytes = vec![0, 0, 1, 0];
    bytes.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_le_bytes());
    for (size, offset) in entries {
        bytes.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0]);
        bytes.extend_from_slice(&u32::try_from(*size).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(*offset).unwrap().to_le_bytes());
    }
    bytes.resize(total, 9);
    bytes
}

fn message(bytes: &[u8]) -> String {
    ico_images(bytes).unwrap_err().to_string()
}

#[test]
fn el_ico_valido_da_una_imagen_por_entrada_con_sus_metadatos() {
    let images = ico_images(&ico(&[(4, 38), (6, 42)], 48)).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].header, [16, 16, 0, 0, 1, 0, 32, 0]);
    assert_eq!(images[0].data.len(), 4);
    assert_eq!(images[1].data.len(), 6);
    let real = ico_images(REAL_ICON).unwrap();
    assert_ne!(real.len(), 0);
}

#[test]
fn el_ico_invalido_se_rechaza_con_los_mensajes_del_original() {
    assert_eq!(message(b"abc"), "El ICO de MITO está truncado");
    assert_eq!(message(&[1, 0, 1, 0, 1, 0]), "El ICO de MITO no es válido");
    assert_eq!(message(&[0, 0, 2, 0, 1, 0]), "El ICO de MITO no es válido");
    assert_eq!(message(&[0, 0, 1, 0, 0, 0]), "El ICO de MITO no es válido");
    // Entrada declarada pero sin sitio en el archivo.
    assert_eq!(
        message(&[0, 0, 1, 0, 3, 0, 0, 0]),
        "El ICO de MITO no es válido"
    );
    let invalid = "El ICO de MITO contiene una imagen inválida";
    assert_eq!(message(&ico(&[(0, 22)], 30)), invalid);
    assert_eq!(message(&ico(&[(4, 10)], 30)), invalid);
    assert_eq!(message(&ico(&[(100, 22)], 30)), invalid);
}

/// Instancia con un Terminal y un icono de prueba.
fn instance(original: &[u8], icon: &[u8]) -> (tempfile::TempDir, Layout) {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    fs::create_dir_all(layout.windows_terminal()).unwrap();
    fs::create_dir_all(layout.branding_icon().parent().unwrap()).unwrap();
    fs::write(layout.windows_terminal_exe(), original).unwrap();
    fs::write(layout.branding_icon(), icon).unwrap();
    (dir, layout)
}

/// Sustituidor falso que escribe una marca en el ejecutable temporal.
fn fake(calls: &Cell<u32>) -> impl Fn(&Path, &[IconImage]) -> Result<(), IconError> + '_ {
    move |path, _| {
        calls.set(calls.get() + 1);
        let mut bytes = fs::read(path)?;
        bytes.extend_from_slice(b"+MITO");
        fs::write(path, bytes)?;
        Ok(())
    }
}

#[test]
fn falta_el_original_o_el_icono_devuelve_el_original_sin_hacer_nada() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let calls = Cell::new(0);
    assert_eq!(
        ensure_branded_with(&layout, &fake(&calls)).unwrap(),
        layout.windows_terminal_exe()
    );
    let (_dir, layout) = instance(b"WT", &ico(&[(4, 22)], 30));
    fs::remove_file(layout.branding_icon()).unwrap();
    assert_eq!(
        ensure_branded_with(&layout, &fake(&calls)).unwrap(),
        layout.windows_terminal_exe()
    );
    assert_eq!(calls.get(), 0);
    assert!(!layout.branded_terminal_exe().exists());
}

#[test]
fn deriva_una_vez_y_solo_reconstruye_si_cambia_el_original_o_el_icono() {
    let (_dir, layout) = instance(b"WT1", &ico(&[(4, 22)], 30));
    let calls = Cell::new(0);
    let branded = ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(branded, layout.branded_terminal_exe());
    assert_eq!(fs::read(&branded).unwrap(), b"WT1+MITO");
    assert_eq!(fs::read(layout.windows_terminal_exe()).unwrap(), b"WT1");
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(layout.windows_terminal().join(".mito-branding.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(marker["source_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(marker["icon_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(calls.get(), 1);

    ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(calls.get(), 1, "con el marcador igual no se reconstruye");

    fs::write(layout.windows_terminal_exe(), b"WT2").unwrap();
    ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(calls.get(), 2, "cambia el original");
    assert_eq!(fs::read(&branded).unwrap(), b"WT2+MITO");

    fs::write(layout.branding_icon(), ico(&[(5, 22)], 30)).unwrap();
    ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(calls.get(), 3, "cambia el icono");

    fs::remove_file(layout.windows_terminal().join(".mito-branding.json")).unwrap();
    ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(calls.get(), 4, "falta el marcador");
    fs::remove_file(&branded).unwrap();
    ensure_branded_with(&layout, &fake(&calls)).unwrap();
    assert_eq!(calls.get(), 5, "falta la copia");
}

#[test]
fn un_fallo_no_deja_copia_parcial_ni_marcador_ni_temporales() {
    let (_dir, layout) = instance(b"WT", &ico(&[(4, 22)], 30));
    let failing = |path: &Path, _: &[IconImage]| -> Result<(), IconError> {
        fs::write(path, b"a medias")?;
        Err(IconError::NoIconGroups)
    };
    let error = ensure_branded_with(&layout, &failing).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Windows Terminal no contiene recursos de icono"
    );
    assert!(!layout.branded_terminal_exe().exists());
    assert!(
        !layout
            .windows_terminal()
            .join(".mito-branding.json")
            .exists()
    );
    let leftovers: Vec<_> = fs::read_dir(layout.windows_terminal())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["WindowsTerminal.exe"]);
    assert_eq!(fs::read(layout.windows_terminal_exe()).unwrap(), b"WT");
}

#[test]
fn un_icono_invalido_falla_antes_de_copiar_nada() {
    let (_dir, layout) = instance(b"WT", b"no es un ico");
    let calls = Cell::new(0);
    assert!(ensure_branded_with(&layout, &fake(&calls)).is_err());
    assert_eq!(calls.get(), 0);
    assert!(!layout.branded_terminal_exe().exists());
}

#[test]
fn el_terminal_degradado_registra_un_aviso_y_sigue_con_el_original() {
    let (_dir, layout) = instance(b"no soy un PE", REAL_ICON);
    assert!(brand_or_warn(&layout).is_none());
    assert!(!layout.branded_terminal_exe().exists());
    let log = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
    assert!(log.contains("terminal.branding"), "{log}");
    assert!(log.contains("\"level\": \"warning\""), "{log}");
}

#[test]
fn brand_window_icons_escribe_mito_ico_si_falta_y_sincroniza_images() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    assert_eq!(brand_window_icons(&layout).unwrap(), None);
    fs::create_dir_all(layout.windows_terminal()).unwrap();
    fs::write(layout.windows_terminal_exe(), b"no soy un PE").unwrap();
    assert!(brand_window_icons(&layout).is_err());
    assert_eq!(fs::read(layout.branding_icon()).unwrap(), REAL_ICON);
    for name in IMAGE_ICONS {
        assert_eq!(
            fs::read(layout.windows_terminal().join("Images").join(name)).unwrap(),
            REAL_ICON
        );
    }
}

#[test]
fn con_un_ejecutable_real_la_copia_difiere_y_sigue_siendo_un_pe() {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let original = fs::read(Path::new(&system).join(r"System32\notepad.exe")).unwrap();
    let (_dir, layout) = instance(&original, REAL_ICON);
    let branded = ensure_branded_executable(&layout).unwrap();
    assert_eq!(branded, layout.branded_terminal_exe());
    let bytes = fs::read(&branded).unwrap();
    assert_ne!(bytes, original);
    assert_eq!(fs::read(layout.windows_terminal_exe()).unwrap(), original);
    assert_eq!(
        mito_win::resources::group_icons(&branded).unwrap(),
        mito_win::resources::group_icons(&layout.windows_terminal_exe()).unwrap()
    );
}
