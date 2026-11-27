//! Pruebas de integración del binario `mito` sobre una instancia temporal (`--root`).
//!
//! Ninguna sale a Internet: los buckets se importan desde una carpeta de fixtures y los
//! `install` descargan de un servidor HTTP local.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "los helpers de tests de integración pueden abortar"
)]

use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;

use assert_cmd::Command;
use insta::assert_snapshot;
use predicates::prelude::*;
use predicates::str::contains;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

fn mito() -> Command {
    let mut command = Command::cargo_bin("mito").unwrap();
    for variable in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(variable);
    }
    command.env("NO_PROXY", "127.0.0.1,localhost");
    // Las raíces temporales superan los 32 caracteres: el límite se sube para las pruebas.
    command.env("MITO_ROOT_PATH_LIMIT", "1000");
    command
}

/// Instancia temporal: carpeta raíz y atajos para invocar `mito --root`.
struct Instance {
    dir: TempDir,
}

impl Instance {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn mito(&self) -> Command {
        let mut command = mito();
        command.arg("--root").arg(self.root());
        command
    }

    /// Ejecuta `mito <args>` y devuelve su stdout si termina bien.
    fn ok(&self, args: &[&str]) -> String {
        let output = self
            .mito()
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(output.stdout).unwrap()
    }

    /// Importa el bucket de fixtures como `main`.
    fn import_fixtures(&self) {
        self.mito()
            .args(["bucket", "import", "main"])
            .arg(fixtures())
            .assert()
            .success();
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bucket")
}

/// ZIP en memoria con `files`.
fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Servidor HTTP local que responde a cualquier GET con `body`. Devuelve su URL base.
fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").unwrap();
            hex
        })
}

/// Carpeta de bucket con una app `tool` cuyo ZIP (con `tool.cmd`) sirve un servidor local.
fn bucket_with_tool(instance: &Instance, script: &str, hash: bool) -> PathBuf {
    let archive = zip_of(&[("tool.cmd", script.as_bytes())]);
    let base = serve(archive.clone());
    let folder = instance.root().join("source-bucket");
    fs::create_dir_all(&folder).unwrap();
    let mut manifest = json!({
        "version": "1.0",
        "url": format!("{base}/tool.zip"),
        "bin": [["tool.cmd", "tool"]],
        "shortcuts": [["tool.cmd", "Tool"]],
    });
    if hash {
        manifest["hash"] = json!(sha256(&archive));
    }
    fs::write(folder.join("tool.json"), manifest.to_string()).unwrap();
    folder
}

fn import(instance: &Instance, folder: &Path) {
    instance
        .mito()
        .args(["bucket", "import", "main"])
        .arg(folder)
        .assert()
        .success();
}

// --- pruebas de humo --------------------------------------------------------------

