//! El gancho de pánicos registra los errores no capturados de los hilos.
//!
//! Va en su propio binario de test porque `set_hook` es global al proceso.

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

use mito_core::Layout;
use mito_core::error_tracking::install_panic_hook;
use serde_json::Value;

static PREVIOUS_CALLED: AtomicBool = AtomicBool::new(false);

#[test]
fn un_panico_de_hilo_se_registra_y_se_llama_al_gancho_anterior() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    std::panic::set_hook(Box::new(|_| PREVIOUS_CALLED.store(true, Ordering::SeqCst)));
    install_panic_hook(layout.clone());

    let worker = std::thread::spawn(|| {
        panic!("fallo password=SECRET en el hilo");
    });
    assert!(worker.join().is_err());

    let text = fs::read_to_string(layout.logs().join("mito.log")).unwrap();
    assert!(!text.contains("SECRET"));
    let record: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(record["level"], "error");
    assert_eq!(record["operation"], "thread.unhandled");
    assert_eq!(record["message"], "fallo password=[redacted] en el hilo");
    let exception = &record["exceptions"][0];
    assert_eq!(exception["type"], "Panic");
    assert_eq!(exception["mechanism"]["handled"], false);
    let frame = &exception["stacktrace"]["frames"][0];
    assert_eq!(frame["filename"], "crates/mito-core/tests/panic_hook.rs");
    assert_eq!(frame["in_app"], true);
    assert!(PREVIOUS_CALLED.load(Ordering::SeqCst));
}
