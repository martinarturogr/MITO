//! Pruebas del bootstrap contra un servidor HTTP local y un lock de prueba: ninguna sale
//! a Internet. Cubren lo que el original no probaba (`ensure_tools`, `-Approved`,
//! `-PreviewWelcome`, hash fallido) y lo portable de `test_diagnostics.py`.

use std::collections::HashMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use mito_core::{Layout, Operation};
use mito_engine::HttpDownloader;
use mito_transfer::DownloadOptions;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zip::write::SimpleFileOptions;

use super::*;
use crate::console::testing::console;

fn sha(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").unwrap();
            hex
        })
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Servidor HTTP local que sirve `routes` y cuenta las peticiones.
struct Server {
    base: String,
    requests: Arc<AtomicUsize>,
}

fn serve(routes: HashMap<String, Vec<u8>>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
            }
            counter.fetch_add(1, Ordering::SeqCst);
            let text = String::from_utf8_lossy(&request).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let (status, body) = routes
                .get(&path)
                .map_or(("404 Not Found", Vec::new()), |b| ("200 OK", b.clone()));
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Server {
        base: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

/// Contenido servido y su lock de prueba.
struct Fixture {
    server: Server,
    lock: Value,
}

fn wt_zip() -> Vec<u8> {
    zip_of(&[
        ("terminal-9.9/WindowsTerminal.exe", b"terminal"),
        ("terminal-9.9/wt.exe", b"wt"),
        ("terminal-9.9/Images/a.txt", b"imagen"),
        ("otra-carpeta/ignorado.txt", b"no se copia"),
    ])
}

fn fixture(version: &str) -> Fixture {
    let inno = zip_of(&[("innounp.exe", b"inno"), ("innounp.htm", b"<html>")]);
    let wt = wt_zip();
    let routes: HashMap<String, Vec<u8>> = HashMap::from([
        ("/7zr.exe".to_owned(), b"7zr".to_vec()),
        ("/7z.exe".to_owned(), b"7z".to_vec()),
        ("/7z.dll".to_owned(), b"dll".to_vec()),
        ("/wt.zip".to_owned(), wt.clone()),
        ("/inno.zip".to_owned(), inno.clone()),
    ]);
    let server = serve(routes);
    let url = |path: &str| format!("{}{path}", server.base);
    let file = |name: &str, bytes: &[u8]| {
        json!({"fileName": name, "url": url(&format!("/{name}")), "sha256": sha(bytes),
               "install": {"type": "file", "destination": ".", "target": name}})
    };
    let lock = json!({
        "schema": 1,
        "tools": [
            {"id": "7zip", "version": version, "required": ["7z.exe", "7z.dll"],
             "artifacts": [file("7zr.exe", b"7zr"), file("7z.exe", b"7z"), file("7z.dll", b"dll")]},
            {"id": "windows-terminal", "version": version,
             "required": ["WindowsTerminal.exe", "wt.exe"],
             "artifacts": [{"fileName": "wt.zip", "url": url("/wt.zip"), "sha256": sha(&wt),
                 "install": {"type": "zip", "destination": ".", "source": "terminal-9.9"}}]},
            {"id": "innounp", "version": version, "required": ["innounp.exe", "innounp.htm"],
             "artifacts": [{"fileName": "inno.zip", "url": url("/inno.zip"), "sha256": sha(&inno),
                 "install": {"type": "zip", "source": "."}}]}
        ]
    });
    Fixture { server, lock }
}

fn instance(lock: &Value) -> (tempfile::TempDir, Layout) {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    write_lock(&layout, lock);
    (dir, layout)
}

fn write_lock(layout: &Layout, lock: &Value) {
    fs::create_dir_all(layout.core()).unwrap();
    fs::write(
        layout.bootstrap_lock(),
        serde_json::to_vec_pretty(lock).unwrap(),
    )
    .unwrap();
}

fn downloader() -> HttpDownloader {
    HttpDownloader::new(DownloadOptions {
        attempts: 1,
        proxies: Some(mito_transfer::network::Proxies::default()),
        ..DownloadOptions::default()
    })
}

fn run_with(layout: &Layout, options: Options, input: &str) -> (i32, String, String) {
    let (console, out, err) = console(input);
    let code = run(
        layout,
        options,
        &console,
        &Operation::silent(),
        &downloader(),
    );
    (code, out.text(), err.text())
}

const APPROVED: Options = Options {
    approved: true,
    preview_welcome: false,
};

fn records(layout: &Layout) -> Vec<Value> {
    fs::read_to_string(layout.logs().join("mito.log"))
        .map(|text| {
            text.lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        })
        .unwrap_or_default()
}

fn tool_files(layout: &Layout, id: &str) -> Vec<String> {
    let mut names: Vec<String> = walk(&layout.tools().join(id));
    names.sort();
    names
}

fn visit(folder: &Path, base: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, base, found);
        } else {
            found.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn walk(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    visit(folder, folder, &mut found);
    found
}

// --- bienvenida --------------------------------------------------------------------

#[test]
fn solo_s_si_y_si_con_tilde_dan_consentimiento() {
    for answer in ["s", "S", "si", "SI", " sí ", "Sí\r\n", "s\n"] {
        assert!(accepts(answer), "{answer:?}");
    }
    for answer in ["", "n", "N", "yes", "y", "no", "sí.", "claro", "ss"] {
        assert!(!accepts(answer), "{answer:?}");
    }
}

#[test]
fn el_texto_de_la_bienvenida_es_el_del_original_sin_python_ni_textual() {
    let lock = Lock::parse(lock::EMBEDDED.as_bytes()).unwrap();
    let tools: Vec<&Tool> = lock.tools.iter().collect();
    let text: Vec<String> = welcome_lines(&tools).into_iter().map(|(l, _)| l).collect();
    assert_eq!(
        text.join("\n"),
        "\nBienvenido a MITO (My Integrated Terminal Orchestrator)\n\n\
         Para preparar MITO se descargarán las siguientes herramientas:\n  \
         - 7-Zip 26.02\n  \
         - Windows Terminal 1.24.11911.0\n  \
         - innounp (extracción de instaladores Inno Setup) 2.71.1\n\n\
         Es necesaria una conexión a Internet.\n\
         Las herramientas se guardarán dentro de esta carpeta de MITO.\n"
    );
    let joined = text.join("\n").to_lowercase();
    for forbidden in ["python", "pillow", "textual", "sdk mcp"] {
        assert!(!joined.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn la_bienvenida_usa_cian_para_el_titulo_y_amarillo_para_el_aviso() {
    let lock = Lock::parse(lock::EMBEDDED.as_bytes()).unwrap();
    let tools: Vec<&Tool> = lock.tools.iter().collect();
    let colors: Vec<(String, Option<Color>)> = welcome_lines(&tools)
        .into_iter()
        .filter(|(_, color)| color.is_some())
        .collect();
    assert_eq!(
        colors,
        [
            (
                "Bienvenido a MITO (My Integrated Terminal Orchestrator)".to_owned(),
                Some(Color::Cyan)
            ),
            (
                "Es necesaria una conexión a Internet.".to_owned(),
                Some(Color::Yellow)
            ),
        ]
    );
    let (console, out, _) = console("n\n");
    let console = console.with_color();
    assert!(!show_welcome(&console, &tools));
    let text = out.text();
    assert!(
        text.contains("\x1b[36mBienvenido a MITO (My Integrated Terminal Orchestrator)\x1b[0m")
    );
    assert!(text.contains("\x1b[33mEs necesaria una conexión a Internet.\x1b[0m"));
    assert!(text.ends_with("¿Deseas continuar? [s/N] "));
}

#[test]
fn cancelar_no_crea_herramientas_ni_descarga_y_registra_el_diagnostico() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    let (code, out, _) = run_with(&layout, Options::default(), "n\n");
    assert_eq!(code, CANCELLED);
    assert!(out.contains("Bienvenido a MITO"));
    assert!(out.contains("  - Windows Terminal 9.9\n"));
    assert!(
        out.ends_with("Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.\n")
    );
    assert!(!layout.tools().exists());
    assert!(!layout.state().join("bootstrap-tools.json").exists());
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 0);
    let last = records(&layout).pop().unwrap();
    assert_eq!(last["phase"], "confirmation");
    assert_eq!(last["level"], "info");
    assert_eq!(last["message"], "Bootstrap cancelled before downloading");
}

#[test]
fn sin_respuesta_la_bienvenida_cancela() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    let (code, _, _) = run_with(&layout, Options::default(), "");
    assert_eq!(code, CANCELLED);
    assert!(!layout.tools().exists());
}

#[test]
fn la_vista_previa_muestra_todo_sin_crear_nada_ni_registrar() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    let options = Options {
        approved: false,
        preview_welcome: true,
    };
    let (code, out, _) = run_with(&layout, options, "s\n");
    assert_eq!(code, 0);
    assert!(out.contains("  - 7-Zip 9.9\n"));
    assert!(out.ends_with(
        "Vista previa terminada. No se ha descargado ni preparado ninguna herramienta.\n"
    ));
    assert!(!layout.tools().exists());
    assert!(!layout.logs().exists());
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 0);
}

