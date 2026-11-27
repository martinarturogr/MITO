//! Pruebas de humo del binario `mito`.
#![allow(
    clippy::unwrap_used,
    reason = "los helpers de tests de integración pueden abortar"
)]

use assert_cmd::Command;
use predicates::str::contains;

fn mito() -> Command {
    Command::cargo_bin("mito").unwrap()
}

#[test]
fn muestra_la_version() {
    mito()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn sin_argumentos_abre_la_interfaz_que_aun_no_esta_disponible() {
    mito()
        .arg("--root")
        .arg(std::env::temp_dir().join("mito-sin-estado"))
        .assert()
        .failure()
        .stderr("MITO: Comando no disponible todavía: ui\n");
}
