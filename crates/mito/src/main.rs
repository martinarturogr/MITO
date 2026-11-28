//! Punto de entrada de `mito.exe`: CLI compatible con `mito.cmd` del MITO original.
#![forbid(unsafe_code)]

fn main() {
    mito::main_entry(mito::Mode::Console);
}