// --- instalación -------------------------------------------------------------------

#[test]
fn con_aprobacion_instala_las_tres_herramientas_con_sus_marcadores() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    let (code, out, err) = run_with(&layout, APPROVED, "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(!out.contains("Bienvenido"));
    assert!(out.contains("Preparando 7zip 9.9…\n"));
    assert!(out.contains("Preparando windows-terminal 9.9…\n"));
    assert_eq!(
        tool_files(&layout, "7zip"),
        [".mito-tool.json", "7z.dll", "7z.exe", "7zr.exe"]
    );
    assert_eq!(
        tool_files(&layout, "windows-terminal"),
        [
            ".mito-tool.json",
            ".portable",
            "Images/a.txt",
            // Marca de Windows Terminal: los iconos de `Images` se sincronizan con mito.ico.
            "Images/terminal_contrast-black.ico",
            "Images/terminal_contrast-white.ico",
            "WindowsTerminal.exe",
            "wt.exe"
        ]
    );
    // Un Terminal que no es un PE no se puede marcar: aviso, no error (ADR 0010).
    assert!(!layout.branded_terminal_exe().exists());
    assert!(
        records(&layout)
            .iter()
            .any(|r| r["operation"] == "terminal.branding" && r["level"] == "warning")
    );
    assert_eq!(
        tool_files(&layout, "innounp"),
        [".mito-tool.json", "innounp.exe", "innounp.htm"]
    );
    assert_eq!(
        fs::read(layout.tools().join("windows-terminal/.portable")).unwrap(),
        b""
    );
    assert_eq!(
        fs::read_to_string(layout.tools().join("7zip/.mito-tool.json")).unwrap(),
        "{\n  \"version\": \"9.9\"\n}\n"
    );
    let lock_bytes = fs::read(layout.bootstrap_lock()).unwrap();
    assert_eq!(
        fs::read_to_string(layout.state().join("bootstrap-tools.json")).unwrap(),
        format!("{{\n  \"lock_sha256\": \"{}\"\n}}\n", sha(&lock_bytes))
    );
    // Solo quedan las tres carpetas: ningún staging.
    let mut entries: Vec<String> = fs::read_dir(layout.tools())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, ["7zip", "innounp", "windows-terminal"]);
    let phases: Vec<String> = records(&layout)
        .iter()
        .filter_map(|r| r["phase"].as_str().map(str::to_owned))
        .collect();
    for phase in ["startup", "confirmation", "complete"] {
        assert!(phases.iter().any(|p| p == phase), "{phase}: {phases:?}");
    }
    let messages: Vec<String> = records(&layout)
        .iter()
        .filter_map(|r| r["message"].as_str().map(str::to_owned))
        .collect();
    for message in ["Preparing tool", "download-and-hash", "extract-zip"] {
        assert!(messages.iter().any(|m| m == message), "{message}");
    }
}

