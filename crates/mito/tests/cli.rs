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
fn sin_argumentos_muestra_la_ayuda() {
    mito().assert().failure().stderr(contains("Usage"));
}
