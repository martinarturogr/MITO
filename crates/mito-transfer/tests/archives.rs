//! Tests de extracción: los de `archives.py` del original (`test_activation.py`,
//! `test_engine.py`) más fixtures maliciosos generados aquí mismo.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use mito_transfer::Error;
use mito_transfer::archives::{
    ExtractOptions, Extraction, Limits, extract, extract_tar_with, extract_zip, extract_zip_with,
};
use proptest::prelude::*;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

/// Construye un ZIP en memoria con `(nombre, contenido)`.
fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, options()).unwrap();
        } else {
            writer.start_file(*name, options()).unwrap();
            writer.write_all(data).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn write_zip(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, zip_bytes(entries)).unwrap();
    path
}

fn tree(root: &Path) -> Vec<String> {
    fn walk(base: &Path, folder: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if path.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn message(result: Result<impl std::fmt::Debug, Error>) -> String {
    result.unwrap_err().to_string()
}

// --- ZIP ----------------------------------------------------------------------------

#[test]
fn un_zip_con_traversal_se_rechaza_antes_de_extraer_nada() {
    // test_zip_traversal_rejected_before_any_extraction
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(
        dir.path(),
        "bad.zip",
        &[("good.txt", b"ok"), ("../escape", b"x")],
    );
    let out = dir.path().join("out");
    assert_eq!(
        message(extract_zip(&archive, &out)),
        "Ruta no portable: ../escape"
    );
    assert!(!out.join("good.txt").exists());
    assert!(!out.exists());
    assert!(!dir.path().join("escape").exists());
}

#[test]
fn un_zip_valido_se_extrae_con_carpetas_y_archivos() {
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(
        dir.path(),
        "good.zip",
        &[
            ("folder/", b""),
            ("folder/file.txt", b"test"),
            ("empty/", b""),
            ("top.bin", &[0, 1, 2]),
            ("a\\b.txt", b"backslash"),
        ],
    );
    let out = dir.path().join("out");
    extract_zip(&archive, &out).unwrap();
    assert_eq!(fs::read(out.join("folder/file.txt")).unwrap(), b"test");
    assert_eq!(fs::read(out.join("top.bin")).unwrap(), [0, 1, 2]);
    assert!(out.join("empty").is_dir());
    assert_eq!(fs::read(out.join("a/b.txt")).unwrap(), b"backslash");
}

#[test]
fn rutas_peligrosas_en_un_zip() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [(&str, &str); 7] = [
        ("/abs.txt", "Ruta no portable: /abs.txt"),
        ("a/../../x", "Ruta no portable: a/../../x"),
        ("C:\\evil.txt", "Ruta no portable: C:\\evil.txt"),
        ("file.txt:stream", "Ruta no portable: file.txt:stream"),
        ("dir/NUL", "Ruta reservada: dir/NUL"),
        ("con.txt", "Ruta reservada: con.txt"),
        ("bad?.txt", "Ruta reservada: bad?.txt"),
    ];
    for (name, expected) in cases {
        let archive = write_zip(dir.path(), "bad.zip", &[("ok.txt", b"ok"), (name, b"x")]);
        let out = dir.path().join("out");
        assert_eq!(message(extract_zip(&archive, &out)), expected, "{name}");
        assert!(!out.exists(), "{name}");
    }
}

#[test]
fn duplicados_que_solo_difieren_en_mayusculas() {
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(
        dir.path(),
        "dup.zip",
        &[("Readme.txt", b"a"), ("README.TXT", b"b")],
    );
    let out = dir.path().join("out");
    assert_eq!(
        message(extract_zip(&archive, &out)),
        "Entrada duplicada: README.TXT"
    );
    assert!(!out.exists());
}

#[test]
fn un_enlace_simbolico_en_un_zip_se_rechaza() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("link.zip");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("ok.txt", options()).unwrap();
    writer.write_all(b"ok").unwrap();
    writer
        .add_symlink("link", "../outside", SimpleFileOptions::default())
        .unwrap();
    fs::write(&archive, writer.finish().unwrap().into_inner()).unwrap();
    let out = dir.path().join("out");
    assert_eq!(
        message(extract_zip(&archive, &out)),
        "Los enlaces del archivo no están permitidos"
    );
    assert!(!out.exists());
}

