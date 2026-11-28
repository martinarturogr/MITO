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

use mito_core::Layout;
use mito_core::diagnostics::{self, LogLevel};
use mito_engine::network::Credentials;
use serde::Deserialize;
use zeroize::Zeroizing;

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

/// Texto de [`NativeError::Failed`]: el código (o `?`) y `stderr` si lo hay.
fn failed_text(code: Option<i32>, stderr: &str) -> String {
    let code = code.map_or_else(|| "?".to_owned(), |code| code.to_string());
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("PowerShell terminó con el código {code}")
    } else {
        format!("PowerShell terminó con el código {code}: {stderr}")
    }
}

/// Errores de los selectores nativos.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NativeError {
    /// No se pudo arrancar PowerShell o hablarle.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// PowerShell terminó con error (`CalledProcessError` del original).
    #[error("{}", failed_text(*.code, .stderr))]
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
/// con `input` en stdin si lo hay, y devuelve la salida estándar sin analizar o `None`
/// si está vacía (el usuario canceló). El texto se borra de memoria al soltarse: puede
/// llevar una contraseña.
///
/// # Errors
///
/// [`NativeError::Io`] si PowerShell no arranca y [`NativeError::Failed`] si termina con
/// error.
pub fn run_raw(
    script: &str,
    input: Option<&str>,
) -> Result<Option<Zeroizing<String>>, NativeError> {
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
    let stdout = Zeroizing::new(output.stdout);
    if !output.status.success() {
        return Err(NativeError::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let text = Zeroizing::new(String::from_utf8_lossy(&stdout).into_owned());
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(Zeroizing::new(trimmed.to_owned())))
}

/// Ejecuta `script` (ver [`run_raw`]) y devuelve la cadena JSON de stdout o `None` si el
/// usuario canceló (salida vacía).
///
/// # Errors
///
/// [`NativeError`] si PowerShell no arranca, falla o devuelve un JSON no válido.
pub fn run_script(script: &str, input: Option<&str>) -> Result<Option<String>, NativeError> {
    match run_raw(script, input)? {
        None => Ok(None),
        Some(text) => Ok(Some(serde_json::from_str::<String>(&text)?)),
    }
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

/// Título del diálogo de credenciales del portal.
pub const CREDENTIALS_TITLE: &str = "MITO - Autenticación de red";
/// Etiqueta del diálogo de credenciales.
pub const CREDENTIALS_LABEL: &str =
    "Usuario y contraseña del portal. Cancelar continúa sin conexión.";
/// Etiqueta del diálogo tras un intento fallido.
pub const CREDENTIALS_FAILED: &str =
    "No se pudo verificar Internet. Reintenta o cancela para continuar.";
/// Botón de aceptar del diálogo de credenciales.
pub const CREDENTIALS_CONNECT: &str = "Conectar";
/// Botón de cancelar del diálogo de credenciales.
pub const CREDENTIALS_CANCEL: &str = "Cancelar";

/// Script del diálogo de credenciales (el texto empieza por un salto de línea, como el
/// original).
fn credentials_text() -> String {
    include_str!("scripts/credentials.ps1").replace("\r\n", "\n")
}

/// Script del diálogo de credenciales del portal (`prompt_credentials`), ya localizado.
/// Con `failed` la etiqueta pide reintentar en lugar de describir el formulario.
pub fn credentials_script(failed: bool, translate: &dyn Fn(&str) -> String) -> String {
    let mut script = credentials_text();
    if failed {
        script = script.replace(CREDENTIALS_LABEL, CREDENTIALS_FAILED);
    }
    localize_script(
        &script,
        &[
            CREDENTIALS_TITLE,
            CREDENTIALS_LABEL,
            CREDENTIALS_FAILED,
            CREDENTIALS_CONNECT,
            CREDENTIALS_CANCEL,
        ],
        translate,
    )
}

/// Forma de la salida del diálogo.
#[derive(Deserialize)]
struct RawCredentials {
    username: String,
    password: String,
}

/// Convierte la salida JSON del diálogo en [`Credentials`]. El texto intermedio y la
/// contraseña de la estructura se borran al soltarse.
fn parse_credentials(output: &str) -> Option<Credentials> {
    let raw: RawCredentials = serde_json::from_str(output).ok()?;
    let RawCredentials { username, password } = raw;
    Some(Credentials::new(username, password))
}

/// Resultado de ejecutar el script del diálogo (inyectable en pruebas).
type DialogRun<'a> = &'a dyn Fn(&str) -> Result<Option<Zeroizing<String>>, NativeError>;

/// [`prompt_credentials`] con el ejecutor del script inyectado y el registro opcional
/// de fallos: `None` si se cancela, PowerShell falla o su salida no es el JSON esperado.
/// Un fallo se registra como `network.credentials` si se conoce la instancia.
pub fn prompt_credentials_with(
    layout: Option<&Layout>,
    failed: bool,
    translate: &dyn Fn(&str) -> String,
    run: DialogRun<'_>,
) -> Option<Credentials> {
    let script = credentials_script(failed, translate);
    let text = match run(&script) {
        Ok(Some(text)) => text,
        Ok(None) => return None,
        Err(error) => {
            log_dialog_failure(layout, &error.to_string());
            return None;
        }
    };
    let credentials = parse_credentials(&text);
    if credentials.is_none() {
        // No se cita el texto: puede contener la contraseña.
        log_dialog_failure(layout, "respuesta no válida del diálogo de credenciales");
    }
    credentials
}

/// Registra el fallo del diálogo, sin credenciales.
fn log_dialog_failure(layout: Option<&Layout>, text: &str) {
    if let Some(layout) = layout {
        diagnostics::write(layout, LogLevel::Warning, "network.credentials", text, &[]);
    }
}

/// Diálogo de credenciales de Windows Forms (`prompt_credentials`), con el script
/// literal del original y las etiquetas localizadas. `None` si se cancela o PowerShell
/// falla (sin distinguir ambos casos, como el original).
pub fn prompt_credentials(failed: bool, translate: &dyn Fn(&str) -> String) -> Option<Credentials> {
    prompt_credentials_with(None, failed, translate, &|script| run_raw(script, None))
}

/// Igual que [`prompt_credentials`], y registra en `logs/mito.log` un fallo de
/// PowerShell (Duda D10 del contrato).
pub fn prompt_credentials_in(
    layout: &Layout,
    failed: bool,
    translate: &dyn Fn(&str) -> String,
) -> Option<Credentials> {
    prompt_credentials_with(Some(layout), failed, translate, &|script| {
        run_raw(script, None)
    })
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
            localize_script("$title = 'Open'", &["Open"], &|_| "User's $folder"
                .to_owned()),
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

#[cfg(test)]
mod credentials_tests {
    use super::*;

    fn identity(text: &str) -> String {
        text.to_owned()
    }

    fn english(text: &str) -> String {
        match text {
            CREDENTIALS_TITLE => "MITO - Network authentication".to_owned(),
            CREDENTIALS_LABEL => "Portal user and password. Cancel to continue offline.".to_owned(),
            CREDENTIALS_FAILED => "Internet could not be verified. Retry or cancel.".to_owned(),
            CREDENTIALS_CONNECT => "Connect".to_owned(),
            CREDENTIALS_CANCEL => "Cancel".to_owned(),
            other => other.to_owned(),
        }
    }

    #[test]
    fn el_script_de_credenciales_es_el_del_original() {
        let script = credentials_script(false, &identity);
        insta::assert_snapshot!("script_de_credenciales", script);
        assert!(script.starts_with("\nAdd-Type -AssemblyName System.Windows.Forms\n"));
        assert!(!script.contains('\r'));
        assert!(script.contains("$p.UseSystemPasswordChar = $true"));
        assert!(script.contains("-eq 'OK'"));
    }

    #[test]
    fn tras_un_fallo_la_etiqueta_pide_reintentar() {
        let script = credentials_script(true, &identity);
        assert!(script.contains(&format!("$l.Text = '{CREDENTIALS_FAILED}'")));
        assert!(!script.contains(CREDENTIALS_LABEL));
        let script = credentials_script(false, &identity);
        assert!(script.contains(&format!("$l.Text = '{CREDENTIALS_LABEL}'")));
        assert!(!script.contains(CREDENTIALS_FAILED));
    }

    #[test]
    fn las_cinco_etiquetas_se_localizan() {
        let script = credentials_script(false, &english);
        assert!(script.contains("$f.Text = 'MITO - Network authentication'"));
        assert!(
            script.contains("$l.Text = 'Portal user and password. Cancel to continue offline.'")
        );
        assert!(script.contains("$ok.Text = 'Connect'"));
        assert!(script.contains("$cancel.Text = 'Cancel'"));
        let failed = credentials_script(true, &english);
        assert!(failed.contains("$l.Text = 'Internet could not be verified. Retry or cancel.'"));
    }

    #[test]
    fn una_traduccion_con_comillas_no_rompe_el_literal() {
        let script = credentials_script(false, &|text| {
            if text == CREDENTIALS_CONNECT {
                "It's; calc".to_owned()
            } else {
                text.to_owned()
            }
        });
        assert!(script.contains("$ok.Text = 'It''s; calc'"));
    }

    #[test]
    fn la_salida_json_se_convierte_en_credenciales() {
        let ok = |_: &str| {
            Ok(Some(Zeroizing::new(
                r#"{"username":"ana","password":"s3cr3t"}"#.to_owned(),
            )))
        };
        let credentials = prompt_credentials_with(None, false, &identity, &ok).unwrap();
        assert_eq!(credentials.username, "ana");
        assert_eq!(credentials.password.as_str(), "s3cr3t");
        assert!(!format!("{credentials:?}").contains("s3cr3t"));
    }

    #[test]
    fn cancelar_devuelve_none_sin_registrar() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let cancel = |_: &str| Ok(None);
        assert!(prompt_credentials_with(Some(&layout), false, &identity, &cancel).is_none());
        assert!(!layout.logs().join("mito.log").exists());
    }

    #[test]
    fn un_fallo_de_powershell_devuelve_none_y_se_registra_sin_la_salida() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let failing = |_: &str| {
            Err(NativeError::Failed {
                code: Some(1),
                stderr: "boom".to_owned(),
            })
        };
        assert!(prompt_credentials_with(Some(&layout), true, &identity, &failing).is_none());
        let garbage = |_: &str| Ok(Some(Zeroizing::new(r#"{"password":"hunter2"#.to_owned())));
        assert!(prompt_credentials_with(Some(&layout), false, &identity, &garbage).is_none());
        // Sin raíz no hay dónde registrar, y tampoco falla.
        assert!(prompt_credentials_with(None, false, &identity, &failing).is_none());
        let log = std::fs::read_to_string(layout.logs().join("mito.log")).unwrap();
        let records: Vec<serde_json::Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let messages: Vec<&str> = records
            .iter()
            .map(|r| r["message"].as_str().unwrap())
            .collect();
        assert_eq!(
            messages,
            [
                "PowerShell terminó con el código 1: boom",
                "respuesta no válida del diálogo de credenciales"
            ]
        );
        assert!(
            records
                .iter()
                .all(|r| r["operation"] == "network.credentials")
        );
        assert!(!log.contains("hunter2"), "{log}");
    }

    #[test]
    fn una_salida_sin_los_dos_campos_no_es_valida() {
        assert!(parse_credentials(r#"{"username":"ana"}"#).is_none());
        assert!(parse_credentials("no es json").is_none());
    }
}

#[cfg(test)]
mod failure_text_tests {
    use super::*;

    #[test]
    fn el_fallo_de_powershell_muestra_el_codigo_sin_option() {
        let error = NativeError::Failed {
            code: Some(-1),
            stderr: String::new(),
        };
        assert_eq!(error.to_string(), "PowerShell terminó con el código -1");
        let error = NativeError::Failed {
            code: None,
            stderr: " sin permiso \n".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "PowerShell terminó con el código ?: sin permiso"
        );
    }
}
