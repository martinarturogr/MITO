//! Hooks del catálogo con PowerShell real (`test_engine.py`, `test_installer_files.py` y
//! `test_bucket_resources.py` del original). Cada prueba monta una instancia temporal con
//! un descargador falso y `PowerShellHooks`, que vuelve a este mismo `mito.exe` para los
//! helpers `Expand-*`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "los helpers de tests de integración pueden abortar"
)]

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mito_core::Operation;
use mito_engine::{Downloader, Engine, Extensions, InstallOptions, LogLine, PowerShellHooks};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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

type Served = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Sirve lo registrado para la URL o un ZIP con `tool.exe`.
#[derive(Clone, Default)]
struct Fake {
    served: Served,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Downloader for Fake {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        _expected: Option<&str>,
        _operation: &Operation,
    ) -> mito_transfer::Result<()> {
        self.hits.lock().unwrap().push(url.to_owned());
        let bytes = self
            .served
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| zip_of(&[("tool.exe", url.as_bytes())]));
        fs::write(destination, bytes).map_err(|e| mito_core::Error::io(destination, e))?;
        Ok(())
    }
}

struct Hx {
    /// Se conserva para borrar la carpeta temporal al terminar.
    _dir: tempfile::TempDir,
    engine: Engine,
    fake: Fake,
    logs: Mutex<Vec<String>>,
}

fn hx() -> Hx {
    // Con espacios, como el test original, para probar rutas con espacios en el contexto.
    let dir = tempfile::Builder::new()
        .prefix("mito installer spaces ")
        .tempdir()
        .unwrap();
    let fake = Fake::default();
    let extensions = Extensions {
        downloader: Box::new(fake.clone()),
        hooks: Box::new(PowerShellHooks::new(env!("CARGO_BIN_EXE_mito"))),
        ..Extensions::default()
    };
    // Raíz con nombres largos: en la CI la carpeta temporal llega con nombre corto 8.3
    // (`RUNNER~1`) y PowerShell devuelve las rutas con el nombre largo.
    let root = dunce::canonicalize(dir.path()).unwrap();
    let engine = Engine::with_extensions(&root, extensions);
    engine.initialize().unwrap();
    Hx {
        _dir: dir,
        engine,
        fake,
        logs: Mutex::new(Vec::new()),
    }
}

/// 7-Zip real: `MITO_TEST_7Z` o el instalado en el sistema.
fn real_7zip() -> Option<PathBuf> {
    [
        std::env::var_os("MITO_TEST_7Z").map(PathBuf::from),
        Some(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

impl Hx {
    fn root(&self) -> &Path {
        self.engine.root()
    }

    fn bucket(&self, apps: &Value) {
        let path = self.root().join("core").join("buckets").join("test.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&json!({"apps": apps})).unwrap()).unwrap();
    }

    fn serve(&self, url: &str, bytes: Vec<u8>) {
        self.fake
            .served
            .lock()
            .unwrap()
            .insert(url.to_owned(), bytes);
    }

    /// Instala `test/tool` con el manifiesto dado y devuelve la carpeta del payload.
    fn install(&self, manifest: &Value) -> mito_engine::Result<PathBuf> {
        self.bucket(&json!({"tool": manifest}));
        let yes = |_: &[String]| true;
        let options = InstallOptions {
            confirm_unverified: Some(&yes),
            ..InstallOptions::default()
        };
        let log = |line: LogLine| self.logs.lock().unwrap().push(line.message.to_string());
        self.engine
            .install(&["test/tool"], &options, &log, &Operation::detached())?;
        Ok(self
            .root()
            .join("core")
            .join("apps")
            .join("test")
            .join("tool"))
    }

    fn logged(&self) -> Vec<String> {
        self.logs.lock().unwrap().clone()
    }

    /// Copia el 7-Zip real a `core/tools/7zip`. `false` si no hay uno en el sistema.
    fn with_7zip(&self) -> bool {
        let Some(exe) = real_7zip() else {
            eprintln!("omitido: no hay 7-Zip (define MITO_TEST_7Z)");
            return false;
        };
        let folder = self.engine.layout().sevenzip_exe();
        let folder = folder.parent().unwrap();
        fs::create_dir_all(folder).unwrap();
        for name in ["7z.exe", "7z.dll"] {
            fs::copy(exe.with_file_name(name), folder.join(name)).unwrap();
        }
        true
    }
}

fn manifest(extra: &Value) -> Value {
    let mut base = json!({"version": "1", "url": "https://example.org/tool.zip",
                          "hash": ZERO_HASH, "bin": "tool.exe"});
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn index_of(lines: &[String], text: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(text))
        .unwrap_or_else(|| panic!("falta `{text}` en {lines:#?}"))
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap();
    serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap()
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect(),
        Value::String(text) => vec![text.clone()],
        Value::Null => Vec::new(),
        other => panic!("no es texto ni lista: {other}"),
    }
}

#[test]
fn pre_install_se_ejecuta_y_post_install_se_omite() {
    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Set-Content \"$dir/prepared.txt\" ready",
            "post_install": "Set-Content \"$dir/never.txt\" never",
        })))
        .unwrap();
    assert!(payload.join("prepared.txt").is_file());
    assert!(!payload.join("never.txt").exists());
    let logs = h.logged();
    assert!(logs.contains(&"test/tool: ejecutando pre_install".to_owned()));
    assert!(
        logs.contains(
            &"test/tool: post_install omitido por la política general de MITO".to_owned()
        )
    );
}