#[test]
fn muestra_la_version() {
    mito()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

/// La interfaz ya no es un comando pendiente: sin una terminal interactiva (aquí la
/// entrada y la salida van a tuberías) avisa de ello.
#[test]
fn sin_argumentos_y_con_ui_abren_la_interfaz_y_exigen_una_terminal() {
    let instance = Instance::new();
    for args in [&[][..], &["ui"], &["ui", "--profile", "p"]] {
        instance
            .mito()
            .args(args)
            .assert()
            .failure()
            .stderr("MITO: la interfaz necesita una terminal interactiva\n");
    }
    // Antes de abrir la interfaz se prepara el profile por defecto.
    assert!(instance.root().join("profiles").is_dir());
}

// --- errores de uso ---------------------------------------------------------------

#[test]
fn los_errores_de_uso_salen_con_2() {
    let instance = Instance::new();
    for args in [
        vec!["desconocido"],
        vec!["bucket"],
        vec!["profile"],
        vec!["install"],
        vec!["shell", "--profile", "p", "--type", "bash"],
        vec!["posthog-check"],
        vec!["run", "--profile", "p"],
    ] {
        instance
            .mito()
            .args(&args)
            .assert()
            .code(2)
            .stderr(predicates::str::is_empty().not());
    }
}

#[test]
fn release_invalido_falla_antes_de_crear_estado() {
    for args in [
        vec!["invalid"],
        vec!["mayor", "1.0.1"],
        vec!["--local", "mayor"],
        vec!["minor", "--local"],
        vec!["hotfix", "--local"],
    ] {
        let instance = Instance::new();
        instance.mito().arg("release").args(&args).assert().code(2);
        assert_eq!(
            fs::read_dir(instance.root()).unwrap().count(),
            0,
            "{args:?}"
        );
    }
    let instance = Instance::new();
    instance
        .mito()
        .args(["release", "--local", "mayor"])
        .assert()
        .code(2)
        .stderr(contains(
            "--local no admite mayor, minor ni hotfix: empaqueta la versión actual",
        ));
}

#[test]
fn los_comandos_de_otras_fases_se_analizan_pero_no_estan_disponibles() {
    let instance = Instance::new();
    for (args, name) in [
        (vec!["migrate-storage"], "migrate-storage"),
        (vec!["release", "--local", "--output", "out"], "release"),
        (
            vec!["shortcut", "--profile", "p", "--badge", "DEV"],
            "shortcut",
        ),
        (
            vec![
                "app-shortcut",
                "extras/tool",
                "--profile",
                "one",
                "--badge",
                "DEV",
                "--coupled",
                "--",
                "--name",
                "two words",
            ],
            "app-shortcut",
        ),
        (
            vec![
                "host-run",
                "host-id",
                "--profile",
                "one",
                "--owned",
                "--",
                "--port",
                "8080",
            ],
            "host-run",
        ),
        (
            vec![
                "pocketool-run",
                "--profile",
                "p",
                "--package",
                "x",
                "--entry",
                "y",
            ],
            "pocketool-run",
        ),
        (
            vec!["profile", "export", "one", "out.zip"],
            "profile export",
        ),
        (
            vec!["profile", "import", "in.zip", "--name", "two"],
            "profile import",
        ),
        (
            vec![
                "mcp-server",
                "--pipe",
                "p",
                "--owner",
                "7",
                "--session",
                "s",
            ],
            "mcp-server",
        ),
    ] {
        instance
            .mito()
            .args(&args)
            .assert()
            .code(1)
            .stdout("")
            .stderr(format!("MITO: Comando no disponible todavía: {name}\n"));
    }
}

// --- logs, installed, doctor -------------------------------------------------------

#[test]
fn logs_muestra_las_dos_carpetas_de_diagnostico_sin_escribir_el_log() {
    let instance = Instance::new();
    let output = instance.ok(&["logs"]);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "{output}");
    assert!(lines[0].ends_with("logs"), "{output}");
    assert!(lines[1].contains("MITO-logs"), "{output}");
    assert!(!instance.root().join("logs").exists());
}

#[test]
fn installed_sin_componentes_no_imprime_nada_y_prepara_la_instancia() {
    let instance = Instance::new();
    assert_eq!(instance.ok(&["installed"]), "");
    assert!(instance.root().join("home").is_dir());
    assert!(instance.root().join("core/apps").is_dir());
    // `Command started` queda registrado en el log local.
    let log = fs::read_to_string(instance.root().join("logs/mito.log")).unwrap();
    assert!(log.contains("\"operation\": \"cli.installed\""), "{log}");
}

