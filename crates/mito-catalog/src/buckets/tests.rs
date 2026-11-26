use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::json;

use super::*;
use crate::archive::tests::{Member, member, write_zip};

/// Respuesta de un servidor falso.
enum Reply {
    Bytes(Vec<u8>),
    Status(u16),
    Fail(&'static str),
    Zip(Box<dyn Fn(&Path)>),
}

/// `Fetcher` que sirve contenido preparado y registra las URL pedidas.
#[derive(Default)]
struct FakeFetcher {
    replies: BTreeMap<String, Reply>,
    calls: Mutex<Vec<String>>,
}

impl FakeFetcher {
    fn with(mut self, url: &str, reply: Reply) -> Self {
        self.replies.insert(url.to_owned(), reply);
        self
    }

    fn json(self, url: &str, value: &Value) -> Self {
        self.with(url, Reply::Bytes(serde_json::to_vec(value).unwrap()))
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fetcher for FakeFetcher {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        expected: Option<&str>,
        _operation: &Operation,
    ) -> std::result::Result<(), FetchError> {
        assert_eq!(expected, None, "el catálogo no pasa hash esperado");
        self.calls.lock().unwrap().push(url.to_owned());
        match self.replies.get(url) {
            Some(Reply::Bytes(bytes)) => {
                fs::write(destination, bytes).unwrap();
                Ok(())
            }
            Some(Reply::Zip(build)) => {
                build(destination);
                Ok(())
            }
            Some(Reply::Status(code)) => Err(FetchError::HttpStatus(*code)),
            Some(Reply::Fail(text)) => Err(FetchError::Failed(mito_core::Error::domain(
                Message::new(*text),
            ))),
            None => Err(FetchError::HttpStatus(404)),
        }
    }
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn layout(&self) -> Layout {
        Layout::new(self.root.path())
    }

    fn catalog(&self, sources: &[(&str, &str)]) -> Catalog {
        let mut configured = BucketSources::new();
        for (name, source) in sources {
            configured.insert(name, source).unwrap();
        }
        Catalog::new(self.layout(), configured)
    }

    fn write_snapshot(&self, bucket: &str, value: &Value) {
        write_json(
            &self.layout().buckets().join(format!("{bucket}.json")),
            value,
        )
        .unwrap();
    }

    fn snapshot(&self, bucket: &str) -> Value {
        let path = self.layout().buckets().join(format!("{bucket}.json"));
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
}

fn detached() -> Operation {
    Operation::detached()
}

fn refresh(catalog: &Catalog, keys: &[&str], fetcher: &FakeFetcher) -> (usize, Vec<String>) {
    let mut log = Vec::new();
    let count = catalog.refresh_keys(keys.iter().copied(), fetcher, &detached(), &mut |message| {
        log.push(message.to_string());
    });
    (count, log)
}

const ZIPBALL: &str = "https://api.github.com/repos/example/catalog/zipball";
const GITHUB: &str = "https://github.com/example/catalog";

fn bucket_zip(version: &'static str) -> Reply {
    Reply::Zip(Box::new(move |target| {
        let manifest = format!("{{\"version\":\"{version}\"}}");
        write_zip(
            target,
            &[
                member("repo/bucket/tool.json", manifest.as_bytes()),
                member("repo/bucket/Other.json", manifest.as_bytes()),
                member("repo/README.md", b"x"),
            ],
        );
    }))
}

#[test]
fn las_fuentes_se_leen_de_las_lineas_bucket() {
    let text = [
        "\u{feff}# comentario",
        "bucket.main=https://github.com/ScoopInstaller/Main",
        "bucket.Extras = https://github.com/ScoopInstaller/Extras ",
        " bucket.indented=x",
        "!bucket.skipped=x",
        "bucket.sinigual",
        "http_proxy=http://p",
        "bucket.main=C:/catalog",
        "",
    ]
    .join("\r\n");
    let sources = BucketSources::from_properties(&text).unwrap();
    let pairs: Vec<_> = sources.iter().collect();
    // Un nombre repetido conserva su posición con el último valor.
    assert_eq!(
        pairs,
        [
            ("main", "C:/catalog"),
            ("extras", "https://github.com/ScoopInstaller/Extras")
        ]
    );
    assert_eq!(
        BucketSources::from_properties("bucket.-x=1")
            .unwrap_err()
            .to_string(),
        "Nombre no válido: '-x'"
    );
    assert!(BucketSources::from_properties("").unwrap().is_empty());
}

#[test]
fn snapshot_con_el_formato_exacto_del_original() {
    let fixture = Fixture::new();
    let source = fixture.root.path().join("catalogo");
    fs::create_dir_all(source.join("bucket")).unwrap();
    fs::write(
        source.join("bucket/b.json"),
        b"\xEF\xBB\xBF{\"version\": \"2\", \"description\": \"Caf\xC3\xA9\"}",
    )
    .unwrap();
    fs::write(source.join("bucket/A.json"), b"{\"version\": \"1\"}").unwrap();
    fs::write(source.join("bucket/notas.txt"), b"x").unwrap();
    fs::write(source.join("raiz.json"), b"{\"version\": \"9\"}").unwrap();
    let source_text = source.to_string_lossy().into_owned();
    let catalog = fixture.catalog(&[]);
    let count = catalog
        .import_bucket("Main", &source_text, &FakeFetcher::default(), &detached())
        .unwrap();
    assert_eq!(count, 2);
    let text = fs::read_to_string(catalog.snapshot_path("main")).unwrap();
    let escaped = serde_json::to_string(&source_text).unwrap();
    let expected = format!(
        "{{\n  \"schema\": 1,\n  \"source\": {escaped},\n  \"apps\": {{\n    \"a\": {{\n      \"version\": \"1\"\n    }},\n    \"b\": {{\n      \"version\": \"2\",\n      \"description\": \"Caf\u{e9}\"\n    }}\n  }}\n}}\n"
    );
    assert_eq!(text, expected);
    // La carpeta temporal de la importación no deja rastro.
    let leftovers: Vec<_> = fs::read_dir(catalog.folder())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["main.json"]);
}

#[test]
fn importar_una_carpeta_sin_subcarpeta_bucket_lee_su_raiz() {
    let fixture = Fixture::new();
    let source = fixture.root.path().join("plano");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tool.json"), b"{\"version\": \"1\"}").unwrap();
    let catalog = fixture.catalog(&[]);
    let count = catalog
        .import_bucket(
            "plano",
            source.to_str().unwrap(),
            &FakeFetcher::default(),
            &detached(),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        fixture.snapshot("plano")["apps"],
        json!({"tool": {"version": "1"}})
    );
    // No escribe en config.properties.
    assert!(!fixture.layout().config_file().exists());
}

#[test]
fn importar_fuentes_invalidas() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default();
    let empty = fixture.root.path().join("vacio");
    fs::create_dir_all(&empty).unwrap();
    let cases = [
        (
            "https://example.org/repo".to_owned(),
            "La fuente debe ser un repositorio GitHub HTTPS o una carpeta local".to_owned(),
        ),
        (
            "https://github.com/solo-owner".to_owned(),
            "La fuente debe ser un repositorio GitHub HTTPS o una carpeta local".to_owned(),
        ),
        (
            empty.to_string_lossy().into_owned(),
            format!("No hay manifiestos en {}", empty.display()),
        ),
        (
            "no-existe".to_owned(),
            "No hay manifiestos en no-existe".to_owned(),
        ),
    ];
    for (source, expected) in cases {
        let error = catalog
            .import_bucket("main", &source, &fetcher, &detached())
            .unwrap_err();
        assert_eq!(error.to_string(), expected);
    }
    assert_eq!(
        catalog
            .import_bucket("-malo", GITHUB, &fetcher, &detached())
            .unwrap_err()
            .to_string(),
        "Nombre no válido: '-malo'"
    );
    assert!(!catalog.snapshot_path("main").exists());
}

#[test]
fn un_json_local_invalido_es_un_error_de_dominio() {
    let fixture = Fixture::new();
    let source = fixture.root.path().join("roto");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tool.json"), b"{ no json").unwrap();
    let catalog = fixture.catalog(&[]);
    let error = catalog
        .import_bucket(
            "roto",
            source.to_str().unwrap(),
            &FakeFetcher::default(),
            &detached(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("JSON no válido en "), "{error}");
    assert!(error.contains("tool.json"), "{error}");
}

#[test]
fn importar_desde_github_descarga_el_zipball_sin_hash() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default().with(ZIPBALL, bucket_zip("1"));
    // La barra final y `http://` se aceptan.
    for source in [
        GITHUB,
        "https://github.com/example/catalog/",
        "http://github.com/example/catalog",
    ] {
        let count = catalog
            .import_bucket("main", source, &fetcher, &detached())
            .unwrap();
        assert_eq!(count, 2);
    }
    assert_eq!(fetcher.calls(), [ZIPBALL; 3]);
    let snapshot = fixture.snapshot("main");
    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["source"], "http://github.com/example/catalog");
    assert_eq!(
        snapshot["apps"],
        json!({"other": {"version": "1"}, "tool": {"version": "1"}})
    );
}

#[test]
fn los_zip_malos_conservan_el_snapshot_anterior() {
    let fixture = Fixture::new();
    fixture.write_snapshot("main", &json!({"apps": {"old": {"version": "1"}}}));
    let before = fs::read(fixture.layout().buckets().join("main.json")).unwrap();
    let symlink = 0o120_777_u32 << 16;
    for (bad, attributes) in [
        ("repo/../escape", None),
        ("repo/CON", None),
        ("repo/bucket/TOOL.json", None),
        ("repo/link", Some(symlink)),
    ] {
        let reply = Reply::Zip(Box::new(move |target| {
            write_zip(
                target,
                &[
                    member("repo/bucket/tool.json", b"{\"version\":\"2\"}"),
                    Member {
                        name: bad,
                        content: b"{}",
                        attributes,
                    },
                ],
            );
        }));
        let fetcher = FakeFetcher::default().with(ZIPBALL, reply);
        let catalog = fixture.catalog(&[]);
        assert!(
            catalog
                .import_bucket("main", GITHUB, &fetcher, &detached())
                .is_err(),
            "{bad}"
        );
        let after = fs::read(fixture.layout().buckets().join("main.json")).unwrap();
        assert_eq!(after, before, "{bad}");
    }
}

#[test]
fn un_fallo_de_descarga_no_toca_el_snapshot() {
    let fixture = Fixture::new();
    fixture.write_snapshot("main", &json!({"apps": {}}));
    let fetcher = FakeFetcher::default().with(ZIPBALL, Reply::Fail("sin red"));
    let error = fixture
        .catalog(&[])
        .import_bucket("main", GITHUB, &fetcher, &detached())
        .unwrap_err();
    assert_eq!(error.to_string(), "sin red");
    assert_eq!(fixture.snapshot("main"), json!({"apps": {}}));
}

#[test]
fn refresh_reimporta_las_fuentes_configuradas() {
    let fixture = Fixture::new();
    let local = fixture.root.path().join("local");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("one.json"), b"{\"version\": \"1\"}").unwrap();
    let local_text = local.to_string_lossy().into_owned();
    fixture.write_snapshot("stale", &json!({"apps": {"x": {"version": "1"}}}));
    let catalog = fixture.catalog(&[("main", GITHUB), ("extra", &local_text)]);
    let fetcher = FakeFetcher::default().with(ZIPBALL, bucket_zip("3"));
    let results = catalog
        .refresh_sources(None, &fetcher, &detached())
        .unwrap();
    assert_eq!(results, [("main".to_owned(), 2), ("extra".to_owned(), 1)]);
    // Un snapshot sin fuente configurada no se toca.
    assert_eq!(
        fixture.snapshot("stale"),
        json!({"apps": {"x": {"version": "1"}}})
    );
    let only = catalog
        .refresh_sources(Some("extra"), &fetcher, &detached())
        .unwrap();
    assert_eq!(only, [("extra".to_owned(), 1)]);
    assert_eq!(
        catalog
            .refresh_sources(Some("nope"), &fetcher, &detached())
            .unwrap_err()
            .to_string(),
        "Bucket no configurado"
    );
}