#[test]
fn el_orden_el_contexto_y_la_arquitectura_son_los_del_original() {
    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Add-Content \"$dir/order.txt\" pre",
            "installer": {"script": [
                "Add-Content \"$dir/order.txt\" script",
                "info 'hola desde info'",
                "warn 'cuidado'",
                "@{fname=$fname; arch=$architecture; global=$global; original=$original_dir; version=$manifest.version; app=$app; bucket=$bucket; cwd=(Get-Location).Path} | ConvertTo-Json -Compress | Set-Content \"$dir/ctx.json\"",
            ]},
        })))
        .unwrap();
    let order = fs::read_to_string(payload.join("order.txt")).unwrap();
    assert_eq!(order.lines().collect::<Vec<_>>(), ["pre", "script"]);
    let ctx = read_json(&payload.join("ctx.json"));
    assert_eq!(ctx["fname"], "tool.zip");
    assert_eq!(ctx["arch"], "64bit");
    assert_eq!(ctx["global"], false);
    assert_eq!(ctx["version"], "1");
    assert_eq!(
        (ctx["app"].as_str(), ctx["bucket"].as_str()),
        (Some("tool"), Some("test"))
    );
    // `$dir` es el payload en staging y es la carpeta de trabajo.
    assert_eq!(ctx["original"], ctx["cwd"]);
    assert!(ctx["original"].as_str().unwrap().ends_with("payload"));
    let logs = h.logged();
    assert!(
        index_of(&logs, "ejecutando pre_install") < index_of(&logs, "ejecutando installer.script")
    );
    assert!(index_of(&logs, "ejecutando installer.script") < index_of(&logs, "Validando"));
    assert!(logs.contains(&"test/tool: hola desde info".to_owned()));
    assert!(logs.contains(&"test/tool: Aviso: cuidado".to_owned()));
}

