//! Punto de entrada de `mitow.exe`: el mismo MITO que `mito.exe`, sin consola (ADR 0013).
//!
//! Es el destino de los accesos directos, «Abrir con» y los protocolos, para que abrir
//! una aplicación desde Explorer no muestre ninguna ventana de consola. Los errores se
//! registran en `logs/mito.log`.
#![forbid(unsafe_code)]
#![windows_subsystem = "windows"]

fn main() {
    mito::main_entry(mito::Mode::Windowed);
}