#[test]
fn limites_de_entradas_y_de_tamano_declarado_en_un_zip() {
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(
        dir.path(),
        "a.zip",
        &[("a", b"1234"), ("b", b"1234"), ("c", b"1234")],
    );
    let out = dir.path().join("out");
    let too_many = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    assert_eq!(
        message(extract_zip_with(&archive, &out, &too_many)),
        "Archivo demasiado grande para extraer"
    );
    let too_big = Limits {
        max_expanded: 11,
        ..Limits::default()
    };
    assert_eq!(
        message(extract_zip_with(&archive, &out, &too_big)),
        "Archivo demasiado grande para extraer"
    );
    assert!(!out.exists());
    let exact = Limits {
        max_expanded: 12,
        max_entries: 3,
    };
    extract_zip_with(&archive, &out, &exact).unwrap();
}

/// Un ZIP comprimido de `size` ceros cuyo tamaño declarado se reescribe a `declared`.
fn bomb(dir: &Path, size: usize, declared: u32) -> PathBuf {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "bomb.bin",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0_u8; size]).unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    let declared = declared.to_le_bytes();
    // Cabecera local (tamaño sin comprimir en +22) y de directorio central (en +24).
    for (signature, offset) in [(b"PK\x03\x04", 22), (b"PK\x01\x02", 24)] {
        let at = bytes
            .windows(4)
            .position(|w| w == signature)
            .expect("firma de cabecera");
        bytes[at + offset..at + offset + 4].copy_from_slice(&declared);
    }
    let path = dir.join("bomb.zip");
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn una_bomba_que_declara_poco_y_expande_mucho_se_corta_en_los_bytes_reales() {
    let dir = tempfile::tempdir().unwrap();
    let archive = bomb(dir.path(), 4 * 1024 * 1024, 16);
    let out = dir.path().join("out");
    let limits = Limits {
        max_expanded: 1024 * 1024,
        ..Limits::default()
    };
    let result = extract_zip_with(&archive, &out, &limits);
    // `zip` ya rechaza un contenido mayor que el tamaño declarado; si no lo hiciera, o
    // el ZIP usara descriptores de datos, el límite de `Sink` lo cortaría (ver sink.rs).
    assert!(result.is_err(), "se esperaba un error");
    assert!(!out.exists(), "{:?}", tree(dir.path()));
}

#[test]
fn una_bomba_con_tamano_honesto_se_rechaza_por_el_tamano_declarado() {
    let dir = tempfile::tempdir().unwrap();
    let archive = bomb(dir.path(), 4 * 1024 * 1024, 4 * 1024 * 1024);
    let out = dir.path().join("out");
    let limits = Limits {
        max_expanded: 1024 * 1024,
        ..Limits::default()
    };
    assert_eq!(
        message(extract_zip_with(&archive, &out, &limits)),
        "Archivo demasiado grande para extraer"
    );
    assert!(!out.exists());
}

/// Cambia el método de compresión de un ZIP de un solo archivo a `method`.
fn patch_method(bytes: &mut [u8], method: u16) {
    for (signature, offset) in [(b"PK\x03\x04", 8), (b"PK\x01\x02", 10)] {
        let at = bytes.windows(4).position(|w| w == signature).unwrap();
        bytes[at + offset..at + offset + 2].copy_from_slice(&method.to_le_bytes());
    }
}