#[test]
fn doctor_informa_de_la_instancia() {
    let instance = Instance::new();
    instance.import_fixtures();
    instance.ok(&["profile", "create", "dev"]);
    let output = instance.ok(&["doctor"]);
    assert!(output.starts_with(&format!("MITO: {}\n", env!("CARGO_PKG_VERSION"))));
    for expected in [
        "Raíz: ",
        "Longitud de la raíz: ",
        "(máximo recomendado 32)",
        "Home único: ",
        "Idioma: ",
        "Falta windows-terminal; ejecuta bootstrap",
        "Falta 7zip; ejecuta bootstrap",
        "Bucket main: sin origen configurado (snapshot de 3 manifiestos)",
        "Catálogo: 3 apps | Profiles: 1",
        "OK profile dev",
    ] {
        assert!(
            output.contains(expected),
            "falta {expected:?} en:\n{output}"
        );
    }
    assert!(!output.contains("Python") && !output.contains("Textual"));
}

#[test]
fn doctor_lee_la_version_de_las_herramientas_privadas() {
    let instance = Instance::new();
    let tool = instance.root().join("core/tools/7zip");
    fs::create_dir_all(&tool).unwrap();
    fs::write(tool.join(".mito-tool.json"), r#"{"version": "24.09"}"#).unwrap();
    let output = instance.ok(&["doctor"]);
    assert!(output.contains("\n7zip: 24.09\n"), "{output}");
    assert!(output.contains("Falta windows-terminal; ejecuta bootstrap"));
}

// --- buckets y búsqueda -------------------------------------------------------------

#[test]
fn bucket_import_cuenta_los_manifiestos() {
    let instance = Instance::new();
    instance
        .mito()
        .args(["bucket", "import", "main"])
        .arg(fixtures())
        .assert()
        .success()
        .stdout("3 manifiestos importados\n");
    assert!(instance.root().join("core/buckets/main.json").is_file());
}

#[test]
fn bucket_import_de_una_carpeta_sin_manifiestos_falla() {
    let instance = Instance::new();
    let empty = instance.root().join("vacia");
    fs::create_dir_all(&empty).unwrap();
    instance
        .mito()
        .args(["bucket", "import", "main"])
        .arg(&empty)
        .assert()
        .code(1)
        .stderr(contains("MITO: No hay manifiestos en "));
}

#[test]
fn bucket_import_rechaza_fuentes_http_que_no_son_github() {
    let instance = Instance::new();
    instance
        .mito()
        .args(["bucket", "import", "main", "https://example.org/otro/repo"])
        .assert()
        .code(1)
        .stderr("MITO: La fuente debe ser un repositorio GitHub HTTPS o una carpeta local\n");
}

#[test]
fn bucket_refresh_reimporta_los_origenes_configurados() {
    let instance = Instance::new();
    // Sin orígenes configurados no hay nada que refrescar.
    assert_eq!(instance.ok(&["bucket", "refresh"]), "");
    fs::write(
        instance.root().join("config.properties"),
        format!("bucket.local={}\n", fixtures().display()),
    )
    .unwrap();
    assert_eq!(
        instance.ok(&["bucket", "refresh"]),
        "local: 3 manifiestos\n"
    );
    assert_eq!(
        instance.ok(&["bucket", "refresh", "local"]),
        "local: 3 manifiestos\n"
    );
    instance
        .mito()
        .args(["bucket", "refresh", "otro"])
        .assert()
        .code(1)
        .stderr("MITO: Bucket no configurado\n");
}

#[test]
fn search_lista_ordena_y_marca_la_compatibilidad() {
    let instance = Instance::new();
    instance.import_fixtures();
    assert_snapshot!("search_todo", instance.ok(&["search"]));
    assert_snapshot!("search_git", instance.ok(&["search", "git"]));
    assert_snapshot!(
        "search_compatible",
        instance.ok(&["search", "--compatible"])
    );
    assert_eq!(instance.ok(&["search", "inexistente"]), "");
}

// --- profiles ---------------------------------------------------------------------

#[test]
fn profiles_crear_listar_mostrar_y_eliminar() {
    let instance = Instance::new();
    assert_snapshot!("profile_list_vacio", instance.ok(&["profile", "list"]));
    assert_eq!(
        instance.ok(&["profile", "create", "Dev"]),
        "Profile creado: dev\n"
    );
    instance.ok(&["profile", "create", "work", "--workspace", "shared"]);
    assert_snapshot!("profile_list", instance.ok(&["profile", "list"]));
    assert_snapshot!("profile_show", instance.ok(&["profile", "show", "work"]));
    assert!(instance.root().join("workspaces/shared").is_dir());
    assert_eq!(
        instance.ok(&["profile", "delete", "dev"]),
        "Profile eliminado: dev\n"
    );
    assert_eq!(instance.ok(&["profile", "list"]), "work\n");
}

#[test]
fn los_errores_de_profile_se_muestran_en_espanol_y_salen_con_1() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    for (args, message) in [
        (
            vec!["profile", "create", "dev"],
            "MITO: El profile dev ya existe\n",
        ),
        (
            vec!["profile", "delete", "dev"],
            "MITO: No se puede eliminar el último profile\n",
        ),
    ] {
        instance
            .mito()
            .args(&args)
            .assert()
            .code(1)
            .stdout("")
            .stderr(message);
    }
    instance
        .mito()
        .args(["profile", "show", "missing"])
        .assert()
        .code(1)
        .stderr(contains("MITO: "));
}