#[test]
fn tras_la_bienvenida_con_s_instala_y_con_todo_listo_no_vuelve_a_preguntar() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    let (code, out, _) = run_with(&layout, Options::default(), "s\n");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Bienvenido a MITO"));
    assert!(is_missing(&layout).eq(&false));
    let downloads = fixture.server.requests.load(Ordering::SeqCst);
    assert_eq!(downloads, 5);
    let (code, out, _) = run_with(&layout, Options::default(), "");
    assert_eq!(code, 0);
    assert!(!out.contains("Bienvenido"));
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), downloads);
    let messages: Vec<String> = records(&layout)
        .iter()
        .filter_map(|r| r["message"].as_str().map(str::to_owned))
        .collect();
    assert!(messages.iter().any(|m| m == "Tools already prepared"));
}

#[test]
fn solo_se_ofrece_lo_que_falta() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    run_with(&layout, APPROVED, "");
    fs::remove_dir_all(layout.tools().join("innounp")).unwrap();
    assert!(is_missing(&layout));
    let (code, out, _) = run_with(&layout, Options::default(), "s\n");
    assert_eq!(code, 0);
    assert!(out.contains("(extracción de instaladores Inno Setup) 9.9"));
    assert!(!out.contains("7-Zip"));
    assert!(!is_missing(&layout));
}