#[test]
fn un_metodo_no_soportado_se_senala_despues_de_validar_y_sin_escribir() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = zip_bytes(&[("fixture.txt", b"data")]);
    patch_method(&mut bytes, 99);
    let archive = dir.path().join("odd.zip");
    fs::write(&archive, &bytes).unwrap();
    let out = dir.path().join("out");
    let error = extract_zip(&archive, &out).unwrap_err();
    assert!(matches!(error, Error::UnsupportedZipMethod), "{error:?}");
    assert!(!out.exists());

    // `extract` delega en 7-Zip; sin 7-Zip lo dice y no escribe nada.
    let error = extract(&archive, &out, "odd.zip", &ExtractOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Falta el extractor privado 7-Zip; ejecuta bootstrap"
    );
    assert!(!out.exists());

    // test_zip_fallback_never_bypasses_path_validation: con una ruta insegura el error
    // es el de la ruta, y 7-Zip ni se considera.
    let mut bad = zip_bytes(&[("../escape.txt", b"bad")]);
    patch_method(&mut bad, 99);
    fs::write(&archive, &bad).unwrap();
    let error = extract(&archive, &out, "bad.zip", &ExtractOptions::default()).unwrap_err();
    assert_eq!(error.to_string(), "Ruta no portable: ../escape.txt");
    assert!(!out.exists());
}

// --- TAR ----------------------------------------------------------------------------

fn tar_header(name: &str, kind: tar::EntryType, link: Option<&str>, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    // Se escribe el nombre sin pasar por la validación de `set_path`, que rechaza `..`.
    let raw = header.as_old_mut();
    raw.name[..name.len()].copy_from_slice(name.as_bytes());
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(0o644);
    if let Some(link) = link {
        header.set_link_name(link).unwrap();
    }
    header.set_cksum();
    header
}

fn tar_bytes(members: &[(&str, tar::EntryType, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, kind, link, data) in members {
        let header = tar_header(name, *kind, *link, data.len() as u64);
        builder.append(&header, *data).unwrap();
    }
    builder.into_inner().unwrap()
}

fn regular(
    name: &'static str,
    data: &'static [u8],
) -> (
    &'static str,
    tar::EntryType,
    Option<&'static str>,
    &'static [u8],
) {
    (name, tar::EntryType::Regular, None, data)
}

#[test]
fn un_tar_con_enlaces_simbolicos_se_rechaza() {
    // test_tar_rejects_symlinks
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("bad.tar");
    fs::write(
        &archive,
        tar_bytes(&[("link", tar::EntryType::Symlink, Some("../outside"), b"")]),
    )
    .unwrap();
    let out = dir.path().join("out");
    let error = extract(&archive, &out, "bad.tar", &ExtractOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "TAR con enlaces, dispositivos o entradas duplicadas"
    );
    assert!(!out.exists());
}

#[test]
fn un_tar_gz_se_extrae_y_un_exe_suelto_se_copia() {
    // test_tar_and_standalone_file
    let dir = tempfile::tempdir().unwrap();
    let raw = tar_bytes(&[regular("folder/file.txt", b"test")]);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&raw).unwrap();
    let archive = dir.path().join("good.tar.gz");
    fs::write(&archive, encoder.finish().unwrap()).unwrap();
    let out = dir.path().join("out");
    let result = extract(&archive, &out, "good.tar.gz", &ExtractOptions::default()).unwrap();
    assert_eq!(result, Extraction::Extracted);
    assert_eq!(fs::read(out.join("folder/file.txt")).unwrap(), b"test");

    let exe = dir.path().join("download");
    fs::write(&exe, b"MZ fixture").unwrap();
    let result = extract(
        &exe,
        &dir.path().join("exe"),
        "tool.exe",
        &ExtractOptions::default(),
    );
    assert_eq!(result.unwrap(), Extraction::Loose);
    assert_eq!(
        fs::read(dir.path().join("exe/tool.exe")).unwrap(),
        b"MZ fixture"
    );
}

#[test]
fn tar_xz_y_tar_bz2_se_detectan_por_contenido() {
    let dir = tempfile::tempdir().unwrap();
    let raw = tar_bytes(&[regular("x/y.txt", b"payload")]);

    let mut xz = liblzma::write::XzEncoder::new(Vec::new(), 1);
    xz.write_all(&raw).unwrap();
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    bz.write_all(&raw).unwrap();
    // Los nombres no tienen que coincidir con la compresión real.
    for (bytes, name) in [
        (xz.finish().unwrap(), "pkg.tar.xz"),
        (bz.finish().unwrap(), "download"),
    ] {
        let archive = dir.path().join(name);
        fs::write(&archive, bytes).unwrap();
        let out = dir.path().join(format!("out-{name}"));
        let result = extract(&archive, &out, name, &ExtractOptions::default()).unwrap();
        assert_eq!(result, Extraction::Extracted, "{name}");
        assert_eq!(fs::read(out.join("x/y.txt")).unwrap(), b"payload");
    }
}

