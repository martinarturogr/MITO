//! Escritura de entradas con límite de bytes reales.
//!
//! Corrección deliberada respecto al original (ADR 0008, punto 1): `extract_zip` solo
//! validaba el tamaño *declarado* de las entradas y copiaba sin límite, de modo que una
//! «bomba» que declara poco y expande mucho podía escribir más de `MAX_EXPANDED`. Aquí
//! cada byte escrito se descuenta del presupuesto.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use mito_core::Message;

use crate::error::{Error, Result};

const BUFFER: usize = 64 * 1024;

pub(crate) struct Sink<'a> {
    archive: &'a Path,
    destination: &'a Path,
    /// ¿Existía `destination` antes de empezar a escribir?
    existed: bool,
    written: Vec<std::path::PathBuf>,
    remaining: u64,
    too_large: &'static str,
}

impl<'a> Sink<'a> {
    /// `too_large` es el mensaje del formato para «demasiados bytes reales».
    pub fn new(
        archive: &'a Path,
        destination: &'a Path,
        max_expanded: u64,
        too_large: &'static str,
    ) -> Self {
        Self {
            archive,
            destination,
            existed: destination.exists(),
            written: Vec::new(),
            remaining: max_expanded,
            too_large,
        }
    }

    pub fn dir(target: &Path) -> Result<()> {
        fs::create_dir_all(target).map_err(|e| mito_core::Error::io(target, e).into())
    }

    /// Escribe `reader` en `target`, creando las carpetas intermedias.
    pub fn file(&mut self, target: &Path, reader: &mut dyn Read) -> Result<()> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| mito_core::Error::io(parent, e))?;
        }
        let mut output = File::create(target).map_err(|e| mito_core::Error::io(target, e))?;
        self.written.push(target.to_path_buf());
        let mut buffer = vec![0_u8; BUFFER];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|e| mito_core::Error::io(self.archive, e))?;
            if read == 0 {
                return Ok(());
            }
            let read_bytes = read as u64;
            if read_bytes > self.remaining {
                return Err(Error::domain(Message::new(self.too_large)));
            }
            self.remaining -= read_bytes;
            output
                .write_all(&buffer[..read])
                .map_err(|e| mito_core::Error::io(target, e))?;
        }
    }

    /// Deshace lo escrito tras un fallo: borra el destino si lo creó esta extracción o,
    /// si ya existía, solo los archivos que escribió. Es un esfuerzo: los errores de
    /// borrado no sustituyen al error original.
    pub fn rollback(&self) {
        if self.existed {
            for path in &self.written {
                let _ = fs::remove_file(path);
            }
        } else {
            let _ = fs::remove_dir_all(self.destination);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_bytes_reales_se_limitan_aunque_la_entrada_declare_poco() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let archive = dir.path().join("a.zip");
        let mut sink = Sink::new(&archive, &out, 100, "Archivo demasiado grande para extraer");
        // Dos entradas que juntas superan el presupuesto de 100 bytes.
        sink.file(&out.join("a"), &mut std::io::repeat(1).take(60))
            .unwrap();
        let error = sink
            .file(&out.join("dir/b"), &mut std::io::repeat(1).take(60))
            .unwrap_err();
        assert_eq!(error.to_string(), "Archivo demasiado grande para extraer");
        sink.rollback();
        // El destino lo creó la extracción, así que desaparece entero.
        assert!(!out.exists());
    }

    #[test]
    fn si_el_destino_ya_existia_solo_se_borra_lo_escrito() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(out.join("previo.txt"), b"keep").unwrap();
        let archive = dir.path().join("a.zip");
        let mut sink = Sink::new(&archive, &out, 5, "demasiado");
        assert!(
            sink.file(&out.join("nuevo.txt"), &mut std::io::repeat(1).take(10))
                .is_err()
        );
        sink.rollback();
        assert!(out.join("previo.txt").is_file());
        assert!(!out.join("nuevo.txt").exists());
    }
}
