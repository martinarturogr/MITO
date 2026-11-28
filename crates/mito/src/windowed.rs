//! Consola de `mitow.exe`: sin ventana, con los errores en `logs/mito.log`.

use std::io::{self, Cursor, Write};

use mito_core::Layout;
use mito_core::diagnostics::{self, LogLevel};

use crate::console::Console;

/// Raíz de la instancia: la opción oculta `--root` si se pasó (aunque los argumentos no
/// se hayan podido analizar) o la deducida de `<raíz>\core\bin\mitow.exe`.
fn layout() -> Option<Layout> {
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if argument == "--root" {
            return arguments.next().map(Layout::new);
        }
        if let Some(root) = argument.to_string_lossy().strip_prefix("--root=") {
            return Some(Layout::new(root));
        }
    }
    Layout::from_executable(&std::env::current_exe().ok()?)
}

/// Registra `text` como error de `mitow.exe` en `logs/mito.log`, si se conoce la raíz.
pub fn log_error(text: &str) {
    let Some(layout) = layout() else {
        return;
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        diagnostics::write(&layout, LogLevel::Error, "mitow", line, &[]);
    }
}

/// Destino de stderr: cada vaciado envía al log las líneas escritas.
#[derive(Default)]
struct LogWriter {
    pending: Vec<u8>,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        log_error(&text);
        Ok(())
    }
}

/// Consola sin salida estándar: stdout se descarta, stderr va al log y la entrada está
/// vacía (no hay consola en la que pulsar Intro).
pub fn console() -> Console {
    Console::new(
        Box::new(io::sink()),
        Box::new(LogWriter::default()),
        Box::new(Cursor::new(Vec::new())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_destino_del_log_acumula_hasta_el_vaciado_y_acepta_texto_no_utf8() {
        // No se vacía: el log se escribe junto al ejecutable de la prueba.
        let mut writer = LogWriter::default();
        writer.write_all(b"MITO: algo \xff\n").unwrap();
        assert_eq!(writer.pending, b"MITO: algo \xff\n");
    }

    #[test]
    fn la_consola_sin_ventana_no_bloquea_en_la_entrada() {
        let console = console();
        console.line("se descarta");
        assert_eq!(console.read_line(""), None);
    }
}