#[test]
fn rutas_y_tipos_peligrosos_en_un_tar() {
    let dir = tempfile::tempdir().unwrap();
    let cases: Vec<(&str, Vec<_>, &str)> = vec![
        (
            "traversal",
            vec![regular("good.txt", b"ok"), regular("../escape", b"x")],
            "Ruta no portable: ../escape",
        ),
        (
            "ads",
            vec![regular("a.txt:stream", b"x")],
            "Ruta no portable: a.txt:stream",
        ),
        (
            "duplicado",
            vec![regular("A.txt", b"1"), regular("a.TXT", b"2")],
            "TAR con enlaces, dispositivos o entradas duplicadas",
        ),
        (
            "hardlink",
            vec![
                regular("a.txt", b"1"),
                ("b", tar::EntryType::Link, Some("a.txt"), b""),
            ],
            "TAR con enlaces, dispositivos o entradas duplicadas",
        ),
        (
            "dispositivo",
            vec![("dev", tar::EntryType::Char, None, b"")],
            "TAR con enlaces, dispositivos o entradas duplicadas",
        ),
    ];
    for (label, members, expected) in cases {
        let archive = dir.path().join("t.tar");
        fs::write(&archive, tar_bytes(&members)).unwrap();
        let out = dir.path().join("out");
        let error = extract(&archive, &out, "t.tar", &ExtractOptions::default()).unwrap_err();
        assert_eq!(error.to_string(), expected, "{label}");
        assert!(!out.exists(), "{label}");
    }
}

#[test]
fn limites_de_un_tar() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("t.tar");
    fs::write(
        &archive,
        tar_bytes(&[
            regular("a", b"1234"),
            regular("b", b"1234"),
            regular("c", b"1234"),
        ]),
    )
    .unwrap();
    let out = dir.path().join("out");
    for limits in [
        Limits {
            max_entries: 2,
            ..Limits::default()
        },
        Limits {
            max_expanded: 11,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            message(extract_tar_with(&archive, &out, &limits)),
            "TAR demasiado grande"
        );
        assert!(!out.exists());
    }
}

// --- extract: orden de decisión --------------------------------------------------------

#[test]
fn un_jar_se_conserva_salvo_que_el_alias_pida_extraer() {
    // test_jar_is_preserved_unless_url_alias_requests_zip_extraction
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(dir.path(), "download", &[("Main.class", b"fixture")]);
    let jar = dir.path().join("jar");
    let result = extract(&archive, &jar, "Tool.JAR", &ExtractOptions::default()).unwrap();
    assert_eq!(result, Extraction::Loose);
    assert_eq!(
        fs::read(jar.join("Tool.JAR")).unwrap(),
        fs::read(&archive).unwrap()
    );
    let zip = dir.path().join("zip");
    let result = extract(&archive, &zip, "tool.zip", &ExtractOptions::default()).unwrap();
    assert_eq!(result, Extraction::Extracted);
    assert_eq!(fs::read(zip.join("Main.class")).unwrap(), b"fixture");
    // Un .ico también se copia íntegro.
    let ico = dir.path().join("ico");
    extract(&archive, &ico, "icon.ICO", &ExtractOptions::default()).unwrap();
    assert!(ico.join("icon.ICO").is_file());
}

#[test]
fn el_contenido_zip_gana_al_sufijo() {
    // Duda 5 del contrato: un .exe cuyo contenido es ZIP se extrae.
    let dir = tempfile::tempdir().unwrap();
    let archive = write_zip(dir.path(), "download", &[("inside.txt", b"x")]);
    let out = dir.path().join("out");
    let result = extract(&archive, &out, "setup.exe", &ExtractOptions::default()).unwrap();
    assert_eq!(result, Extraction::Extracted);
    assert!(out.join("inside.txt").is_file());
}