#[test]
fn refresh_se_detiene_si_la_operacion_se_cancela() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(&[("main", GITHUB)]);
    let operation = Operation::silent();
    operation.cancel();
    let error = catalog
        .refresh_sources(None, &FakeFetcher::default(), &operation)
        .unwrap_err();
    assert!(error.is_cancelled());
}

#[test]
fn all_ordena_snapshots_por_nombre_y_conserva_el_orden_de_apps() {
    let fixture = Fixture::new();
    fixture.write_snapshot(
        "extras",
        &json!({"apps": {"zeta": {"version": "1"}, "alfa": {"version": "2"}}}),
    );
    fixture.write_snapshot("Main", &json!({"apps": {"tool": {"version": "3"}}}));
    let catalog = fixture.catalog(&[]);
    let keys: Vec<String> = catalog.all().unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["extras/zeta", "extras/alfa", "Main/tool"]);
    let entries = catalog.search_entries().unwrap();
    assert!(entries.iter().all(|e| e.status.reason().is_some()));
    assert_eq!(entries.len(), 3);
}

#[test]
fn all_con_un_snapshot_roto_falla_con_mensaje() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(&[]);
    assert_eq!(catalog.all().unwrap(), Vec::new());
    fixture.write_snapshot("main", &json!({"schema": 1}));
    let error = catalog.all().unwrap_err().to_string();
    assert!(
        error.starts_with("Snapshot de catálogo no válido: "),
        "{error}"
    );
    fs::write(catalog.snapshot_path("main"), b"{").unwrap();
    let error = catalog.all().unwrap_err().to_string();
    assert!(error.starts_with("JSON no válido en "), "{error}");
}