// --- install, activate, run, shell ---------------------------------------------------

#[test]
fn install_con_hash_activa_en_el_profile_y_aparece_en_installed() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    import(
        &instance,
        &bucket_with_tool(&instance, "@echo off\r\nexit /b 0\r\n", true),
    );
    instance
        .mito()
        .args(["install", "main/tool", "--profile", "dev"])
        .assert()
        .success();
    let installed = instance.ok(&["installed"]);
    assert_eq!(installed, format!("{:<65} 1.0\n", "main/tool"));
    assert_snapshot!("installed", installed);
    let profile = instance.ok(&["profile", "show", "dev"]);
    assert!(profile.contains("\"main/tool\""), "{profile}");
    assert!(
        instance
            .root()
            .join("core/apps/main/tool/tool.cmd")
            .is_file()
    );
}

#[test]
fn activate_y_deactivate_cambian_solo_el_profile() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    import(
        &instance,
        &bucket_with_tool(&instance, "@echo off\r\n", true),
    );
    instance.ok(&["install", "main/tool"]);
    assert!(
        !instance
            .ok(&["profile", "show", "dev"])
            .contains("main/tool")
    );
    assert_eq!(
        instance.ok(&["activate", "main/tool", "--profile", "dev"]),
        ""
    );
    assert!(
        instance
            .ok(&["profile", "show", "dev"])
            .contains("main/tool")
    );
    assert_eq!(
        instance.ok(&["deactivate", "main/tool", "--profile", "dev"]),
        ""
    );
    assert!(
        !instance
            .ok(&["profile", "show", "dev"])
            .contains("main/tool")
    );
    assert!(instance.ok(&["installed"]).contains("main/tool"));
    instance
        .mito()
        .args(["activate", "main/otro", "--profile", "dev"])
        .assert()
        .code(1)
        .stderr("MITO: El componente no está instalado\n");
}

#[test]
fn install_sin_hash_pregunta_y_solo_la_s_confirma() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    import(
        &instance,
        &bucket_with_tool(&instance, "@echo off\r\n", false),
    );

    // «n»: se cancela antes de descargar.
    instance
        .mito()
        .args(["install", "main/tool"])
        .write_stdin("n\n")
        .assert()
        .code(1)
        .stdout(contains(
            "Descargas sin hash: no se podrá comprobar su integridad.",
        ))
        .stdout(contains("¿Descargar e instalar? [s/N] "))
        .stderr(contains("MITO: Instalación cancelada"));
    assert_eq!(instance.ok(&["installed"]), "");

    // EOF equivale a «no».
    instance
        .mito()
        .args(["install", "main/tool"])
        .assert()
        .code(1);
    assert_eq!(instance.ok(&["installed"]), "");

    // «s»: instala.
    instance
        .mito()
        .args(["install", "main/tool"])
        .write_stdin("s\n")
        .assert()
        .success()
        .stdout(contains("¿Descargar e instalar? [s/N] "));
    assert!(instance.ok(&["installed"]).starts_with("main/tool"));
}