#[test]
fn el_entorno_del_hook_apunta_a_home_y_se_limpia() {
    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "@{home=$env:USERPROFILE; h=$env:HOME; app=$env:APPDATA; local=$env:LOCALAPPDATA; temp=$env:TEMP; tmp=$env:TMP; root=$env:MITO_ROOT; profile=$env:MITO_PROFILE; active=$env:MITO_ACTIVE_VARS; drive=$env:HOMEDRIVE; hp=$env:HOMEPATH} | ConvertTo-Json -Compress | Set-Content \"$dir/env.json\"",
        })))
        .unwrap();
    let env = read_json(&payload.join("env.json"));
    let home = h.engine.layout().home();
    let text = |value: &Value| value.as_str().unwrap().to_owned();
    assert_eq!(text(&env["home"]), home.display().to_string());
    assert_eq!(text(&env["h"]), home.display().to_string());
    assert_eq!(
        text(&env["app"]),
        home.join("AppData").join("Roaming").display().to_string()
    );
    assert_eq!(
        text(&env["local"]),
        home.join("AppData").join("Local").display().to_string()
    );
    assert_eq!(text(&env["temp"]), home.join("tmp").display().to_string());
    assert_eq!(env["temp"], env["tmp"]);
    assert_eq!(text(&env["root"]), h.root().display().to_string());
    assert_eq!(
        format!("{}{}", text(&env["drive"]), text(&env["hp"])),
        text(&env["home"])
    );
    assert!(env["profile"].is_null());
    assert!(env["active"].is_null());
}

#[test]
fn pre_install_fallido_impide_el_instalador_y_no_publica() {
    let h = hx();
    let error = h
        .install(&manifest(&json!({
            "pre_install": "throw 'boom'",
            "installer": {"script": "Set-Content \"$dir/installer.txt\" ran"},
        })))
        .unwrap_err();
    assert_eq!(error.to_string(), "test/tool: pre_install falló (1): boom");
    assert!(
        !h.logged()
            .iter()
            .any(|l| l.contains("ejecutando installer"))
    );
    assert!(h.engine.installed().unwrap().is_empty());
}

#[test]
fn las_descargas_sin_extension_se_renombran_con_fname() {
    let h = hx();
    h.serve("https://example.org/download", b"MZ binario".to_vec());
    let payload = h
        .install(&manifest(&json!({
            "url": "https://example.org/download",
            "pre_install": "Rename-Item -LiteralPath \"$dir/$fname\" tool.exe",
        })))
        .unwrap();
    assert_eq!(fs::read(payload.join("tool.exe")).unwrap(), b"MZ binario");
}

#[test]
fn fname_es_una_lista_con_varias_url() {
    let h = hx();
    h.serve("https://example.org/a.zip", zip_of(&[("tool.exe", b"a")]));
    h.serve("https://example.org/b.zip", zip_of(&[("other.txt", b"b")]));
    let payload = h
        .install(&manifest(&json!({
            "url": ["https://example.org/a.zip", "https://example.org/b.zip"],
            "hash": [ZERO_HASH, ZERO_HASH],
            "pre_install": "@{names=@($fname); count=$fname.Count} | ConvertTo-Json -Compress | Set-Content \"$dir/names.json\"",
        })))
        .unwrap();
    let names = read_json(&payload.join("names.json"));
    assert_eq!(strings(&names["names"]), ["a.zip", "b.zip"]);
    assert_eq!(names["count"], 2);
}

#[test]
fn persist_dir_es_compartido_o_aislado() {
    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Set-Content \"$dir/persist.txt\" $persist_dir",
        })))
        .unwrap();
    let shared = fs::read_to_string(payload.join("persist.txt")).unwrap();
    let expected = h
        .root()
        .join("home")
        .join("persist")
        .join("test")
        .join("tool");
    assert_eq!(shared.trim(), expected.display().to_string());
    assert!(expected.is_dir());

    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "persist": "data",
            "shortcuts": [["tool.exe", "Tool"]],
            "pre_install": "Set-Content \"$dir/persist.txt\" $persist_dir",
        })))
        .unwrap();
    let isolated = fs::read_to_string(payload.join("persist.txt")).unwrap();
    let isolated = PathBuf::from(isolated.trim());
    assert_eq!(isolated.file_name().unwrap(), "persist");
    assert!(isolated.starts_with(h.engine.layout().staging()));
}

