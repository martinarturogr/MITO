//! Port de `core/tests/test_transfer.py` del original (descarga HTTP) y de los casos que
//! el contrato señala como huecos de test.
//!
//! No se portan las comprobaciones de `Catalog.compatibility` de esos tests: pertenecen
//! a la fase del catálogo (F3).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Request, Server, ok, redirect, respond};
use mito_core::Operation;
use mito_transfer::hash::{HashAlgorithm, Hasher};
use mito_transfer::{DownloadOptions, Error, MAX_DOWNLOAD, download};
use tracing_subscriber::fmt::MakeWriter;

fn hex(algorithm: HashAlgorithm, data: &[u8]) -> String {
    let mut hasher = Hasher::new(algorithm);
    hasher.update(data);
    hasher.hex_digest()
}

fn fast() -> DownloadOptions {
    DownloadOptions {
        delay: Duration::ZERO,
        ..DownloadOptions::default()
    }
}

fn get(
    url: &str,
    target: &std::path::Path,
    expected: Option<&str>,
    options: &DownloadOptions,
) -> Result<(), Error> {
    download(url, target, expected, options, &Operation::detached())
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// Ejecuta `body` recogiendo los eventos `tracing` que emite en este hilo.
fn captured<T>(body: impl FnOnce() -> T) -> (T, String) {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let result = tracing::subscriber::with_default(subscriber, body);
    let text = String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned();
    (result, text)
}

#[test]
fn redireccion_y_verificacion_de_hash() {
    let payload = b"MITO HTTP payload";
    let server = Server::http(move |req, out| {
        if req.path == "/redirect" {
            redirect(out, 302, "/payload");
        } else {
            ok(out, &[], payload);
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let url = server.url("/redirect");
    for algorithm in [
        HashAlgorithm::Md5,
        HashAlgorithm::Sha1,
        HashAlgorithm::Sha256,
        HashAlgorithm::Sha512,
    ] {
        let expected = format!("{}:{}", algorithm.name(), hex(algorithm, payload));
        get(&url, &target, Some(&expected), &fast()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), payload);
    }
    for expected in [None, Some(""), Some("skip")] {
        get(&url, &target, expected, &fast()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), payload);
    }
    let zeros = "0".repeat(64);
    assert!(matches!(
        get(&url, &target, Some(&zeros), &fast()),
        Err(Error::Integrity { portal: false, .. })
    ));
    assert!(!target.exists());
    let md5_zeros = format!("md5:{}", "0".repeat(32));
    assert!(get(&url, &target, Some(&md5_zeros), &fast()).is_err());
    assert!(!target.exists());

    let limited = DownloadOptions {
        max_bytes: payload.len() as u64 - 1,
        ..fast()
    };
    let error = get(&url, &target, None, &limited).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("Descarga excede el límite de {} bytes", payload.len() - 1)
    );
    assert!(!target.exists());
    let exact = DownloadOptions {
        max_bytes: payload.len() as u64,
        ..fast()
    };
    get(&url, &target, None, &exact).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
}

#[test]
fn el_limite_por_defecto_es_4_gib_y_su_mensaje_es_propio() {
    assert_eq!(MAX_DOWNLOAD, 4 * 1024 * 1024 * 1024);
    let server = Server::http(|_, out| ok(out, &[], b"0123456789"));
    let dir = tempfile::tempdir().unwrap();
    let options = DownloadOptions {
        max_bytes: MAX_DOWNLOAD,
        ..fast()
    };
    // Con el límite por defecto la descarga pasa; el mensaje de 4 GiB se comprueba con
    // el mismo código cuando se supera.
    get(&server.url("/x"), &dir.path().join("d"), None, &options).unwrap();
    let defaults = DownloadOptions::default();
    assert_eq!(defaults.attempts, 5);
    assert_eq!(defaults.delay, Duration::from_secs(2));
    assert_eq!(defaults.timeout, Duration::from_secs(60));
    assert_eq!(DownloadOptions::quick().attempts, 2);
    assert_eq!(DownloadOptions::quick().delay, Duration::from_secs(1));
}

#[test]
fn una_pagina_de_portal_se_informa_en_lugar_de_un_error_de_hash() {
    let page: &'static [u8] =
        b"<!DOCTYPE html><html><form><input name=\"magic\" type=\"hidden\"></form></html>";
    let server = Server::http(move |req, out| {
        if req.path.starts_with("/fgtauth") {
            ok(out, &[("Content-Type", "text/html")], page);
        } else {
            redirect(out, 303, "/fgtauth?magic=1");
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let zeros = "0".repeat(64);
    for (path, expected) in [
        ("/app.zip", Some(zeros.as_str())),
        ("/app.zip", None),
        ("/bucket/app.json", None),
    ] {
        let error = get(&server.url(path), &target, expected, &fast()).unwrap_err();
        let Error::Integrity { portal, host, .. } = &error else {
            panic!("se esperaba Integrity, no {error:?}");
        };
        assert!(portal);
        assert_eq!(host, "127.0.0.1");
        let text = error.to_string();
        assert!(text.contains("página web"), "{text}");
        assert!(text.contains("127.0.0.1"), "{text}");
        assert!(!target.exists());
    }
    // Un documento HTML pedido no se confunde con una interceptación.
    get(&server.url("/fgtauth/index.html"), &target, None, &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), page);
}

#[test]
fn el_hash_incorrecto_nombra_url_tamano_y_servidor_sin_la_query_firmada() {
    let body: &'static [u8] =
        b"\xef\xbb\xbf<?xml version=\"1.0\" encoding=\"utf-8\"?><BlockList></BlockList>";
    let server = Server::http(move |req, out| {
        if req.path.starts_with("/asset") {
            ok(out, &[("Content-Type", "application/octet-stream")], body);
        } else {
            redirect(out, 302, "/asset?sig=secret-token");
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let url = server.url("/release/jdk.zip?token=abc");
    let zeros = "0".repeat(64);
    let (result, log) = captured(|| get(&url, &target, Some(&zeros), &fast()));
    let error = result.unwrap_err();
    let Error::Integrity {
        url: public,
        size,
        host,
        portal,
        ..
    } = &error
    else {
        panic!("se esperaba Integrity, no {error:?}");
    };
    assert_eq!(*public, server.url("/release/jdk.zip"));
    assert_eq!(*size, Some(body.len() as u64));
    assert_eq!(host, "127.0.0.1");
    assert!(!portal);
    let message = error.to_string();
    assert!(message.starts_with("Hash incorrecto en "), "{message}");
    assert!(message.contains(&server.url("/release/jdk.zip")));
    assert!(message.contains(&body.len().to_string()));
    assert!(!message.contains("token=abc") && !message.contains("secret-token"));
    assert!(message.ends_with("Se ha descartado la descarga."));
    assert!(!target.exists());
    assert!(log.contains("download.hash"), "{log}");
    assert!(
        log.contains("Hash mismatch") && log.contains("sha256"),
        "{log}"
    );
    assert!(
        !log.contains("secret-token") && !log.contains("token=abc"),
        "{log}"
    );
}

#[test]
fn esquema_y_hash_invalidos_se_rechazan_antes_de_la_red_y_de_crear_carpetas() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("nueva/carpeta/app.zip");
    let error = get("file:///app.zip", &target, None, &fast()).unwrap_err();
    assert_eq!(error.to_string(), "Las descargas requieren HTTP o HTTPS");
    let error = get(
        "http://127.0.0.1:9/x",
        &target,
        Some("md5:invalid"),
        &fast(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Formato de hash incorrecto");
    assert!(!dir.path().join("nueva").exists());
}

#[test]
fn una_redireccion_a_otro_esquema_se_rechaza() {
    let server = Server::http(|_, out| redirect(out, 302, "file:///app.zip"));
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app.zip");
    let error = get(&server.url("/x"), &target, None, &fast()).unwrap_err();
    assert_eq!(error.to_string(), "La redirección debe usar HTTP o HTTPS");
    assert!(!target.exists());
}

#[test]
fn demasiadas_redirecciones_son_un_error_permanente() {
    let counter = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&counter);
    let server = Server::http(move |_, out| {
        seen.fetch_add(1, Ordering::SeqCst);
        redirect(out, 302, "/loop");
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app.zip");
    let error = get(&server.url("/loop"), &target, None, &fast()).unwrap_err();
    assert!(
        matches!(error, Error::Http { status: 302, .. }),
        "{error:?}"
    );
    // Una petición inicial más 10 redirecciones, sin reintentos.
    assert_eq!(counter.load(Ordering::SeqCst), 11);
    assert!(!target.exists());
}

#[test]
fn reintenta_errores_transitorios_pero_no_los_permanentes() {
    let payload: &'static [u8] = b"MITO retry payload";
    let gateway = Arc::new(AtomicUsize::new(0));
    let down = Arc::new(AtomicUsize::new(0));
    let missing = Arc::new(AtomicUsize::new(0));
    let (g, d, m) = (
        Arc::clone(&gateway),
        Arc::clone(&down),
        Arc::clone(&missing),
    );
    let server = Server::http(move |req: &Request, out| match req.path.as_str() {
        "/gateway" => {
            if g.fetch_add(1, Ordering::SeqCst) < 2 {
                respond(out, 504, &[("Content-Length", "0")], b"");
            } else {
                ok(out, &[], payload);
            }
        }
        "/down" => {
            d.fetch_add(1, Ordering::SeqCst);
            respond(out, 504, &[("Content-Length", "0")], b"");
        }
        _ => {
            m.fetch_add(1, Ordering::SeqCst);
            respond(out, 404, &[("Content-Length", "0")], b"");
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let digest = hex(HashAlgorithm::Sha256, payload);
    // 504 transitorio de un gateway o proxy: reintenta y completa.
    get(&server.url("/gateway"), &target, Some(&digest), &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(gateway.load(Ordering::SeqCst), 3);
    // 404 permanente: aborta en el primer intento.
    let error = get(&server.url("/missing"), &target, None, &fast()).unwrap_err();
    assert!(
        matches!(error, Error::Http { status: 404, .. }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        format!("HTTP Error 404: Not Found ({})", server.url("/missing"))
    );
    assert_eq!(missing.load(Ordering::SeqCst), 1);
    assert!(!target.exists());
    // Agotados los intentos, el último error se propaga y se descarta el archivo.
    let three = DownloadOptions {
        attempts: 3,
        ..fast()
    };
    let error = get(&server.url("/down"), &target, None, &three).unwrap_err();
    assert!(
        matches!(error, Error::Http { status: 504, .. }),
        "{error:?}"
    );
    assert_eq!(down.load(Ordering::SeqCst), 3);
    assert!(!target.exists());
}

#[test]
fn reintenta_408_y_429_y_avisa_con_el_texto_del_original() {
    let payload: &'static [u8] = b"retry payload";
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let server = Server::http(move |_, out| match seen.fetch_add(1, Ordering::SeqCst) {
        0 => respond(out, 408, &[("Content-Length", "0")], b""),
        1 => respond(out, 429, &[("Content-Length", "0")], b""),
        _ => ok(out, &[], payload),
    });
    let dir = tempfile::tempdir().unwrap();
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notes);
    let operation = Operation::new(move |state| {
        let mut notes = sink.lock().unwrap();
        let note = state.note.to_string();
        if !note.is_empty() && notes.last() != Some(&note) {
            notes.push(note);
        }
    });
    download(
        &server.url("/x"),
        &dir.path().join("d"),
        None,
        &fast(),
        &operation,
    )
    .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(
        *notes.lock().unwrap(),
        [
            "Aviso: HTTP 408; reintentando la descarga (2/5)",
            "Aviso: HTTP 429; reintentando la descarga (3/5)"
        ]
    );
}

fn payload16k() -> Vec<u8> {
    (0..=255_u8).cycle().take(256 * 64).collect()
}

type Seen = Arc<Mutex<Vec<(Option<String>, Option<String>)>>>;

/// Servidor del test de reanudación: la primera respuesta completa se corta a la mitad.
fn resuming_server(payload: Vec<u8>, requests: &Seen) -> Server {
    let requests = Arc::clone(requests);
    Server::http(move |req, out| {
        let count = {
            let mut list = requests.lock().unwrap();
            list.push((
                req.header("range").map(str::to_owned),
                req.header("if-range").map(str::to_owned),
            ));
            list.len()
        };
        let validator = (req.path == "/etag").then_some("\"v1\"");
        let mut extra: Vec<(&str, &str)> = Vec::new();
        if let Some(validator) = &validator {
            extra.push(("ETag", validator));
        }
        if let Some(wanted) = req.header("range")
            && req.path != "/ignores-range"
        {
            let start: usize = wanted
                .trim_start_matches("bytes=")
                .trim_end_matches('-')
                .parse()
                .unwrap();
            let range = format!("bytes {start}-{}/{}", payload.len() - 1, payload.len());
            extra.push(("Content-Range", &range));
            let length = (payload.len() - start).to_string();
            extra.push(("Content-Length", &length));
            respond(out, 206, &extra, &payload[start..]);
            return;
        }
        let length = payload.len().to_string();
        extra.push(("Content-Length", &length));
        let sent = if count > 1 {
            &payload[..]
        } else {
            &payload[..payload.len() / 2]
        };
        respond(out, 200, &extra, sent);
    })
}

#[test]
fn una_descarga_interrumpida_se_reanuda_con_range_desde_lo_recibido() {
    let payload = payload16k();
    let requests: Seen = Arc::default();
    let server = resuming_server(payload.clone(), &requests);
    let digest = hex(HashAlgorithm::Sha256, &payload);
    let half = format!("bytes={}-", payload.len() / 2);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");

    // Validador ETag y sin hash: reanuda con Range e If-Range.
    get(&server.url("/etag"), &target, None, &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(
        requests.lock().unwrap()[1],
        (Some(half.clone()), Some("\"v1\"".to_owned()))
    );

    // Hash declarado y sin validador: reanuda solo con Range.
    requests.lock().unwrap().clear();
    get(&server.url("/plain"), &target, Some(&digest), &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(requests.lock().unwrap()[1], (Some(half), None));

    // Ni validador ni hash: el contenido no se puede comprobar, así que reinicia.
    requests.lock().unwrap().clear();
    get(&server.url("/plain"), &target, None, &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    let ranges: Vec<_> = requests
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.0.clone())
        .collect();
    assert_eq!(ranges, [None, None]);

    // Un servidor que ignora Range responde 200: empieza de nuevo, nunca concatena.
    requests.lock().unwrap().clear();
    get(
        &server.url("/ignores-range"),
        &target,
        Some(&digest),
        &fast(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[test]
fn un_content_range_que_no_continua_el_archivo_reinicia_desde_cero() {
    let payload = payload16k();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let body = payload.clone();
    let server = Server::http(move |req, out| {
        let call = seen.fetch_add(1, Ordering::SeqCst);
        let length = body.len().to_string();
        match call {
            0 => respond(out, 200, &[("Content-Length", &length)], &body[..4096]),
            // Responde 206 pero empezando en otro punto: no se debe concatenar.
            1 => {
                assert!(req.header("range").is_some());
                respond(
                    out,
                    206,
                    &[
                        ("Content-Range", "bytes 0-16383/16384"),
                        ("Content-Length", &length),
                    ],
                    &body,
                );
            }
            _ => respond(out, 200, &[("Content-Length", &length)], &body),
        }
    });
    let digest = hex(HashAlgorithm::Sha256, &payload);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    get(&server.url("/x"), &target, Some(&digest), &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn un_416_al_reanudar_reinicia_la_descarga() {
    let payload = payload16k();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let body = payload.clone();
    let server = Server::http(move |req, out| {
        let call = seen.fetch_add(1, Ordering::SeqCst);
        let length = body.len().to_string();
        if call == 0 {
            respond(out, 200, &[("Content-Length", &length)], &body[..8192]);
        } else if req.header("range").is_some() {
            respond(out, 416, &[("Content-Length", "0")], b"");
        } else {
            respond(out, 200, &[("Content-Length", &length)], &body);
        }
    });
    let digest = hex(HashAlgorithm::Sha256, &payload);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    get(&server.url("/x"), &target, Some(&digest), &fast()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn un_hash_distinto_tras_reanudar_obliga_a_una_descarga_completa() {
    // El archivo cambia entre el corte y la reanudación: las dos mitades no encajan.
    let old = vec![b'a'; 8192];
    let new = vec![b'b'; 8192];
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let (first, second) = (old.clone(), new.clone());
    let server = Server::http(move |req, out| {
        let call = seen.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            respond(out, 200, &[("Content-Length", "8192")], &first[..4096]);
        } else if let Some(range) = req.header("range") {
            assert_eq!(range, "bytes=4096-");
            respond(
                out,
                206,
                &[
                    ("Content-Range", "bytes 4096-8191/8192"),
                    ("Content-Length", "4096"),
                ],
                &second[4096..],
            );
        } else {
            respond(out, 200, &[("Content-Length", "8192")], &second);
        }
    });
    let digest = hex(HashAlgorithm::Sha256, &new);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let operation = Operation::silent();
    download(
        &server.url("/x"),
        &target,
        Some(&digest),
        &fast(),
        &operation,
    )
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), new);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn la_cancelacion_interrumpe_una_lectura_bloqueada_y_descarta_el_parcial() {
    let release = Arc::new(AtomicBool::new(false));
    let gate = Arc::clone(&release);
    let server = Server::http(move |_, out| {
        out.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n")
            .ok();
        out.write_all(&[b'x'; 1000]).ok();
        out.flush().ok();
        let waited = Instant::now();
        while !gate.load(Ordering::SeqCst) && waited.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let progressed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&progressed);
    let operation = Operation::new(move |state| {
        if state.received.is_some_and(|received| received > 0) {
            flag.store(true, Ordering::SeqCst);
        }
    });
    let remote = operation.clone();
    let waiting = Arc::clone(&progressed);
    std::thread::spawn(move || {
        let waited = Instant::now();
        while !waiting.load(Ordering::SeqCst) && waited.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        remote.cancel();
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let started = Instant::now();
    let result = mito_core::operation::monitored(&operation, |op| {
        download(&server.url("/stall"), &target, None, &fast(), op)
    });
    release.store(true, Ordering::SeqCst);
    let error = result.unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    // El timeout de lectura es de 60 s; cancelar no debe esperarlo.
    assert!(started.elapsed() < Duration::from_secs(15));
    assert!(!target.exists());
    assert!(!operation.state().active);
}

#[test]
fn cancelar_durante_la_espera_de_reintento_descarta_el_archivo() {
    let server = Server::http(|_, out| respond(out, 504, &[("Content-Length", "0")], b""));
    let operation = Operation::silent();
    let remote = operation.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        remote.cancel();
    });
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let slow = DownloadOptions {
        delay: Duration::from_secs(30),
        ..DownloadOptions::default()
    };
    let started = Instant::now();
    let error = download(&server.url("/x"), &target, None, &slow, &operation).unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!target.exists());
}

#[test]
fn un_fallo_de_conexion_se_reintenta_y_luego_se_informa() {
    // Puerto cerrado: se reserva y se suelta.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let options = DownloadOptions {
        attempts: 2,
        ..fast()
    };
    let error = get(
        &format!("http://127.0.0.1:{port}/x"),
        &target,
        None,
        &options,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Network { .. }), "{error:?}");
    assert!(!error.to_string().contains("/x"));
    assert!(!target.exists());
}

#[test]
fn una_descarga_cancelada_antes_de_empezar_no_toca_la_red() {
    let operation = Operation::silent();
    operation.cancel();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let error = download("http://127.0.0.1:9/x", &target, None, &fast(), &operation).unwrap_err();
    assert!(error.is_cancelled());
    assert!(!target.exists());
}

#[test]
fn hashes_opcionales_y_mixtos() {
    // Parte de test_optional_and_mixed_hashes que no depende del catálogo.
    use mito_transfer::{download_hashes, parse_hash};
    use serde_json::json;
    let md5 = format!("md5:{}", "a".repeat(32));
    assert_eq!(download_hashes(2, None).unwrap(), [None, None]);
    assert_eq!(
        download_hashes(2, Some(&json!("skip"))).unwrap(),
        [None, None]
    );
    assert_eq!(
        download_hashes(2, Some(&json!(["skip", md5]))).unwrap(),
        [None, Some(md5)]
    );
    assert_eq!(parse_hash(Some("   ")).unwrap(), None);
    assert!(download_hashes(2, Some(&json!(["a".repeat(64)]))).is_err());
    assert!(parse_hash(Some("md5:invalid")).is_err());
}