#[test]
fn install_de_un_componente_inexistente_falla_con_el_mensaje_del_catalogo() {
    let instance = Instance::new();
    instance.import_fixtures();
    instance
        .mito()
        .args(["install", "main/nada"])
        .assert()
        .code(1)
        .stderr(contains(
            "MITO: Componente no encontrado o ambiguo: main/nada",
        ));
}

#[test]
fn install_rechaza_un_manifiesto_incompatible() {
    let instance = Instance::new();
    instance.import_fixtures();
    instance
        .mito()
        .args(["install", "main/pwsh-module"])
        .assert()
        .code(1)
        .stderr(contains("Pasos no soportados: psmodule"));
}

#[test]
fn run_de_un_bin_devuelve_la_salida_y_el_codigo_del_comando() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    import(
        &instance,
        &bucket_with_tool(
            &instance,
            "@echo off\r\necho hola %~1 %~2\r\nexit /b 3\r\n",
            true,
        ),
    );
    instance.ok(&["install", "main/tool", "--profile", "dev"]);
    instance
        .mito()
        .args([
            "run",
            "--profile",
            "dev",
            "--app",
            "main/tool",
            "--bin",
            "tool",
        ])
        .args(["--", "uno", "dos"])
        .assert()
        .code(3)
        .stdout(contains("hola uno dos"));
}

#[test]
fn run_con_owned_o_coupled_no_esta_soportado_todavia() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    import(
        &instance,
        &bucket_with_tool(&instance, "@echo off\r\n", true),
    );
    instance.ok(&["install", "main/tool", "--profile", "dev"]);
    instance
        .mito()
        .args(["run", "--profile", "dev", "--app", "tool", "--coupled"])
        .assert()
        .code(1)
        .stderr("MITO: Modo de lanzamiento no soportado todavía: --coupled\n");
    // `--owned` pausa aunque falle, y deja constancia del código.
    instance
        .mito()
        .args(["run", "--profile", "dev", "--app", "tool", "--owned"])
        .write_stdin("\n")
        .assert()
        .code(1)
        .stdout("La aplicación terminó con el código 1.\nPulsa Intro para cerrar esta pestaña.")
        .stderr("MITO: Modo de lanzamiento no soportado todavía: --owned\n");
}

#[test]
fn run_de_una_aplicacion_no_seleccionada_falla() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    instance
        .mito()
        .args(["run", "--profile", "dev", "--app", "main/tool"])
        .assert()
        .code(1)
        .stderr("MITO: main/tool no está seleccionado en dev\n");
}

#[test]
fn shell_ejecuta_un_comando_en_el_entorno_del_profile_y_devuelve_su_codigo() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    instance
        .mito()
        .args(["shell", "--profile", "dev", "--type", "cmd", "--"])
        .args(["cmd.exe", "/c", "echo %MITO_PROFILE%& exit 5"])
        .assert()
        .code(5)
        .stdout(contains("dev"));
}

// --- ayuda -----------------------------------------------------------------------