#[test]
fn un_hook_no_puede_dejar_enlaces_en_el_payload() {
    let h = hx();
    let error = h
        .install(&manifest(&json!({
            "pre_install": "New-Item -ItemType Junction -Path \"$dir/link\" -Target $persist_dir | Out-Null",
        })))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "test/tool: pre_install creó un enlace no gestionado: link"
    );
    assert!(h.engine.installed().unwrap().is_empty());
}

#[test]
fn el_helper_expand_7zip_extrae_en_el_payload_y_con_pipeline() {
    let h = hx();
    if !h.with_7zip() {
        return;
    }
    let work = h.root().join("work");
    fs::create_dir_all(work.join("inner")).unwrap();
    fs::write(work.join("inner").join("a.txt"), "dentro").unwrap();
    let sevenz = work.join("nested.7z");
    let output = std::process::Command::new(real_7zip().unwrap())
        .current_dir(&work)
        .args(["a", "-t7z"])
        .arg(&sevenz)
        .arg("inner")
        .output()
        .unwrap();
    assert!(output.status.success());
    let nested = fs::read(&sevenz).unwrap();
    h.serve(
        "https://example.org/tool.zip",
        zip_of(&[
            ("tool.exe", b"x"),
            ("nested.7z", &nested),
            ("piped.7z", &nested),
        ]),
    );
    let payload = h
        .install(&manifest(&json!({
            "pre_install": [
                "Expand-7zipArchive \"$dir/nested.7z\" \"$dir/out\" -ExtractDir inner -Removal",
                "Get-Item \"$dir/piped.7z\" | Expand-7zipArchive -DestinationPath \"$dir/piped\"",
            ],
        })))
        .unwrap();
    assert_eq!(
        fs::read(payload.join("out").join("a.txt")).unwrap(),
        b"dentro"
    );
    assert_eq!(
        fs::read(payload.join("piped").join("inner").join("a.txt")).unwrap(),
        b"dentro"
    );
    assert!(!payload.join("nested.7z").exists());
    assert!(payload.join("piped.7z").exists());
}

#[test]
fn los_helpers_expand_no_salen_del_payload() {
    let h = hx();
    for (script, helper) in [
        (
            "Expand-7zipArchive \"$dir/x.7z\" \"$dir/../outside\"",
            "Expand-7zipArchive",
        ),
        (
            "Expand-InnoArchive \"$dir/../x.exe\" \"$dir/out\"",
            "Expand-InnoArchive",
        ),
        (
            "Expand-DarkArchive \"$dir/x.exe\" \"$dir/../out\"",
            "Expand-DarkArchive",
        ),
        (
            "Expand-MsiArchive \"$dir/../x.msi\" \"$dir/out\"",
            "Expand-MsiArchive",
        ),
    ] {
        let error = h
            .install(&manifest(&json!({"pre_install": script})))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("test/tool: pre_install falló (1): "),
            "{error}"
        );
        assert!(
            error.contains(&format!("{helper} requiere rutas dentro del payload")),
            "{error}"
        );
        assert!(error.contains(&format!("{helper} falló")), "{error}");
    }
    assert!(h.engine.installed().unwrap().is_empty());
}

#[test]
fn inno_solo_acepta_app_o_una_funcion_code() {
    let h = hx();
    let error = h
        .install(&manifest(&json!({
            "pre_install": "Expand-InnoArchive \"$dir/x.exe\" \"$dir/out\" -ExtractDir '{sys}'",
        })))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Expand-InnoArchive: ExtractDir no encontrado"),
        "{error}"
    );
}

#[test]
fn get_helper_path_solo_resuelve_innounp() {
    let h = hx();
    let error = h
        .install(&manifest(
            &json!({"pre_install": "Get-HelperPath -Helper Otro"}),
        ))
        .unwrap_err()
        .to_string();
    assert!(error.ends_with("Helper no soportado: Otro"), "{error}");
    let error = h
        .install(&manifest(
            &json!({"pre_install": "Get-HelperPath -Helper Innounp"}),
        ))
        .unwrap_err()
        .to_string();
    assert!(
        error.ends_with("Herramienta incompleta: innounp"),
        "{error}"
    );
}

