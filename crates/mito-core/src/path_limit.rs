//! Límite de longitud de la raíz de la instancia (`path_limit.py`).
//!
//! Windows limita las rutas a 260 caracteres salvo que `LongPathsEnabled` esté activo.
//! La carpeta de MITO debe quedar cerca de la raíz del disco para que quepan árboles
//! profundos (`node_modules`, recursos de Electron). Solo se bloquean `ui`, `start` y
//! `bootstrap`; las aplicaciones y shims ya instalados siguen funcionando.

use std::path::Path;

use crate::message::Message;

/// Longitud máxima de la raíz sin rutas largas (`MAX_ROOT`).
pub const MAX_ROOT: usize = 32;

/// Variable de entorno que solo puede subir el umbral (para pruebas).
pub const LIMIT_VARIABLE: &str = "MITO_ROOT_PATH_LIMIT";

/// Clave de `HKEY_LOCAL_MACHINE` con `LongPathsEnabled`.
const FILE_SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\FileSystem";

/// ¿Están activadas las rutas largas de Windows? Solo cuenta el valor `1`; una clave
/// ausente o ilegible es «no».
pub fn long_paths_enabled() -> bool {
    mito_win::registry::local_machine_dword(FILE_SYSTEM_KEY, "LongPathsEnabled") == Some(1)
}

/// Umbral efectivo a partir del valor de [`LIMIT_VARIABLE`]: nunca menor que
/// [`MAX_ROOT`]; un valor no numérico deja el predeterminado.
pub fn maximum_from(value: Option<&str>) -> usize {
    value
        .and_then(|text| text.trim().parse::<usize>().ok())
        .map_or(MAX_ROOT, |limit| limit.max(MAX_ROOT))
}

/// Umbral efectivo leyendo [`LIMIT_VARIABLE`] del entorno.
pub fn maximum() -> usize {
    maximum_from(std::env::var(LIMIT_VARIABLE).ok().as_deref())
}

/// Ruta absoluta de `root` sin barra final y su longitud en unidades UTF-16 (lo que
/// cuenta el límite de 260 de Windows).
pub fn normalized(root: &Path) -> (String, usize) {
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let text = dunce::simplified(&absolute).display().to_string();
    let text = text.trim_end_matches(['\\', '/']).to_owned();
    let length = text.encode_utf16().count();
    (text, length)
}

/// Mensaje de «ruta demasiado larga» para `root`, o `None` si pasa. No toca el disco.
pub fn problem_with(root: &Path, maximum: usize, long_paths: bool) -> Option<Message> {
    let (text, length) = normalized(root);
    if length <= maximum || long_paths {
        return None;
    }
    Some(
        Message::new(
            "La carpeta de MITO tiene una ruta demasiado larga ({p1} caracteres; máximo {p2}): {p3}\n\
             Windows limita las rutas a 260 caracteres y algunas aplicaciones no se podrían instalar.\n\
             Mueve la carpeta más cerca de la raíz del disco, por ejemplo C:\\mito, y vuelve a ejecutar mito.cmd.",
        )
        .arg("p1", length)
        .arg("p2", maximum)
        .arg("p3", text),
    )
}

/// Como [`problem_with`] con el umbral del entorno y `enabled` como detector de rutas
/// largas (en producción, [`long_paths_enabled`]). Solo consulta el detector si la
/// ruta supera el umbral.
pub fn problem(root: &Path, enabled: impl FnOnce() -> bool) -> Option<Message> {
    let maximum = maximum();
    let (_, length) = normalized(root);
    if length <= maximum {
        return None;
    }
    problem_with(root, maximum, enabled())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const DOWNLOADS: &str = r"C:\Users\usuario\Downloads\mito-0.3.1-windows-x64";

    fn blocked(root: &str, long: bool) -> Option<String> {
        problem_with(&PathBuf::from(root), MAX_ROOT, long).map(|m| m.to_string())
    }

    #[test]
    fn las_raices_cortas_pasan_y_las_largas_solo_fallan_sin_rutas_largas() {
        assert!(blocked(r"C:\mito", false).is_none());
        assert!(blocked(r"C:\mito-0.3.1-windows-x64\", false).is_none());
        let exact = format!(r"C:\{}", "x".repeat(MAX_ROOT - 3));
        assert_eq!(exact.len(), 32);
        assert!(blocked(&exact, false).is_none());
        let over = format!(r"C:\{}", "x".repeat(MAX_ROOT - 2));
        assert!(blocked(&over, false).is_some());
        let message = blocked(DOWNLOADS, false).unwrap();
        assert!(message.contains(DOWNLOADS));
        assert!(message.contains(&format!("({} caracteres; máximo 32)", DOWNLOADS.len())));
        assert!(blocked(DOWNLOADS, true).is_none());
    }

    #[test]
    fn el_mensaje_es_el_literal_del_original() {
        assert_eq!(
            blocked(DOWNLOADS, false).unwrap(),
            format!(
                "La carpeta de MITO tiene una ruta demasiado larga ({} caracteres; máximo 32): {DOWNLOADS}\n\
                 Windows limita las rutas a 260 caracteres y algunas aplicaciones no se podrían instalar.\n\
                 Mueve la carpeta más cerca de la raíz del disco, por ejemplo C:\\mito, y vuelve a ejecutar mito.cmd.",
                DOWNLOADS.len()
            )
        );
    }

    #[test]
    fn la_barra_final_de_ambos_tipos_no_cuenta() {
        assert!(blocked(r"C:\mito\", false).is_none());
        assert!(blocked("C:/mito/", false).is_none());
    }

    #[test]
    fn la_longitud_cuenta_unidades_utf16() {
        let (_, plain) = normalized(Path::new(r"C:\ab"));
        let (_, emoji) = normalized(Path::new("C:\\a\u{1F600}"));
        assert_eq!(plain, 5);
        assert_eq!(emoji, 6);
    }

    #[test]
    fn el_entorno_solo_puede_subir_el_umbral() {
        assert_eq!(maximum_from(Some("1000")), 1000);
        assert_eq!(maximum_from(Some(" 40 ")), 40);
        assert_eq!(maximum_from(Some("5")), 32);
        assert_eq!(maximum_from(Some("x")), 32);
        assert_eq!(maximum_from(Some("")), 32);
        assert_eq!(maximum_from(None), 32);
        assert_eq!(MAX_ROOT, 32);
    }

    #[test]
    fn el_detector_solo_se_consulta_si_la_ruta_es_larga() {
        let root = PathBuf::from(r"C:\mito");
        assert!(problem(&root, || panic!("no debe consultarse")).is_none());
        let long = PathBuf::from(DOWNLOADS);
        assert!(problem(&long, || false).is_some());
        assert!(problem(&long, || true).is_none());
    }
}
