//! Selectores nativos de archivo y carpeta (`native_ui.py` y `choose_file`/`choose_folder`
//! de `host_apps.py` y `host_data.py`).
//!
//! Los diálogos son los de Windows Forms, ejecutados con PowerShell sin ventana de
//! consola. Las etiquetas fijas se localizan sustituyendo literales completos; las rutas
//! viajan por stdin y el resultado por stdout como JSON, nunca dentro del texto del
//! script (contrato `docs/spec/modules/native_ui.md`).

use std::io::{self, Write as _};
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `CREATE_NO_WINDOW`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Título del selector de aplicaciones del host.
pub const FILE_TITLE: &str = "MITO - Agregar aplicación del host";
/// Filtro del selector de aplicaciones del host.
pub const FILE_FILTER: &str =
    "Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)|*.exe;*.bat;*.cmd;*.ps1";
/// Título del selector de la carpeta del host.
pub const FOLDER_TITLE: &str = "MITO - Selecciona la carpeta del host para enlazar";

/// Script del selector de archivo (el texto empieza por un salto de línea, como el
/// original). Los `.ps1` se guardan con CRLF en Git: se normaliza a LF.
fn file_script() -> String {
    include_str!("scripts/choose_host_file.ps1").replace("\r\n", "\n")
}

/// Script del selector de carpeta.
fn folder_script() -> String {
    include_str!("scripts/choose_host_folder.ps1").replace("\r\n", "\n")
}

/// Errores de los selectores nativos.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NativeError {
    /// No se pudo arrancar PowerShell o hablarle.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// PowerShell terminó con error (`CalledProcessError` del original).
    #[error("PowerShell terminó con el código {code:?}: {stderr}")]
    Failed {
        /// Código de salida, si lo hubo.
        code: Option<i32>,
        /// Texto de `stderr`.
        stderr: String,
    },
    /// La salida no era el JSON de una cadena.
    #[error("respuesta no válida del selector: {0}")]
    Json(#[from] serde_json::Error),
}

/// Sustituye, en `script`, cada literal completo `'<etiqueta>'` por `'<traducción>'`.
///
/// Las comillas simples se duplican antes de buscar y al insertar, así que una
/// traducción es siempre un dato y nunca puede cerrar el literal. Las etiquetas se
/// aplican en el orden dado y se reemplazan todas sus apariciones; una etiqueta que no
/// está en el script no hace nada.
pub fn localize_script(
    script: &str,
    labels: &[&str],
    translate: &dyn Fn(&str) -> String,
) -> String {
    fn literal(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
    let mut script = script.to_owned();
    for label in labels {
        script = script.replace(&literal(label), &literal(&translate(label)));
    }
    script
}

/// Script del selector de archivo de aplicaciones del host, ya localizado.
pub fn choose_file_script(translate: &dyn Fn(&str) -> String) -> String {
    localize_script(&file_script(), &[FILE_TITLE, FILE_FILTER], translate)
}

/// Script del selector de carpeta del host, ya localizado.
pub fn choose_folder_script(translate: &dyn Fn(&str) -> String) -> String {
    localize_script(&folder_script(), &[FOLDER_TITLE], translate)
}

/// Ejecuta `script` con `powershell.exe -NoLogo -NoProfile -STA -Command`, sin ventana,
/// con `input` en stdin si lo hay, y devuelve la cadena JSON de stdout o `None` si el
/// usuario canceló (salida vacía).
///
/// # Errors
///
/// [`NativeError`] si PowerShell no arranca, falla o devuelve un JSON no válido.
pub fn run_script(script: &str, input: Option<&str>) -> Result<Option<String>, NativeError> {
    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-STA", "-Command", script])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        // Un `ReadToEnd` del script termina al cerrar stdin; un fallo aquí lo cuenta
        // el código de salida.
        let _ = stdin.write_all(text.as_bytes());
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(NativeError::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim_start_matches('\u{feff}').trim();
    if stdout.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str::<String>(stdout)?))
}

/// Abre el selector de aplicaciones del host (`choose_file`). `None` si se cancela.
///
/// # Errors
///
/// Los de [`run_script`].
pub fn choose_file(translate: &dyn Fn(&str) -> String) -> Result<Option<PathBuf>, NativeError> {
    Ok(run_script(&choose_file_script(translate), None)?.map(PathBuf::from))
}

/// Abre el selector de carpeta empezando en `initial` (`choose_folder`). `None` si se
/// cancela.
///
/// # Errors
///
/// Los de [`run_script`].
pub fn choose_folder(
    initial: &Path,
    translate: &dyn Fn(&str) -> String,
) -> Result<Option<PathBuf>, NativeError> {
    let input = initial.display().to_string();
    Ok(run_script(&choose_folder_script(translate), Some(&input))?.map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(text: &str) -> String {
        text.to_owned()
    }

    #[test]
    fn las_etiquetas_se_escapan_como_dato() {
        // `test_native_labels_are_escaped_as_data` de `test_i18n.py`.
        assert_eq!(
            localize_script("$title = 'Open'", &["Open"], &|_| "User's $folder".to_owned()),
            "$title = 'User''s $folder'"
        );
    }

    #[test]
    fn solo_se_sustituyen_literales_completos() {
        let script = "a = 'Open'; b = 'Open file'; c = Open";
        assert_eq!(
            localize_script(script, &["Open"], &|_| "Abrir".to_owned()),
            "a = 'Abrir'; b = 'Open file'; c = Open"
        );
        // Todas las apariciones, y una etiqueta ausente no hace nada.
        assert_eq!(
            localize_script("'x' 'x'", &["x", "y"], &|t| t.to_uppercase()),
            "'X' 'X'"
        );
    }

    #[test]
    fn una_etiqueta_con_comillas_se_busca_duplicada() {
        let script = "t = 'It''s'";
        assert_eq!(
            localize_script(script, &["It's"], &|_| "Es".to_owned()),
            "t = 'Es'"
        );
    }

    #[test]
    fn las_sustituciones_se_encadenan_en_orden() {
        // Si la salida de una etiqueta coincide con la siguiente, esta también aplica.
        assert_eq!(
            localize_script("'a'", &["a", "b"], &|t| if t == "a" {
                "b".to_owned()
            } else {
                "c".to_owned()
            }),
            "'c'"
        );
    }

    #[test]
    fn sin_traduccion_el_script_queda_igual() {
        assert_eq!(choose_file_script(&identity), file_script());
        assert_eq!(choose_folder_script(&identity), folder_script());
    }

    #[test]
    fn los_scripts_son_los_del_original() {
        insta::assert_snapshot!("script_selector_de_archivo", file_script());
        insta::assert_snapshot!("script_selector_de_carpeta", folder_script());
        assert!(file_script().starts_with("\n[Console]::InputEncoding"));
        assert!(!file_script().contains('\r'));
        assert!(folder_script().contains("$dialog.SelectedPath = [Console]::In.ReadToEnd()"));
    }

    #[test]
    fn el_script_de_archivo_usa_la_traduccion_ingles() {
        let english = |text: &str| match text {
            FILE_TITLE => "MITO - Add host application".to_owned(),
            other => other.to_owned(),
        };
        let script = choose_file_script(&english);
        assert!(script.contains("$dialog.Title = 'MITO - Add host application'"));
        assert!(script.contains(FILE_FILTER));
    }

    #[test]
    fn una_traduccion_con_comillas_no_rompe_el_literal() {
        let script = choose_folder_script(&|_| "Carpeta de 'Ana'; calc".to_owned());
        assert!(script.contains("$dialog.Description = 'Carpeta de ''Ana''; calc'"));
    }
}