#[test]
fn los_formatos_7z_y_rar_se_delegan_y_sin_7zip_falla_con_claridad() {
    let dir = tempfile::tempdir().unwrap();
    let missing = "Falta el extractor privado 7-Zip; ejecuta bootstrap";
    // Por sufijo.
    let archive = dir.path().join("download");
    fs::write(&archive, b"not really an archive").unwrap();
    for name in ["a.7z", "a.RAR"] {
        let out = dir.path().join("out");
        let error = extract(&archive, &out, name, &ExtractOptions::default()).unwrap_err();
        assert_eq!(error.to_string(), missing, "{name}");
        assert!(!out.exists());
    }
    // Por firma, solo si no hay sufijo.
    for signature in [&b"7z\xBC\xAF\x27\x1C\x00\x04"[..], &b"Rar!\x1A\x07\x00"[..]] {
        fs::write(&archive, signature).unwrap();
        let error = extract(
            &archive,
            &dir.path().join("sig"),
            "download",
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), missing);
        // Con sufijo `.exe` y firma de 7z, se copia como archivo suelto.
        let loose = extract(
            &archive,
            &dir.path().join("exe"),
            "x.exe",
            &ExtractOptions::default(),
        );
        assert_eq!(loose.unwrap(), Extraction::Loose);
    }
}

#[test]
fn archivos_sueltos_y_formatos_no_soportados() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("download");
    fs::write(&archive, b"plain").unwrap();
    for name in ["tool", "a.exe", "b.DLL", "c.ps1", "d.cmd", "e.bat", "f.com"] {
        let out = dir.path().join(format!("out-{name}"));
        let result = extract(&archive, &out, name, &ExtractOptions::default()).unwrap();
        assert_eq!(result, Extraction::Loose, "{name}");
        assert_eq!(fs::read(out.join(name)).unwrap(), b"plain");
    }
    let out = dir.path().join("none");
    let error = extract(&archive, &out, "notes.txt", &ExtractOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Formato de archivo todavía no soportado: notes.txt"
    );
    assert!(!out.exists());
    // El nombre local también pasa por `inside`.
    let error = extract(&archive, &out, "../x.exe", &ExtractOptions::default()).unwrap_err();
    assert_eq!(error.to_string(), "Ruta no portable: ../x.exe");
}

#[test]
fn msi_e_inno_son_puntos_de_enganche_de_la_fase_f7() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("download");
    fs::write(&archive, b"MZ").unwrap();
    let out = dir.path().join("out");
    // Sin extractor, no se soporta (y nada se copia).
    let error = extract(&archive, &out, "a.msi", &ExtractOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Formato de archivo todavía no soportado: a.msi"
    );
    let calls = std::cell::RefCell::new(Vec::new());
    let msi = |a: &Path, d: &Path| {
        calls
            .borrow_mut()
            .push(format!("msi {} {}", a.display(), d.display()));
        Ok(())
    };
    let inno = |_: &Path, _: &Path| {
        calls.borrow_mut().push("inno".to_owned());
        Ok(())
    };
    let options = ExtractOptions {
        innosetup: true,
        msi: Some(&msi),
        inno: Some(&inno),
        ..ExtractOptions::default()
    };
    assert_eq!(
        extract(&archive, &out, "a.MSI", &options).unwrap(),
        Extraction::Extracted
    );
    assert_eq!(
        extract(&archive, &out, "setup.exe", &options).unwrap(),
        Extraction::Extracted
    );
    assert_eq!(calls.borrow().len(), 2);
    assert_eq!(calls.borrow()[1], "inno");
    // Sin `innosetup`, un .exe es un archivo suelto.
    let plain = ExtractOptions {
        inno: Some(&inno),
        ..ExtractOptions::default()
    };
    assert_eq!(
        extract(&archive, &out, "setup.exe", &plain).unwrap(),
        Extraction::Loose
    );
}