#[test]
fn una_version_nueva_reinstala_y_sustituye_a_la_anterior() {
    let old = fixture("9.9");
    let (_dir, layout) = instance(&old.lock);
    run_with(&layout, APPROVED, "");
    fs::write(layout.tools().join("7zip/extra-del-usuario.txt"), "x").unwrap();
    let new = fixture("10.0");
    write_lock(&layout, &new.lock);
    assert!(is_missing(&layout));
    let (code, out, _) = run_with(&layout, APPROVED, "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Preparando 7zip 10.0…"));
    assert_eq!(
        fs::read_to_string(layout.tools().join("7zip/.mito-tool.json")).unwrap(),
        "{\n  \"version\": \"10.0\"\n}\n"
    );
    assert!(!layout.tools().join("7zip/extra-del-usuario.txt").exists());
    assert_eq!(old.server.requests.load(Ordering::SeqCst), 5);
    assert_eq!(new.server.requests.load(Ordering::SeqCst), 5);
    assert!(!is_missing(&layout));
}

#[test]
fn un_marcador_corrupto_equivale_a_una_herramienta_ausente() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    run_with(&layout, APPROVED, "");
    let marker = layout.tools().join("7zip/.mito-tool.json");
    fs::write(&marker, "{esto no es json").unwrap();
    assert!(is_missing(&layout));
    let (code, out, _) = run_with(&layout, APPROVED, "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Preparando 7zip 9.9…"));
    assert!(!out.contains("Preparando innounp"));
    assert!(!is_missing(&layout));
}

#[test]
fn falta_un_fichero_requerido_cuenta_como_herramienta_no_lista() {
    let fixture = fixture("9.9");
    let (_dir, layout) = instance(&fixture.lock);
    run_with(&layout, APPROVED, "");
    fs::remove_file(layout.tools().join("windows-terminal/wt.exe")).unwrap();
    assert!(is_missing(&layout));
}

#[test]
fn un_hash_incorrecto_falla_sin_publicar_ni_dejar_staging() {
    let mut fixture = fixture("9.9");
    fixture.lock["tools"][1]["artifacts"][0]["sha256"] = json!("0".repeat(64));
    let (_dir, layout) = instance(&fixture.lock);
    let (code, _, err) = run_with(&layout, APPROVED, "");
    assert_eq!(code, FAILURE);
    assert!(err.starts_with("MITO: "), "{err}");
    assert!(err.contains("MITO: log: "), "{err}");
    // 7-Zip, anterior en el lock, sí quedó preparada; el Terminal no.
    assert!(layout.tools().join("7zip/7z.exe").is_file());
    assert!(!layout.tools().join("windows-terminal").exists());
    assert!(!layout.tools().join("innounp").exists());
    let leftovers: Vec<String> = fs::read_dir(layout.tools())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("tmp"))
        .collect();
    assert_eq!(leftovers.len(), 0);
    assert!(!layout.state().join("bootstrap-tools.json").exists());
    let last = records(&layout).pop().unwrap();
    assert_eq!(last["level"], "error");
    assert_eq!(last["phase"], "install-tool");
}

