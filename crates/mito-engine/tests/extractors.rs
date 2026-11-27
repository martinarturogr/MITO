//! Instalación con los extractores de MSI e Inno Setup (F7): el motor construye las
//! opciones de `archives::extract`, aplica la raíz de Inno y restaura los nombres
//! declarados.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{ZERO_HASH, error_text, fx, fx_with, zip_of};
use mito_win::msi::DatabaseBuilder;
use serde_json::json;

fn inno_manifest(url: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut manifest = json!({"version": "1", "url": url, "hash": ZERO_HASH,
                              "innosetup": true, "bin": "Editor.exe"});
    for (key, value) in extra.as_object().unwrap() {
        manifest[key] = value.clone();
    }
    manifest
}

#[test]
fn un_exe_con_innosetup_sin_innounp_no_instala_nada() {
    let f = fx();
    for extract_dir in [None, Some("{app}"), Some("{code_GetDestDir}")] {
        let extra = extract_dir.map_or_else(|| json!({}), |dir| json!({"extract_dir": dir}));
        f.catalog(json!({"editor": inno_manifest("https://example.org/setup.exe", extra)}));
        f.serve("https://example.org/setup.exe", b"MZ installer".to_vec());
        let error = error_text(f.install(&["main/editor"], None));
        assert_eq!(error, "Herramienta incompleta: innounp", "{extract_dir:?}");
        assert!(!f.path("core/apps/main/editor").exists());
        assert!(f.staging_is_empty());
    }
}