#[test]
fn invoke_external_command_propaga_el_codigo_y_vuelca_el_log() {
    let h = hx();
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Invoke-ExternalCommand -FilePath cmd.exe -ArgumentList @('/c','echo ok') -LogPath \"$dir/cmd.log\" | Out-Null",
        })))
        .unwrap();
    assert!(
        fs::read_to_string(payload.join("cmd.log"))
            .unwrap()
            .contains("ok")
    );
    let error = h
        .install(&manifest(&json!({
            "version": "2",
            "pre_install": "Invoke-ExternalCommand -FilePath cmd.exe -ArgumentList @('/c','exit 3') | Out-Null",
        })))
        .unwrap_err()
        .to_string();
    assert!(error.contains("cmd.exe falló (3)"), "{error}");
}

const PYSHIM: &str = "param($destination)
if (!(Test-Path \"$destination/prepared\")) { throw 'pre_install missing' }
Set-Content \"$destination/tool.ps1\" '& python \"$PSScriptRoot/tool.py\" @args'
@{destination=$destination; arguments=@($args); names=@($fname); home=$env:USERPROFILE} | ConvertTo-Json | Set-Content \"$destination/arguments.json\"
";

fn file_manifest(version: &str, args: &Value) -> Value {
    json!({
        "version": version,
        "url": ["https://example.org/sdk.zip", "https://example.org/pyshim.ps1"],
        "hash": [ZERO_HASH, ZERO_HASH],
        "extract_dir": "sdk",
        "bin": "tool.ps1",
        "pre_install": "Set-Content \"$dir/prepared\" ready",
        "installer": {"file": "pyshim.ps1", "args": args},
    })
}

fn serve_sdk(h: &Hx, script: &str) {
    h.serve(
        "https://example.org/sdk.zip",
        zip_of(&[("sdk/tool.py", b"# never executed")]),
    );
    h.serve("https://example.org/pyshim.ps1", script.as_bytes().to_vec());
}

#[test]
fn installer_file_recibe_los_argumentos_sustituidos() {
    let cases = [
        (json!("$dir \"two words\""), vec!["two words"]),
        (
            json!([
                "$dir",
                "",
                "two words",
                "$(throw \"injection\")",
                "${version}",
                "$global",
                "$dir_suffix"
            ]),
            vec![
                "",
                "two words",
                "$(throw \"injection\")",
                "1",
                "False",
                "$dir_suffix",
            ],
        ),
    ];
    for (args, expected) in cases {
        let h = hx();
        serve_sdk(&h, PYSHIM);
        let payload = h.install(&file_manifest("1", &args)).unwrap();
        assert_eq!(
            fs::read_to_string(payload.join("pyshim.ps1")).unwrap(),
            PYSHIM
        );
        assert_eq!(
            fs::read(payload.join("tool.py")).unwrap(),
            b"# never executed"
        );
        assert!(
            fs::read_to_string(payload.join("tool.ps1"))
                .unwrap()
                .contains("& python")
        );
        let seen = read_json(&payload.join("arguments.json"));
        // El contexto apunta al staging durante la ejecución, con espacios en la ruta.
        assert!(
            seen["destination"]
                .as_str()
                .unwrap()
                .contains("mito installer spaces ")
        );
        assert_eq!(strings(&seen["arguments"]), expected, "{args}");
        assert_eq!(strings(&seen["names"]), ["sdk.zip", "pyshim.ps1"]);
        assert_eq!(seen["home"], h.engine.layout().home().display().to_string());
    }
}

#[test]
fn los_errores_de_installer_file_conservan_la_version_instalada() {
    let h = hx();
    serve_sdk(&h, PYSHIM);
    let payload = h.install(&file_manifest("1", &json!("$dir"))).unwrap();
    let before = h.engine.installed().unwrap();
    for (script, code) in [
        ("throw \"file failed\"", 1),
        ("exit 19", 19),
        ("Remove-Item \"$dir/tool.py\"; throw \"failed\"", 1),
    ] {
        h.serve("https://example.org/pyshim.ps1", script.as_bytes().to_vec());
        let error = h
            .install(&file_manifest("2", &json!("$dir")))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("test/tool: installer.file falló ({code})")),
            "{error}"
        );
        assert_eq!(h.engine.installed().unwrap(), before);
        assert_eq!(
            fs::read(payload.join("tool.py")).unwrap(),
            b"# never executed"
        );
    }
}