#[test]
fn el_mensaje_de_progreso_usa_el_texto_del_original() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("download");
    fs::write(&archive, b"x").unwrap();
    let logged = std::cell::RefCell::new(Vec::new());
    let log = |message: mito_core::Message| logged.borrow_mut().push(message.to_string());
    let options = ExtractOptions {
        log: Some(&log),
        ..ExtractOptions::default()
    };
    extract(&archive, &dir.path().join("o"), "tool.exe", &options).unwrap();
    assert_eq!(*logged.borrow(), ["Extrayendo tool.exe"]);
}

// --- 7-Zip real (opcional) -----------------------------------------------------------------

fn real_7zip() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("MITO_TEST_7Z").map(PathBuf::from),
        Some(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe")),
    ];
    candidates.into_iter().flatten().find(|p| p.is_file())
}

#[test]
fn deflate64_y_7z_sin_extension_con_el_7zip_real() {
    let Some(sevenzip) = real_7zip() else {
        eprintln!("omitido: no hay 7-Zip (define MITO_TEST_7Z)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("fixture.txt");
    let content = b"Deflate64 fixture ".repeat(1000);
    fs::write(&source, &content).unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        let output = std::process::Command::new(&sevenzip)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    // ZIP con Deflate64 (compress_type 9): se extrae de forma nativa.
    let zip = dir.path().join("deflate64.zip");
    run(&[
        "a".as_ref(),
        "-tzip".as_ref(),
        "-mm=Deflate64".as_ref(),
        zip.as_os_str(),
        source.as_os_str(),
    ]);
    let options = ExtractOptions {
        sevenzip: Some(&sevenzip),
        ..ExtractOptions::default()
    };
    let out = dir.path().join("zip-out");
    extract(&zip, &out, "fixture.zip", &options).unwrap();
    assert_eq!(fs::read(out.join("fixture.txt")).unwrap(), content);

    // 7z sin extensión: se detecta por la firma (test_extensionless_7zip_detected_by_signature).
    let sevenz = dir.path().join("fixture.7z");
    run(&[
        "a".as_ref(),
        "-t7z".as_ref(),
        sevenz.as_os_str(),
        source.as_os_str(),
    ]);
    let download = dir.path().join("download");
    fs::rename(&sevenz, &download).unwrap();
    let out = dir.path().join("7z-out");
    let result = extract(&download, &out, "download", &options).unwrap();
    assert_eq!(result, Extraction::Extracted);
    assert_eq!(fs::read(out.join("fixture.txt")).unwrap(), content);

    // Un fallo de 7-Zip es un error de dominio, no una excepción sin tratar.
    fs::write(&download, b"7z\xBC\xAF\x27\x1C corrupted").unwrap();
    let error = extract(&download, &dir.path().join("bad"), "download", &options).unwrap_err();
    assert!(
        error.to_string().starts_with("7-Zip terminó con error"),
        "{error}"
    );
}

// --- proptest: validación de rutas de entradas ---------------------------------------------

fn segments_have_traversal_or_ads(name: &str) -> bool {
    name.split(['/', '\\'])
        .any(|s| s == ".." || s.contains(':'))
        || name.starts_with(['/', '\\'])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Ningún nombre de entrada, por raro que sea, escribe fuera del destino; y los que
    /// contienen `..`, `:` o empiezan por separador se rechazan sin dejar nada.
    #[test]
    fn los_nombres_de_entrada_nunca_escapan_del_destino(
        name in proptest::string::string_regex(r"[a-zA-Z0-9._:/\\ ?*<>|~-]{1,24}").unwrap()
    ) {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_zip(dir.path(), "a.zip", &[(name.as_str(), b"data")]);
        let out = dir.path().join("out");
        let result = extract_zip(&archive, &out);
        if segments_have_traversal_or_ads(&name) {
            prop_assert!(result.is_err(), "{name:?} debía rechazarse");
        }
        if result.is_err() {
            prop_assert!(!out.exists(), "{name:?} dejó {:?}", tree(dir.path()));
        }
        // Pase lo que pase, solo existen el archivo y, si se extrajo, `out`.
        for entry in tree(dir.path()) {
            prop_assert!(
                entry == "a.zip" || entry == "out" || entry.starts_with("out/"),
                "{name:?} escribió fuera: {entry}"
            );
        }
    }
}