#[test]
fn resolve_exige_una_unica_coincidencia() {
    let fixture = Fixture::new();
    fixture.write_snapshot(
        "main",
        &json!({"apps": {"tool": {"version": "1"}, "solo": {"version": "1"}}}),
    );
    fixture.write_snapshot("extras", &json!({"apps": {"tool": {"version": "2"}}}));
    let catalog = fixture.catalog(&[]);
    let ambiguous = "Componente no encontrado o ambiguo: tool. Usa bucket/app.";
    assert_eq!(catalog.resolve("tool").unwrap_err().to_string(), ambiguous);
    assert_eq!(
        catalog.resolve("extras/tool").unwrap().1.raw()["version"],
        "2"
    );
    assert_eq!(catalog.resolve("solo").unwrap().0, "main/solo");
    assert_eq!(
        catalog.resolve("Main/Tool").unwrap_err().to_string(),
        "Componente no encontrado o ambiguo: Main/Tool. Usa bucket/app."
    );
    assert!(catalog.resolve("nada").is_err());
}

#[test]
fn nombres_de_bucket_une_fuentes_y_snapshots() {
    let fixture = Fixture::new();
    fixture.write_snapshot("versions", &json!({"apps": {}}));
    let catalog = fixture.catalog(&[("main", GITHUB), ("versions", GITHUB)]);
    let names: Vec<String> = catalog.bucket_names().unwrap().into_iter().collect();
    assert_eq!(names, ["main", "versions"]);
}

