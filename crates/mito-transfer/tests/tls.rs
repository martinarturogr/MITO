//! Descargas HTTPS con un certificado no verificable (`tests/fixtures/tls_cert.pem` del
//! original) y redirección de HTTPS a HTTP.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Server, ok, redirect};
use mito_core::Operation;
use mito_transfer::hash::{HashAlgorithm, Hasher};
use mito_transfer::{DownloadOptions, Error, download};

fn sha256(data: &[u8]) -> String {
    let mut hasher = Hasher::new(HashAlgorithm::Sha256);
    hasher.update(data);
    hasher.hex_digest()
}

fn fast() -> DownloadOptions {
    DownloadOptions {
        delay: Duration::ZERO,
        ..DownloadOptions::default()
    }
}

fn recording() -> (Operation, Arc<Mutex<Vec<String>>>) {
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notes);
    let operation = Operation::new(move |state| {
        let mut notes = sink.lock().unwrap();
        let note = state.note.to_string();
        if !note.is_empty() && notes.last() != Some(&note) {
            notes.push(note);
        }
    });
    (operation, notes)
}

#[test]
fn reintenta_sin_verificar_el_certificado() {
    let payload: &'static [u8] = b"MITO TLS payload";
    let server = Server::https(move |_, out| ok(out, &[], payload));
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    let url = server.url("/payload");

    // Con hash declarado.
    let (operation, notes) = recording();
    download(&url, &target, Some(&sha256(payload)), &fast(), &operation).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(
        *notes.lock().unwrap(),
        [
            "Aviso: certificado TLS no verificable; reintentando sin verificarlo (integridad garantizada por el hash declarado)"
        ]
    );

    // Sin hash también reintenta: prioriza funcionar en redes corporativas.
    let (operation, notes) = recording();
    download(&url, &target, None, &fast(), &operation).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert_eq!(
        *notes.lock().unwrap(),
        [
            "Aviso: certificado TLS no verificable; reintentando sin verificarlo ni comprobar la integridad (descarga sin hash)"
        ]
    );

    // Un hash incorrecto sigue descartando la descarga aunque no se verifique.
    let error = download(
        &url,
        &target,
        Some(&"0".repeat(64)),
        &fast(),
        &Operation::detached(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::Integrity { .. }), "{error:?}");
    assert!(!target.exists());
}

#[test]
fn una_redireccion_de_https_a_http_esta_permitida() {
    let payload: &'static [u8] = b"MITO redirected payload";
    let plain = Server::http(move |_, out| ok(out, &[], payload));
    let location = plain.url("/payload");
    let secure = Server::https(move |_, out| redirect(out, 302, &location));
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("download");
    download(
        &secure.url("/start"),
        &target,
        Some(&sha256(payload)),
        &fast(),
        &Operation::detached(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
}