#[test]
fn la_ayuda_de_cada_subcomando() {
    let commands: &[&[&str]] = &[
        &[],
        &["ui"],
        &["start"],
        &["bootstrap"],
        &["installed"],
        &["logs"],
        &["pocketool-run"],
        &["host-run"],
        &["doctor"],
        &["migrate-storage"],
        &["bucket"],
        &["bucket", "import"],
        &["bucket", "refresh"],
        &["search"],
        &["profile"],
        &["profile", "create"],
        &["profile", "delete"],
        &["profile", "export"],
        &["profile", "import"],
        &["profile", "list"],
        &["profile", "show"],
        &["install"],
        &["activate"],
        &["deactivate"],
        &["shell"],
        &["terminal"],
        &["run"],
        &["app-shortcut"],
        &["shortcut"],
        &["release"],
    ];
    for path in commands {
        let output = mito().args(*path).arg("--help").assert().success();
        let text = String::from_utf8(output.get_output().stdout.clone()).unwrap();
        let name = if path.is_empty() {
            "mito".to_owned()
        } else {
            format!("mito_{}", path.join("_"))
        };
        assert_snapshot!(format!("ayuda_{name}"), text);
    }
}

#[test]
fn la_ayuda_oculta_las_opciones_internas() {
    let output = mito().arg("--help").assert().success();
    let text = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(!text.contains("mcp-server"));
    assert!(!text.contains("--root"));
    let output = mito().args(["run", "--help"]).assert().success();
    let text = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(!text.contains("--owned"));
}

// --- mito.cmd ---------------------------------------------------------------------

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn mito_cmd_llama_a_core_bin_y_usa_crlf() {
    let text = fs::read(repository_file("mito.cmd")).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.lines().count() >= 4);
    for (index, line) in text.split_inclusive('\n').enumerate() {
        assert!(
            line.ends_with("\r\n"),
            "la línea {} no termina en CRLF",
            index + 1
        );
    }
    assert!(text.contains(r#""%~dp0core\bin\mito.exe" %*"#), "{text}");
    assert!(!text.to_lowercase().contains("powershell"));
    assert!(!text.to_lowercase().contains("python"));
}

/// Instala `mito.cmd` y `mito.exe` en una raíz temporal como en una instancia real.
fn installed_instance() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(r"core\bin")).unwrap();
    fs::copy(repository_file("mito.cmd"), dir.path().join("mito.cmd")).unwrap();
    fs::copy(
        assert_cmd::cargo::cargo_bin("mito"),
        dir.path().join(r"core\bin\mito.exe"),
    )
    .unwrap();
    dir
}