// --- Refresco por clave (test_components.py) ---

const RAW: &str = "https://raw.githubusercontent.com/example/catalog/HEAD";

fn installed_snapshot(fixture: &Fixture) {
    fixture.write_snapshot(
        "main",
        &json!({"schema": 1, "source": GITHUB, "apps": {
            "tool": {"version": "1.0"},
            "uninstalled": {"version": "1.0"}
        }}),
    );
}

#[test]
fn refresca_solo_la_clave_pedida_una_vez_y_conserva_el_resto() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default().json(
        &format!("{RAW}/bucket/tool.json"),
        &json!({"version": "2.0", "url": "https://e.org/t.zip"}),
    );
    let (count, log) = refresh(&catalog, &["main/tool", "main/tool"], &fetcher);
    assert_eq!((count, log), (1, Vec::new()));
    assert_eq!(fetcher.calls(), [format!("{RAW}/bucket/tool.json")]);
    let snapshot = fixture.snapshot("main");
    assert_eq!(snapshot["apps"]["tool"]["version"], "2.0");
    assert_eq!(snapshot["apps"]["uninstalled"], json!({"version": "1.0"}));
    assert_eq!(snapshot["source"], GITHUB);
    let order: Vec<_> = snapshot["apps"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(order, ["tool", "uninstalled"]);
}