#[test]
fn un_fichero_requerido_ausente_tras_extraer_es_herramienta_incompleta() {
    let mut fixture = fixture("9.9");
    fixture.lock["tools"][2]["required"] = json!(["innounp.exe", "falta.txt"]);
    let (_dir, layout) = instance(&fixture.lock);
    let (code, _, err) = run_with(&layout, APPROVED, "");
    assert_eq!(code, FAILURE);
    assert!(
        err.contains("MITO: Herramienta incompleta: innounp\n"),
        "{err}"
    );
    assert!(!layout.tools().join("innounp").exists());
}

#[test]
fn un_tipo_de_instalacion_desconocido_se_rechaza() {
    let mut fixture = fixture("9.9");
    fixture.lock["tools"][2]["artifacts"][0]["install"]["type"] = json!("rpm");
    let (_dir, layout) = instance(&fixture.lock);
    let (code, _, err) = run_with(&layout, APPROVED, "");
    assert_eq!(code, FAILURE);
    assert!(
        err.contains("MITO: Formato de herramienta core desconocido\n"),
        "{err}"
    );
}

#[test]
fn un_fallo_de_7zip_no_muestra_los_argumentos() {
    let mut fixture = fixture("9.9");
    // `where.exe` hace de extractor: rechaza los argumentos y sale con error.
    let cmd =
        fs::read(Path::new(&std::env::var("SystemRoot").unwrap()).join(r"System32\where.exe"))
            .unwrap();
    let server = serve(HashMap::from([
        ("/7zr.exe".to_owned(), cmd.clone()),
        ("/pack.7z".to_owned(), b"contenido".to_vec()),
    ]));
    fixture.lock["tools"] = json!([{
        "id": "7zip", "version": "9.9", "required": ["7z.exe"],
        "artifacts": [
            {"fileName": "7zr.exe", "url": format!("{}/7zr.exe", server.base), "sha256": sha(&cmd),
             "install": {"type": "file", "target": "7zr.exe"}},
            {"fileName": "pack.7z", "url": format!("{}/pack.7z", server.base), "sha256": sha(b"contenido"),
             "install": {"type": "sevenZip", "executable": "7zr.exe"}}
        ]
    }]);
    let (_dir, layout) = instance(&fixture.lock);
    let (code, _, err) = run_with(&layout, APPROVED, "");
    assert_eq!(code, FAILURE);
    assert!(
        err.starts_with("MITO: Child process failed with exit code "),
        "{err}"
    );
    assert!(!err.contains("pack.7z"), "{err}");
    assert!(!layout.tools().join("7zip").exists());
}

#[test]
fn un_archivo_7z_se_extrae_con_el_extractor_privado_si_hay_un_7zip_en_el_equipo() {
    // Usa el 7-Zip instalado en el equipo como extractor de prueba; sin él, no hay nada
    // que probar (el extractor real lo verifica el revisor con la descarga oficial).
    let installed = Path::new(r"C:\Program Files\7-Zip");
    let (exe, dll) = (installed.join("7z.exe"), installed.join("7z.dll"));
    if !exe.is_file() || !dll.is_file() {
        return;
    }
    let work = tempfile::tempdir().unwrap();
    fs::write(work.path().join("dentro.txt"), "contenido").unwrap();
    let archive = work.path().join("pack.7z");
    let made = std::process::Command::new(&exe)
        .current_dir(work.path())
        .args(["a", "pack.7z", "dentro.txt"])
        .output()
        .unwrap();
    assert!(made.status.success());
    let (exe_bytes, dll_bytes, pack) = (
        fs::read(&exe).unwrap(),
        fs::read(&dll).unwrap(),
        fs::read(&archive).unwrap(),
    );
    let server = serve(HashMap::from([
        ("/7z.exe".to_owned(), exe_bytes.clone()),
        ("/7z.dll".to_owned(), dll_bytes.clone()),
        ("/pack.7z".to_owned(), pack.clone()),
    ]));
    let url = |name: &str| format!("{}/{name}", server.base);
    let lock = json!({"tools": [{
        "id": "7zip", "version": "9.9", "required": ["7z.exe", "dentro.txt"],
        "artifacts": [
            {"fileName": "7z.exe", "url": url("7z.exe"), "sha256": sha(&exe_bytes),
             "install": {"type": "file", "target": "7z.exe"}},
            {"fileName": "7z.dll", "url": url("7z.dll"), "sha256": sha(&dll_bytes),
             "install": {"type": "file", "target": "7z.dll"}},
            {"fileName": "pack.7z", "url": url("pack.7z"), "sha256": sha(&pack),
             "install": {"type": "sevenZip", "executable": "7z.exe"}}
        ]
    }]});
    let (_dir, layout) = instance(&lock);
    let (code, out, err) = run_with(&layout, APPROVED, "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        fs::read_to_string(layout.tools().join("7zip/dentro.txt")).unwrap(),
        "contenido"
    );
}