#[test]
fn una_constante_de_inno_en_extract_dir_elige_la_raiz_y_no_una_subcarpeta() {
    let f = fx();
    f.catalog(json!({"editor": inno_manifest(
        "https://example.org/editor.zip", json!({"extract_dir": "{code_GetDestDir}"}))}));
    f.serve(
        "https://example.org/editor.zip",
        zip_of(&[("Editor.exe", b"exe")]),
    );
    f.install(&["main/editor"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/editor/Editor.exe")).unwrap(),
        b"exe"
    );
}

#[test]
fn los_nombres_declarados_se_restauran_si_las_copias_numeradas_son_identicas() {
    let f = fx();
    f.catalog(json!({"editor": inno_manifest("https://example.org/editor.zip", json!({}))}));
    f.serve(
        "https://example.org/editor.zip",
        zip_of(&[("Editor,1.exe", b"same"), ("Editor,2.exe", b"same")]),
    );
    f.install(&["main/editor"], None).unwrap();
    let app = f.path("core/apps/main/editor");
    assert_eq!(fs::read(app.join("Editor.exe")).unwrap(), b"same");
    assert!(app.join("Editor,1.exe").is_file());
}

#[test]
fn las_variantes_distintas_son_ambiguas_y_no_se_publica_nada() {
    let f = fx();
    f.catalog(json!({"editor": inno_manifest("https://example.org/editor.zip", json!({}))}));
    f.serve(
        "https://example.org/editor.zip",
        zip_of(&[("Editor,1.exe", b"one"), ("Editor,2.exe", b"two")]),
    );
    let error = error_text(f.install(&["main/editor"], None));
    assert_eq!(error, "Inno: ambiguous variants for Editor.exe");
    assert!(!f.path("core/apps/main/editor").exists());
    assert!(f.staging_is_empty());
}

#[test]
fn los_atajos_tambien_restauran_su_nombre() {
    let f = fx();
    f.catalog(json!({"editor": inno_manifest(
        "https://example.org/editor.zip",
        json!({"bin": "Other.exe", "shortcuts": [["Editor.exe", "Editor"]]}))}));
    f.serve(
        "https://example.org/editor.zip",
        zip_of(&[
            ("Editor,1.exe", b"same"),
            ("Editor,2.exe", b"same"),
            ("Other.exe", b"o"),
        ]),
    );
    f.install(&["main/editor"], None).unwrap();
    assert!(f.path("core/apps/main/editor/Editor.exe").is_file());
}

fn real_7zip() -> Option<PathBuf> {
    [
        std::env::var_os("MITO_7ZIP").map(PathBuf::from),
        Some(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe")),
        Some(PathBuf::from(r"C:\Program Files (x86)\7-Zip\7z.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// MSI real con un CAB de `makecab` y el archivo `payload` (`tool.exe` en `bin`).
fn msi_bytes(folder: &Path, content: &[u8]) -> Vec<u8> {
    let source = folder.join("payload");
    fs::write(&source, content).unwrap();
    let cabinet = folder.join("fixture.cab");
    let made = Command::new(r"C:\Windows\System32\makecab.exe")
        .args(["/D", "CompressionType=MSZIP"])
        .arg(&source)
        .arg(&cabinet)
        .output()
        .unwrap();
    assert!(made.status.success());
    let path = folder.join("fixture.msi");
    let builder = DatabaseBuilder::create(&path).unwrap();
    for sql in [
        "CREATE TABLE `Directory` (`Directory` CHAR(72) NOT NULL, `Directory_Parent` CHAR(72), `DefaultDir` CHAR(255) NOT NULL PRIMARY KEY `Directory`)",
        "CREATE TABLE `Component` (`Component` CHAR(72) NOT NULL, `Directory_` CHAR(72) NOT NULL PRIMARY KEY `Component`)",
        "CREATE TABLE `Media` (`DiskId` SHORT NOT NULL, `LastSequence` SHORT NOT NULL, `Cabinet` CHAR(255) PRIMARY KEY `DiskId`)",
        "CREATE TABLE `File` (`File` CHAR(72) NOT NULL, `Component_` CHAR(72) NOT NULL, `FileName` CHAR(255) NOT NULL, `FileSize` LONG NOT NULL, `Attributes` SHORT, `Sequence` SHORT NOT NULL PRIMARY KEY `File`)",
        "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('TARGETDIR', '', 'SourceDir')",
        "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('APP', 'TARGETDIR', 'TARGET:SHORT|Package')",
        "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('BIN', 'APP', 'bin')",
        "INSERT INTO `Component` (`Component`, `Directory_`) VALUES ('comp', 'BIN')",
        "INSERT INTO `Media` (`DiskId`, `LastSequence`, `Cabinet`) VALUES (1, 1, '#cab')",
    ] {
        builder.execute(sql, &[]).unwrap();
    }
    builder
        .execute(
            &format!("INSERT INTO `File` (`File`, `Component_`, `FileName`, `FileSize`, `Attributes`, `Sequence`) VALUES ('payload', 'comp', 'TOOL.EXE|tool.exe', {}, 16384, 1)", content.len()),
            &[],
        )
        .unwrap();
    builder.add_stream("cab", &cabinet).unwrap();
    builder.commit().unwrap();
    fs::read(path).unwrap()
}

#[test]
fn un_msi_se_extrae_con_el_7zip_privado_y_publica_su_arbol() {
    let Some(sevenzip) = real_7zip() else {
        eprintln!("se omite: no hay 7-Zip (define MITO_7ZIP)");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let f = fx_with(|e| e.sevenzip = Some(sevenzip));
    f.catalog(
        json!({"tool": {"version": "1", "url": "https://example.org/tool.msi",
                              "hash": ZERO_HASH, "bin": "Package/bin/tool.exe"}}),
    );
    f.serve(
        "https://example.org/tool.msi",
        msi_bytes(folder.path(), b"fixture"),
    );
    f.install(&["main/tool"], None).unwrap();
    assert_eq!(
        fs::read(f.path("core/apps/main/tool/Package/bin/tool.exe")).unwrap(),
        b"fixture"
    );
    assert!(f.staging_is_empty());
    // Sin 7-Zip el MSI falla y no queda nada.
    let f = fx();
    f.catalog(
        json!({"tool": {"version": "1", "url": "https://example.org/tool.msi",
                              "hash": ZERO_HASH, "bin": "Package/bin/tool.exe"}}),
    );
    f.serve(
        "https://example.org/tool.msi",
        msi_bytes(folder.path(), b"fixture"),
    );
    let error = error_text(f.install(&["main/tool"], None));
    assert_eq!(error, "Falta el extractor privado 7-Zip; ejecuta bootstrap");
    assert!(!f.path("core/apps/main/tool").exists());
    assert!(f.staging_is_empty());
}