#[test]
fn si_bucket_no_existe_en_github_prueba_la_raiz_del_repositorio() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default()
        .with(&format!("{RAW}/bucket/tool.json"), Reply::Status(404))
        .json(&format!("{RAW}/tool.json"), &json!({"version": 2}));
    assert_eq!(refresh(&catalog, &["main/tool"], &fetcher).0, 1);
    assert_eq!(
        fetcher.calls(),
        [
            format!("{RAW}/bucket/tool.json"),
            format!("{RAW}/tool.json")
        ]
    );
    assert_eq!(
        fixture.snapshot("main")["apps"]["tool"],
        json!({"version": 2})
    );
    // Otros códigos HTTP no se reintentan.
    let fetcher =
        FakeFetcher::default().with(&format!("{RAW}/bucket/tool.json"), Reply::Status(500));
    let (count, log) = refresh(&catalog, &["main/tool"], &fetcher);
    assert_eq!(count, 0);
    assert_eq!(
        log,
        ["No se pudo comprobar main/tool: HTTP Error 500. Se conserva el catálogo anterior."]
    );
    assert_eq!(fetcher.calls().len(), 1);
}

#[test]
fn un_origen_configurado_prevalece_sobre_el_del_snapshot() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[("main", "https://github.com/otro/repo")]);
    let url = "https://raw.githubusercontent.com/otro/repo/HEAD/bucket/tool.json";
    let fetcher = FakeFetcher::default().json(url, &json!({"version": "5"}));
    assert_eq!(refresh(&catalog, &["main/tool"], &fetcher).0, 1);
    assert_eq!(fetcher.calls(), [url]);
}

#[test]
fn refresco_local_con_fallos_parciales_conserva_el_manifiesto_anterior() {
    let fixture = Fixture::new();
    let local = fixture.root.path().join("catalogo");
    fs::create_dir_all(local.join("bucket")).unwrap();
    fs::write(local.join("bucket/good.json"), b"{\"version\": \"2.0\"}").unwrap();
    fs::write(local.join("bucket/bad.json"), b"{ roto").unwrap();
    let local_text = local.to_string_lossy().into_owned();
    fixture.write_snapshot(
        "main",
        &json!({"schema": 1, "source": local_text, "apps": {
            "good": {"version": "1.0"}, "bad": {"version": "1.0"}, "missing": {"version": "1.0"}
        }}),
    );
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default();
    let (count, log) = refresh(
        &catalog,
        &["main/good", "main/bad", "main/missing"],
        &fetcher,
    );
    assert_eq!(count, 1);
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].starts_with("No se pudo comprobar main/bad: JSON no válido en "));
    assert!(log[1].starts_with("No se pudo comprobar main/missing: "));
    assert!(
        log.iter()
            .all(|l| l.ends_with("Se conserva el catálogo anterior."))
    );
    let apps = &fixture.snapshot("main")["apps"];
    assert_eq!(apps["good"]["version"], "2.0");
    assert_eq!(apps["bad"]["version"], "1.0");
    assert_eq!(apps["missing"]["version"], "1.0");
    assert_eq!(fetcher.calls(), Vec::<String>::new());
}

