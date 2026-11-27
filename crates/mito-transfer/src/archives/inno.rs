//! Instaladores de Inno Setup extraídos como datos con el `innounp` privado (`inno.py`).
//!
//! Contrato: `docs/spec/modules/inno.md`. El instalador nunca se ejecuta.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mito_core::storage::inside;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Raíz por defecto de los archivos de una aplicación (`APP`).
pub const APP: &str = "{app}";

/// Constante de Inno elegida por un `extract_dir` desnudo (`inno_root`): `{app}` (sin
/// distinguir mayúsculas) o `{code_Funcion}` (sí las distingue). Cualquier otra
/// (`{sys}`, `{tmp}`, rutas, `{code:X}`) no es una raíz soportada.
pub fn inno_root(extract_dir: &str) -> Option<&str> {
    if extract_dir.to_lowercase() == APP {
        return Some(extract_dir);
    }
    let name = extract_dir.strip_prefix("{code_")?.strip_suffix('}')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some(extract_dir)
}

/// Copias numeradas por Inno (`Editor,1.exe`, `Editor,2.exe`) de `name`: `(n, nombre)`.
fn numbered(name: &str, entry: &str) -> Option<u64> {
    let (stem, suffix) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let entry = entry.to_lowercase();
    let rest = entry.strip_prefix(&format!("{},", stem.to_lowercase()))?;
    let digits = rest.strip_suffix(&suffix.to_lowercase())?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn digest(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).map_err(|e| mito_core::Error::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .map_err(|e| mito_core::Error::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_vec())
}

/// Restaura los nombres declarados (`shortcuts`, `bin`) que Inno renombró con `,N`
/// cuando todas las copias numeradas son idénticas (`restore_declared_targets`). Las
/// copias no se borran.
///
/// # Errors
///
/// `Inno: ambiguous variants for <relativo>` si las copias difieren; rutas fuera del
/// payload y errores de E/S.
pub fn restore_declared_targets(payload: &Path, targets: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for relative in targets {
        if !seen.insert(relative.as_str()) {
            continue;
        }
        let target = inside(payload, relative)?;
        let Some(parent) = target.parent().filter(|p| p.is_dir()) else {
            continue;
        };
        if target.exists() {
            continue;
        }
        let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let mut copies = Vec::new();
        for entry in fs::read_dir(parent).map_err(|e| mito_core::Error::io(parent, e))? {
            let entry = entry.map_err(|e| mito_core::Error::io(parent, e))?;
            let path = entry.path();
            let Some(n) = entry.file_name().to_str().and_then(|e| numbered(name, e)) else {
                continue;
            };
            let kind = entry
                .file_type()
                .map_err(|e| mito_core::Error::io(&path, e))?;
            if kind.is_file() && !kind.is_symlink() {
                copies.push((n, path));
            }
        }
        copies.sort();
        let consecutive = copies
            .iter()
            .enumerate()
            .all(|(index, (n, _))| *n == index as u64 + 1);
        if copies.len() < 2 || !consecutive {
            continue;
        }
        let mut hashes = BTreeSet::new();
        for (_, path) in &copies {
            hashes.insert(digest(path)?);
        }
        if hashes.len() != 1 {
            return Err(Error::untranslated(format!(
                "Inno: ambiguous variants for {relative}"
            )));
        }
        fs::copy(&copies[0].1, &target).map_err(|e| mito_core::Error::io(&target, e))?;
        if let Ok(modified) = fs::metadata(&copies[0].1).and_then(|m| m.modified())
            && let Ok(file) = fs::File::options().write(true).open(&target)
        {
            let _ = file.set_modified(modified);
        }
    }
    Ok(())
}

/// Extrae los archivos bajo `constant` (`{app}` o `{code_X}`) del instalador `archive`
/// en `destination` con `innounp` (`extract_inno`). Primero valida el listado
/// (`innounp -v`) y solo publica en `destination` si lo extraído coincide con él.
///
/// # Errors
///
/// `Inno: …` (sin traducir, como el original), `Herramienta incompleta: innounp` si
/// falta la herramienta, límites y E/S.
pub fn extract_inno(
    archive: &Path,
    destination: &Path,
    innounp: &Path,
    constant: &str,
) -> Result<()> {
    let _ = (archive, destination, innounp, constant);
    Err(Error::untranslated("Inno: extractor pendiente de F7"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_raiz_solo_acepta_app_y_code() {
        assert_eq!(inno_root("{code_GetDestDir}"), Some("{code_GetDestDir}"));
        assert_eq!(inno_root("{app}"), Some("{app}"));
        assert_eq!(inno_root("{APP}"), Some("{APP}"));
        for other in [
            "{sys}",
            "{tmp}",
            "{code_x}\\sub",
            "sub",
            "{code:GetDestDir}",
            "{CODE_x}",
            "{code_}",
            "{code_1x}",
            "",
        ] {
            assert_eq!(inno_root(other), None, "{other}");
        }
    }

    #[test]
    fn restaura_el_nombre_si_las_copias_son_identicas() {
        let folder = tempfile::tempdir().unwrap();
        let payload = folder.path();
        fs::write(payload.join("Editor,1.exe"), b"same").unwrap();
        fs::write(payload.join("Editor,2.EXE"), b"same").unwrap();
        fs::create_dir(payload.join("bin")).unwrap();
        fs::write(payload.join("bin/Tool,1.exe"), b"a").unwrap();
        fs::write(payload.join("bin/Tool,2.exe"), b"b").unwrap();
        fs::write(payload.join("Solo,1.exe"), b"x").unwrap();
        fs::write(payload.join("Gap,1.exe"), b"x").unwrap();
        fs::write(payload.join("Gap,3.exe"), b"x").unwrap();
        restore_declared_targets(
            payload,
            &[
                "Editor.exe".into(),
                "Editor.exe".into(),
                "Solo.exe".into(),
                "Gap.exe".into(),
                "missing/X.exe".into(),
            ],
        )
        .unwrap();
        assert_eq!(fs::read(payload.join("Editor.exe")).unwrap(), b"same");
        assert!(payload.join("Editor,1.exe").exists());
        assert!(!payload.join("Solo.exe").exists());
        assert!(!payload.join("Gap.exe").exists());
        let error = restore_declared_targets(payload, &["bin/Tool.exe".into()]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Inno: ambiguous variants for bin/Tool.exe"
        );
        assert!(restore_declared_targets(payload, &["../fuera.exe".into()]).is_err());
    }
}