fn run_cmd(dir: &TempDir, args: &[&str]) -> std::process::Output {
    std::process::Command::new("cmd.exe")
        .args(["/d", "/c"])
        .arg(dir.path().join("mito.cmd"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("MITO_ROOT_PATH_LIMIT", "1000")
        .output()
        .unwrap()
}

#[test]
fn mito_cmd_deduce_la_raiz_del_ejecutable_y_propaga_el_codigo_de_salida() {
    let dir = installed_instance();
    let output = run_cmd(&dir, &["profile", "create", "dev"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Profile creado: dev"
    );
    assert!(dir.path().join("profiles/dev.json").is_file());
    let output = run_cmd(&dir, &["profile", "show", "missing"]);
    assert_eq!(output.status.code(), Some(1));
    let output = run_cmd(&dir, &["--version-invalida"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn mito_cmd_sin_argumentos_invoca_start_que_pide_preparar_las_herramientas() {
    let dir = installed_instance();
    let output = run_cmd(&dir, &[]);
    // Sin consola interactiva la bienvenida cancela (código 2) y no se crea nada.
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Bienvenido a MITO (My Integrated Terminal Orchestrator)"));
    assert!(stdout.contains("  - Windows Terminal 1.24.11911.0"));
    assert!(stdout.contains("Preparación cancelada."));
    assert!(!dir.path().join(r"core\tools").exists());
}

// --- bootstrap, start y terminal ----------------------------------------------------

/// Escribe `core/bootstrap.lock.json` con una sola herramienta `windows-terminal`.
fn write_terminal_lock(instance: &Instance, artifact_url: Option<(&str, &[u8])>) {
    let artifacts = match artifact_url {
        Some((url, bytes)) => json!([{
            "fileName": "WindowsTerminal.exe", "url": url, "sha256": sha256(bytes),
            "install": {"type": "file", "target": "WindowsTerminal.exe"}
        }]),
        None => json!([]),
    };
    let lock = json!({"schema": 1, "tools": [{
        "id": "windows-terminal", "version": "1.0",
        "required": ["WindowsTerminal.exe"], "artifacts": artifacts
    }]});
    fs::create_dir_all(instance.root().join("core")).unwrap();
    fs::write(
        instance.root().join("core/bootstrap.lock.json"),
        serde_json::to_vec_pretty(&lock).unwrap(),
    )
    .unwrap();
}

/// Instala un «Windows Terminal» de prueba (un ejecutable real que termina enseguida).
fn install_fake_terminal(instance: &Instance) {
    let folder = instance.root().join(r"core\tools\windows-terminal");
    fs::create_dir_all(&folder).unwrap();
    let system = std::env::var("SystemRoot").unwrap();
    fs::copy(
        Path::new(&system).join(r"System32\whoami.exe"),
        folder.join("WindowsTerminal.exe"),
    )
    .unwrap();
    fs::write(folder.join(".mito-tool.json"), "{\"version\": \"1.0\"}").unwrap();
}

#[test]
fn bootstrap_preview_welcome_muestra_la_bienvenida_sin_crear_nada() {
    let instance = Instance::new();
    instance
        .mito()
        .args(["bootstrap", "--preview-welcome"])
        .write_stdin("s\n")
        .assert()
        .success()
        .stdout(contains(
            "Bienvenido a MITO (My Integrated Terminal Orchestrator)",
        ))
        .stdout(contains("  - 7-Zip 26.02"))
        .stdout(contains(
            "  - innounp (extracción de instaladores Inno Setup) 2.71.1",
        ))
        .stdout(contains(
            "Vista previa terminada. No se ha descargado ni preparado ninguna herramienta.",
        ));
    assert!(!instance.root().join("core").exists());
    assert!(!instance.root().join("logs").exists());
}

#[test]
fn bootstrap_cancelado_sale_con_2_y_no_crea_herramientas() {
    let instance = Instance::new();
    write_terminal_lock(&instance, Some(("http://127.0.0.1:9/x", b"x")));
    instance
        .mito()
        .arg("bootstrap")
        .write_stdin("no\n")
        .assert()
        .code(2)
        .stdout(contains(
            "Preparación cancelada. Puedes volver a ejecutar mito.cmd cuando quieras.",
        ));
    assert!(!instance.root().join(r"core\tools").exists());
}

#[test]
fn bootstrap_aprobado_descarga_verifica_e_instala_desde_un_servidor_local() {
    let instance = Instance::new();
    let body = b"WT de prueba".to_vec();
    let base = serve(body.clone());
    write_terminal_lock(
        &instance,
        Some((&format!("{base}/WindowsTerminal.exe"), &body)),
    );
    instance
        .mito()
        .args(["bootstrap", "--approved"])
        .assert()
        .success()
        .stdout(contains("Preparando windows-terminal 1.0…"));
    let folder = instance.root().join(r"core\tools\windows-terminal");
    assert_eq!(fs::read(folder.join("WindowsTerminal.exe")).unwrap(), body);
    assert!(folder.join(".portable").is_file());
    assert_eq!(
        fs::read_to_string(folder.join(".mito-tool.json")).unwrap(),
        "{\n  \"version\": \"1.0\"\n}\n"
    );
    assert!(
        instance
            .root()
            .join("core/state/bootstrap-tools.json")
            .is_file()
    );
    // Con todo listo, otra pasada no descarga ni pregunta.
    instance
        .mito()
        .arg("bootstrap")
        .assert()
        .success()
        .stdout(contains("Bienvenido").not());
}

#[test]
fn bootstrap_con_un_hash_incorrecto_sale_con_1_y_no_publica_nada() {
    let instance = Instance::new();
    let base = serve(b"otro contenido".to_vec());
    write_terminal_lock(
        &instance,
        Some((&format!("{base}/WindowsTerminal.exe"), b"el esperado")),
    );
    instance
        .mito()
        .args(["bootstrap", "--approved"])
        .assert()
        .code(1)
        .stderr(contains("MITO: "))
        .stderr(contains("MITO: log: "));
    assert!(
        !instance
            .root()
            .join(r"core\tools\windows-terminal")
            .exists()
    );
}

#[test]
fn start_sin_herramientas_pasa_por_la_bienvenida_y_cancela_con_2() {
    let instance = Instance::new();
    instance
        .mito()
        .arg("start")
        .write_stdin("")
        .assert()
        .code(2)
        .stdout(contains("Bienvenido a MITO"));
    // El primer arranque crea el profile `default` antes de preparar nada.
    assert!(instance.root().join("profiles/default.json").is_file());
}

#[test]
fn start_con_el_terminal_listo_genera_los_ajustes_y_lo_lanza() {
    let instance = Instance::new();
    write_terminal_lock(&instance, None);
    install_fake_terminal(&instance);
    instance
        .mito()
        .args(["start", "--profile", "default"])
        .assert()
        .success();
    let settings = instance
        .root()
        .join(r"core\tools\windows-terminal\settings\settings.json");
    let data: serde_json::Value = serde_json::from_slice(&fs::read(settings).unwrap()).unwrap();
    let names: Vec<&str> = data["profiles"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["hidden"] == json!(false))
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["CMD (default)", "PowerShell (default)", "MITO"]);
    assert_eq!(
        fs::read_to_string(instance.root().join("core/state/terminal-profile.json")).unwrap(),
        "\"default\"\n"
    );
    assert!(
        instance
            .root()
            .join(r"core\assets\branding\mito.ico")
            .is_file()
    );
}

#[test]
fn start_con_un_profile_inexistente_falla() {
    let instance = Instance::new();
    write_terminal_lock(&instance, None);
    install_fake_terminal(&instance);
    instance
        .mito()
        .args(["start", "--profile", "no-existe"])
        .assert()
        .code(1)
        .stderr(contains("MITO: "));
}

#[test]
fn terminal_sin_windows_terminal_falla_con_el_mensaje_del_original() {
    let instance = Instance::new();
    instance.ok(&["profile", "create", "dev"]);
    instance
        .mito()
        .args(["terminal", "--profile", "dev"])
        .assert()
        .code(1)
        .stderr("MITO: Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap.\n");
}

#[test]
fn terminal_abre_una_pestana_del_profile_en_cmd_o_powershell() {
    let instance = Instance::new();
    install_fake_terminal(&instance);
    instance.ok(&["profile", "create", "dev"]);
    instance
        .mito()
        .args([
            "terminal",
            "--profile",
            "dev",
            "--type",
            "powershell",
            "--new",
        ])
        .assert()
        .success();
    instance
        .mito()
        .args(["terminal", "--profile", "dev"])
        .assert()
        .success();
    let data: serde_json::Value = serde_json::from_slice(
        &fs::read(
            instance
                .root()
                .join(r"core\tools\windows-terminal\settings\settings.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(data["profiles"]["list"][0]["name"], "CMD (dev)");
}

#[test]
fn start_y_bootstrap_se_bloquean_con_una_raiz_demasiado_larga() {
    if mito_core::path_limit::long_paths_enabled() {
        // Con las rutas largas de Windows activadas no hay nada que bloquear.
        return;
    }
    let instance = Instance::new();
    for name in ["start", "bootstrap"] {
        let mut command = instance.mito();
        command.env_remove("MITO_ROOT_PATH_LIMIT");
        command
            .arg(name)
            .write_stdin("\n")
            .assert()
            .code(1)
            .stderr(contains(
                "La carpeta de MITO tiene una ruta demasiado larga (",
            ));
    }
    assert!(!instance.root().join("core").exists());
}