#[test]
fn con_la_red_caida_devuelve_cero_y_lo_ya_refrescado_sigue_vigente() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let working = FakeFetcher::default().json(
        &format!("{RAW}/bucket/tool.json"),
        &json!({"version": "2.0"}),
    );
    assert_eq!(refresh(&catalog, &["main/tool"], &working).0, 1);
    let down =
        FakeFetcher::default().with(&format!("{RAW}/bucket/tool.json"), Reply::Fail("sin red"));
    let (count, log) = refresh(&catalog, &["main/tool"], &down);
    assert_eq!(count, 0);
    assert_eq!(
        log,
        ["No se pudo comprobar main/tool: sin red. Se conserva el catálogo anterior."]
    );
    assert_eq!(fixture.snapshot("main")["apps"]["tool"]["version"], "2.0");
}

#[test]
fn refrescar_una_clave_nueva_crea_el_snapshot_con_su_origen() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(&[("main", GITHUB)]);
    let fetcher =
        FakeFetcher::default().json(&format!("{RAW}/bucket/new.json"), &json!({"version": "1"}));
    assert_eq!(refresh(&catalog, &["main/new"], &fetcher).0, 1);
    assert_eq!(
        fixture.snapshot("main"),
        json!({"schema": 1, "source": GITHUB, "apps": {"new": {"version": "1"}}})
    );
    let text = fs::read_to_string(catalog.snapshot_path("main")).unwrap();
    assert!(
        text.starts_with("{\n  \"schema\": 1,\n  \"source\": "),
        "{text}"
    );
}

#[test]
fn claves_y_manifiestos_no_validos_se_registran_sin_tocar_nada() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let before = fs::read(catalog_path(&fixture)).unwrap();
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default()
        .json(&format!("{RAW}/bucket/array.json"), &json!([1]))
        .json(
            &format!("{RAW}/bucket/noversion.json"),
            &json!({"name": "x"}),
        )
        .json(
            &format!("{RAW}/bucket/listver.json"),
            &json!({"version": ["1"]}),
        )
        .json(
            &format!("{RAW}/bucket/badver.json"),
            &json!({"version": "1+b"}),
        );
    let cases = [
        ("main/array", "Manifiesto sin versión válida"),
        ("main/noversion", "Manifiesto sin versión válida"),
        ("main/listver", "Manifiesto sin versión válida"),
        ("main/badver", "Nombre no válido: '1+b'"),
        (
            "sinbarra",
            "not enough values to unpack (expected 2, got 1)",
        ),
        ("a/b/c", "too many values to unpack (expected 2)"),
        ("main/-x", "Nombre no válido: '-x'"),
        ("nuevo/tool", "No hay origen registrado para el catálogo"),
    ];
    for (key, reason) in cases {
        let (count, log) = refresh(&catalog, &[key], &fetcher);
        assert_eq!(count, 0, "{key}");
        assert_eq!(
            log,
            [format!(
                "No se pudo comprobar {key}: {reason}. Se conserva el catálogo anterior."
            )],
            "{key}"
        );
    }
    assert_eq!(fs::read(catalog_path(&fixture)).unwrap(), before);
    let other = Fixture::new();
    let catalog = other.catalog(&[("odd", "https://example.org/x")]);
    let (_, log) = refresh(&catalog, &["odd/tool"], &fetcher);
    assert_eq!(
        log,
        [
            "No se pudo comprobar odd/tool: La fuente debe ser un repositorio GitHub o una carpeta local. Se conserva el catálogo anterior."
        ]
    );
}