#[test]
fn un_installer_file_ausente_no_se_ignora() {
    let h = hx();
    serve_sdk(&h, PYSHIM);
    let mut m = file_manifest("1", &json!("$dir"));
    m["installer"]["file"] = json!("missing.ps1");
    assert_eq!(
        h.install(&m).unwrap_err().to_string(),
        "No existe missing.ps1"
    );
    assert!(h.engine.installed().unwrap().is_empty());
}

#[test]
fn los_modos_de_installer_invalidos_se_rechazan_antes_de_descargar() {
    let h = hx();
    for installer in [
        json!({"file": "../outside.ps1"}),
        json!({"file": "C:/outside.ps1"}),
        json!({"file": "setup.exe"}),
        json!({"file": "setup.msi"}),
        json!({"file": "pyshim.ps1", "script": "x"}),
        json!({"file": "pyshim.ps1", "keep": true}),
        json!({"file": "pyshim.ps1", "args": [7]}),
        json!({"args": "$dir"}),
        json!({"file": false}),
    ] {
        let mut m = file_manifest("1", &json!("$dir"));
        m["installer"] = installer.clone();
        assert!(h.install(&m).is_err(), "{installer}");
    }
    assert!(h.fake.hits.lock().unwrap().is_empty());
}

#[test]
fn los_hooks_se_ejecutan_sin_ventana_y_registran_stderr() {
    let h = hx();
    h.install(&manifest(&json!({
        "pre_install": "[Console]::Error.WriteLine('aviso por stderr')",
    })))
    .unwrap();
    assert!(
        h.logged()
            .contains(&"test/tool: aviso por stderr".to_owned())
    );
}

fn bucket_config(h: &Hx, source: &str) {
    fs::write(
        h.engine.layout().config_file(),
        format!("bucket.test={source}\n"),
    )
    .unwrap();
}

#[test]
fn los_recursos_locales_se_copian_y_los_comparten_ambas_fases() {
    let h = hx();
    let source = h.root().join("src");
    fs::create_dir_all(source.join("scripts").join("tool")).unwrap();
    fs::write(
        source.join("scripts").join("tool").join("template.reg"),
        "fixture",
    )
    .unwrap();
    bucket_config(&h, &source.display().to_string());
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "if (!(Test-Path \"$bucketsdir/test/scripts/tool/template.reg\")) { throw 'missing' }\nSet-Content \"$dir/first.reg\" (Get-Content \"$bucketsdir/test/scripts/tool/template.reg\")",
            "installer": {"script": "Copy-Item \"$bucketsdir/test/scripts/tool/template.reg\" \"$dir/second.reg\""},
        })))
        .unwrap();
    assert_eq!(
        fs::read_to_string(payload.join("first.reg"))
            .unwrap()
            .trim(),
        "fixture"
    );
    assert_eq!(
        fs::read_to_string(payload.join("second.reg")).unwrap(),
        "fixture"
    );
    assert!(
        h.logged()
            .contains(&"test: preparando recursos del catálogo".to_owned())
    );
    assert!(
        h.engine
            .layout()
            .staging()
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn un_origen_bucket_sin_scripts_usa_la_carpeta_padre() {
    let h = hx();
    let repository = h.root().join("repo");
    fs::create_dir_all(repository.join("bucket")).unwrap();
    fs::create_dir_all(repository.join("scripts").join("tool")).unwrap();
    fs::write(repository.join("scripts").join("tool").join("x.txt"), "ok").unwrap();
    bucket_config(&h, &repository.join("bucket").display().to_string());
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Copy-Item \"$bucketsdir/test/scripts/tool/x.txt\" \"$dir/x.txt\"",
        })))
        .unwrap();
    assert_eq!(fs::read_to_string(payload.join("x.txt")).unwrap(), "ok");
}