#[test]
fn un_lock_que_no_es_json_falla_antes_de_crear_nada() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    fs::create_dir_all(layout.core()).unwrap();
    fs::write(layout.bootstrap_lock(), "{ esto no es json").unwrap();
    let (code, _, err) = run_with(&layout, Options::default(), "s\n");
    assert_eq!(code, FAILURE);
    assert!(err.contains("bootstrap.lock.json"), "{err}");
    assert!(!layout.tools().exists());
    let last = records(&layout).pop().unwrap();
    assert_eq!(last["phase"], "read-bootstrap-lock");
    assert_eq!(last["level"], "error");
}

#[test]
fn un_id_de_herramienta_con_ruta_se_rechaza() {
    let lock = json!({"tools": [{"id": "../fuera", "version": "1"}]});
    assert!(Lock::parse(lock.to_string().as_bytes()).is_err());
}

#[test]
fn sin_lock_en_la_instancia_se_usa_el_incluido_en_el_binario() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let lock = Lock::load(&layout).unwrap();
    let ids: Vec<&str> = lock.tools.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["7zip", "windows-terminal", "innounp"]);
    assert_eq!(lock.sha256, sha(lock::EMBEDDED.as_bytes()));
    assert!(is_missing(&layout));
}

#[test]
fn el_lock_incluido_solo_tiene_tools_con_los_hashes_del_original() {
    let value: Value = serde_json::from_str(lock::EMBEDDED).unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["schema", "tools"]);
    let lock = Lock::parse(lock::EMBEDDED.as_bytes()).unwrap();
    let hashes: Vec<(&str, &str, &str)> = lock
        .tools
        .iter()
        .flat_map(|t| {
            t.artifacts
                .iter()
                .map(move |a| (t.id.as_str(), t.version.as_str(), a.sha256.as_str()))
        })
        .collect();
    assert_eq!(
        hashes,
        [
            (
                "7zip",
                "26.02",
                "56b8cc9f4971cef253644fafe54063ed7fdca551d4dee0f8c6baa81b855acd72"
            ),
            (
                "7zip",
                "26.02",
                "6745fa76dc2ea031596d8678f6f6b99c3c1b435b4164a63485adbbc7b8d82ef0"
            ),
            (
                "windows-terminal",
                "1.24.11911.0",
                "7691efeb71c8dd0b95536c84e366fa4cf809a42c534912f9cefa1056534383bd"
            ),
            (
                "innounp",
                "2.71.1",
                "f2f037fdbc63de31248efae9ccb294398d160dc0cbad5f36d14c2f159e17bbf5"
            ),
        ]
    );
    for tool in &lock.tools {
        for artifact in &tool.artifacts {
            assert!(artifact.url.starts_with("https://"), "{}", artifact.url);
        }
    }
}

#[test]
fn copy_tree_mezcla_con_lo_existente() {
    let dir = tempfile::tempdir().unwrap();
    let (source, destination) = (dir.path().join("a"), dir.path().join("b"));
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("sub/x.txt"), "nuevo").unwrap();
    fs::write(source.join("y.txt"), "y").unwrap();
    fs::write(destination.join("viejo.txt"), "viejo").unwrap();
    install::copy_tree(&source, &destination).unwrap();
    assert_eq!(walk(&destination), ["sub/x.txt", "viejo.txt", "y.txt"]);
}