fn catalog_path(fixture: &Fixture) -> PathBuf {
    fixture.layout().buckets().join("main.json")
}

#[test]
fn una_version_booleana_pasa_como_en_el_original() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let fetcher = FakeFetcher::default().json(
        &format!("{RAW}/bucket/tool.json"),
        &json!({"version": true}),
    );
    assert_eq!(refresh(&catalog, &["main/tool"], &fetcher).0, 1);
}

#[test]
fn con_el_lock_ocupado_el_refresco_falla_y_se_registra() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let fetcher =
        FakeFetcher::default().json(&format!("{RAW}/bucket/tool.json"), &json!({"version": "9"}));
    let held = MutationLock::mutation(&fixture.layout(), Duration::ZERO).unwrap();
    let (count, log) = refresh(&catalog, &["main/tool"], &fetcher);
    assert_eq!(count, 0);
    assert_eq!(
        log,
        [
            "No se pudo comprobar main/tool: Otra instancia está modificando MITO. Espera a que termine.. Se conserva el catálogo anterior."
        ]
    );
    drop(held);
    assert_eq!(refresh(&catalog, &["main/tool"], &fetcher).0, 1);
}

#[test]
fn refresh_installed_equivale_a_refresh_keys() {
    let fixture = Fixture::new();
    installed_snapshot(&fixture);
    let catalog = fixture.catalog(&[]);
    let fetcher =
        FakeFetcher::default().json(&format!("{RAW}/bucket/tool.json"), &json!({"version": "3"}));
    let mut log = Vec::new();
    let count =
        catalog.refresh_installed(["main/tool"], &fetcher, &detached(), &mut |m| log.push(m));
    assert_eq!((count, log.len()), (1, 0));
}

// --- Recursos de bucket (test_bucket_resources.py) ---

#[test]
fn fuentes_de_recursos_del_bucket_actual_y_de_otro() {
    let fixture = Fixture::new();
    fixture.write_snapshot("other", &json!({"source": "C:/other-catalog", "apps": {}}));
    fixture.write_snapshot("test", &json!({"source": "C:/test-catalog", "apps": {}}));
    let catalog = fixture.catalog(&[("test", GITHUB)]);
    let manifest = json!({
        "pre_install": "Copy-Item $bucketsdir/test/scripts/a",
        "installer": {"script": ["& ${bucketsdir}/other/scripts/b.ps1"]}
    });
    let map = manifest.as_object().unwrap();
    // El origen configurado prevalece; si no, el del snapshot.
    assert_eq!(
        catalog.resource_sources("test/tool", map).unwrap(),
        [
            ("other".to_owned(), "C:/other-catalog".to_owned()),
            ("test".to_owned(), GITHUB.to_owned())
        ]
    );
    let dynamic = json!({"pre_install": "$bucketsdir/$computed/scripts/x"});
    assert!(
        catalog
            .resource_sources("test/tool", dynamic.as_object().unwrap())
            .is_err()
    );
    let missing = json!({"pre_install": "$bucketsdir/missing/scripts/x"});
    assert_eq!(
        catalog
            .resource_sources("test/tool", missing.as_object().unwrap())
            .unwrap_err()
            .to_string(),
        "No hay origen registrado para los recursos del catálogo: missing"
    );
    let odd = fixture.catalog(&[("odd", "https://example.org/x")]);
    let manifest = json!({"pre_install": "$bucketsdir/odd/scripts/x"});
    assert_eq!(
        odd.resource_sources("main/tool", manifest.as_object().unwrap())
            .unwrap_err()
            .to_string(),
        "La fuente debe ser un repositorio GitHub o una carpeta local"
    );
}