#[test]
fn el_snapshot_remoto_se_descarga_sin_hash_y_se_extrae() {
    let h = hx();
    h.serve(
        "https://api.github.com/repos/owner/repo/zipball",
        zip_of(&[("repo-abc/scripts/tool/template.reg", b"remoto")]),
    );
    bucket_config(&h, "https://github.com/owner/repo");
    let payload = h
        .install(&manifest(&json!({
            "pre_install": "Copy-Item \"$bucketsdir/test/scripts/tool/template.reg\" \"$dir/t.reg\"",
        })))
        .unwrap();
    assert_eq!(fs::read(payload.join("t.reg")).unwrap(), b"remoto");
}

#[test]
fn un_snapshot_con_dos_raices_se_rechaza() {
    let h = hx();
    h.serve(
        "https://api.github.com/repos/owner/repo/zipball",
        zip_of(&[("a/scripts/x", b"1"), ("b/scripts/x", b"2")]),
    );
    bucket_config(&h, "https://github.com/owner/repo");
    let error = h
        .install(&manifest(&json!({
            "pre_install": "Copy-Item \"$bucketsdir/test/scripts/x\" \"$dir/x\"",
        })))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "El snapshot del catálogo debe tener una sola raíz"
    );
}

#[test]
fn un_fallo_de_recursos_conserva_la_instalacion_anterior() {
    let h = hx();
    let source = h.root().join("src");
    fs::create_dir_all(source.join("scripts")).unwrap();
    fs::write(source.join("scripts").join("a.txt"), "a").unwrap();
    bucket_config(&h, &source.display().to_string());
    let hook =
        json!({"pre_install": "Copy-Item \"$bucketsdir/test/scripts/a.txt\" \"$dir/a.txt\""});
    let payload = h.install(&manifest(&hook)).unwrap();
    let before = h.engine.installed().unwrap();
    bucket_config(&h, &h.root().join("missing").display().to_string());
    let mut next = manifest(&hook);
    next["version"] = json!("2");
    let error = h.install(&next).unwrap_err();
    assert_eq!(
        error.to_string(),
        "No hay recursos scripts válidos en el catálogo: test"
    );
    assert_eq!(h.engine.installed().unwrap(), before);
    assert!(payload.join("tool.exe").is_file());
}

#[test]
fn los_enlaces_en_los_recursos_se_rechazan() {
    let h = hx();
    let source = h.root().join("src");
    fs::create_dir_all(source.join("scripts")).unwrap();
    let target = h.root().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    mito_win::links::create_junction(&target, &source.join("scripts").join("link")).unwrap();
    bucket_config(&h, &source.display().to_string());
    let error = h
        .install(&manifest(&json!({
            "pre_install": "Write-Output $bucketsdir/test/scripts/x",
        })))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Los enlaces del catálogo no están permitidos"
    );
}

fn mito(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_mito"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn hook_expand_esta_oculto_y_falla_con_codigo_1_por_stderr() {
    let help = String::from_utf8(mito(&["--help"]).stdout).unwrap();
    assert!(!help.contains("hook-expand"), "{help}");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().display().to_string();
    let payload = dir.path().join("stage").join("payload");
    fs::create_dir_all(&payload).unwrap();
    let payload = payload.display().to_string();
    let outside = dir.path().join("x.7z").display().to_string();
    let output = mito(&[
        "hook-expand",
        "7zip",
        &root,
        &payload,
        &outside,
        &payload,
        ".",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "Expand-7zipArchive requiere rutas dentro del payload"
    );
    assert_eq!(output.stdout, Vec::<u8>::new());
    let output = mito(&[
        "hook-expand",
        "rar",
        &root,
        &payload,
        &payload,
        &payload,
        ".",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "Extractor no soportado: rar"
    );
}
