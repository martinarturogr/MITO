use serde_json::{Value, json};

use super::*;

const ZEROS: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Manifiesto válido con `bin`, `persist`, `env_add_path` y `extract_dir`.
fn base() -> Value {
    json!({
        "version": "1.0",
        "url": "https://example.org/tool.zip",
        "hash": ZEROS,
        "bin": "bin/tool.exe",
        "persist": "data",
        "env_add_path": "bin",
        "extract_dir": "tool-1.0"
    })
}

fn with(changes: Value) -> Value {
    let mut manifest = base();
    let Value::Object(changes) = changes else {
        return manifest;
    };
    for (key, value) in changes {
        match key.strip_prefix('-') {
            Some(name) if value.is_null() => {
                manifest.as_object_mut().unwrap().remove(name);
            }
            _ => {
                manifest.as_object_mut().unwrap().insert(key, value);
            }
        }
    }
    manifest
}

fn status_of(key: &str, manifest: &Value) -> String {
    compatibility(key, &Manifest::new(manifest.clone())).text()
}

fn status(manifest: &Value) -> String {
    status_of("main/tool", manifest)
}

#[test]
fn el_manifiesto_base_es_compatible() {
    assert_eq!(status(&base()), "Compatible");
    assert!(compatibility("main/tool", &Manifest::new(base())).is_compatible());
}

#[test]
fn clasifica_sin_resolver_rutas_del_host() {
    // `test_classification_does_not_resolve_host_paths`: la clasificación es sintáctica.
    let expected = [
        (
            "../escape",
            "Manifiesto no soportado: Ruta no portable: ../escape",
        ),
        (
            "C:/absolute",
            "Manifiesto no soportado: Ruta no portable: C:/absolute",
        ),
        (
            "/rooted",
            "Manifiesto no soportado: Ruta no portable: /rooted",
        ),
        (
            "\\rooted",
            "Manifiesto no soportado: Ruta no portable: \\rooted",
        ),
        (
            "file:stream",
            "Manifiesto no soportado: Ruta no portable: file:stream",
        ),
        ("CON", "Manifiesto no soportado: Ruta reservada: CON"),
        (
            "trailing.",
            "Manifiesto no soportado: Ruta reservada: trailing.",
        ),
    ];
    for (extract_dir, message) in expected {
        let manifest = with(json!({"extract_dir": extract_dir}));
        assert_eq!(status(&manifest), message, "{extract_dir}");
    }
}

#[test]
fn clasificar_muchos_manifiestos_es_rapido_y_no_toca_el_disco() {
    // Equivalente de `catalog_performance`: miles de clasificaciones sin E/S.
    let manifest = Manifest::new(base());
    let started = std::time::Instant::now();
    for index in 0..5_000 {
        let key = format!("main/tool{index}");
        assert!(compatibility(&key, &manifest).is_compatible());
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn architecture_64bit_sustituye_los_campos_antes_de_validar() {
    let manifest = with(json!({
        "url": "https://example.org/old.iso",
        "env_set": {"TOOL_HOME": "$dir"},
        "architecture": {"64bit": {"url": "https://example.org/new.zip"}}
    }));
    assert_eq!(status(&manifest), "Compatible");
    let no_x64 = with(json!({"architecture": {"32bit": {}}}));
    assert_eq!(
        status(&no_x64),
        "Manifiesto no soportado: No hay versión Windows x64"
    );
}

#[test]
fn scripts_y_variables_reservadas_no_se_ignoran_en_silencio() {
    let installer = with(json!({"installer": {"file": "setup.exe"}}));
    for key in [
        "main/nodejs",
        "main/nodejs-lts",
        "extras/vscode",
        "other/tool",
    ] {
        assert_eq!(
            status_of(key, &installer),
            "Pasos no soportados: installer.file",
            "{key}"
        );
    }
    let env = with(json!({"env_set": {"HOME": "$dir"}}));
    assert_eq!(status(&env), "Variable reservada: HOME");
}

#[test]
fn alias_de_bin_con_caracteres_literales() {
    for alias in ["my tool", "a&b", "100%", "bang!", "-dash"] {
        let manifest = with(json!({"bin": [["bin/tool.exe", alias]]}));
        assert_eq!(status(&manifest), "Compatible", "{alias}");
    }
    let manifest = with(json!({"bin": [["bin/tool.exe", "tool", "--modo rapido"], "other.exe"]}));
    assert_eq!(status(&manifest), "Compatible");
}

#[test]
fn reglas_de_bin() {
    let cases = [
        (
            json!({"bin": [["solo.exe"]]}),
            "bin debe tener ejecutable, alias y argumentos opcionales",
        ),
        (
            json!({"bin": [["a", "b", "c", "d"]]}),
            "bin debe tener ejecutable, alias y argumentos opcionales",
        ),
        (
            json!({"bin": [["../x.exe", "x"]]}),
            "Manifiesto no soportado: Ruta no portable: ../x.exe",
        ),
        (
            json!({"bin": [["x.exe", "a/b"]]}),
            "Manifiesto no soportado: Nombre de comando no válido: 'a/b'",
        ),
        (
            json!({"bin": [["x.exe", 5]]}),
            "Manifiesto no soportado: Nombre de comando no válido: 5",
        ),
        // Tipos inesperados: el original lanza AttributeError sin capturar.
        (
            json!({"bin": 5}),
            "Manifiesto no soportado: 'int' object has no attribute 'replace'",
        ),
        (
            json!({"bin": {"a": 1}}),
            "Manifiesto no soportado: 'dict' object has no attribute 'replace'",
        ),
    ];
    for (changes, expected) in cases {
        assert_eq!(status(&with(changes.clone())), expected, "{changes}");
    }
}

#[test]
fn reglas_de_env_set() {
    let cases = [
        (
            json!({"env_set": {"path": "x"}}),
            "Variable reservada: path",
        ),
        (
            json!({"env_set": {"MITO_ROOT": "x"}}),
            "Variable reservada: MITO_ROOT",
        ),
        (json!({"env_set": {"1A": "x"}}), "Variable reservada: 1A"),
        (json!({"env_set": {"A-B": "x"}}), "Variable reservada: A-B"),
        (
            json!({"env_set": {"A": "$other"}}),
            "Variable no soportada: A",
        ),
        (
            json!({"env_set": {"A": "100%"}}),
            "Variable no soportada: A",
        ),
        (
            json!({"env_set": {"A": "${dir}/x $persist_dir ${persist_dir}"}}),
            "Compatible",
        ),
        (
            json!({"env_set": {"A": "$dir$"}}),
            "Variable no soportada: A",
        ),
        (
            json!({"env_set": ["A"]}),
            "Manifiesto no soportado: 'list' object has no attribute 'items'",
        ),
        (
            json!({"env_set": {"A": 1}}),
            "Manifiesto no soportado: 'int' object has no attribute 'replace'",
        ),
    ];
    for (changes, expected) in cases {
        assert_eq!(status(&with(changes.clone())), expected, "{changes}");
    }
}

#[test]
fn version_obligatoria_e_identificador() {
    assert_eq!(
        status(&with(json!({"-version": null}))),
        "Manifiesto no soportado: 'version'"
    );
    assert_eq!(
        status(&with(json!({"version": "1.0+build"}))),
        "Manifiesto no soportado: Nombre no válido: '1.0+build'"
    );
    assert_eq!(status(&with(json!({"version": 1.0}))), "Compatible");
    assert_eq!(status(&with(json!({"version": 2}))), "Compatible");
    // `str(True)` es `True`, un identificador válido en el original.
    assert_eq!(status(&with(json!({"version": true}))), "Compatible");
    assert_eq!(
        status(&with(json!({"version": "con"}))),
        "Manifiesto no soportado: Nombre reservado: con"
    );
    assert_eq!(
        status(&with(json!({"version": [1]}))),
        "Manifiesto no soportado: Nombre no válido: '[1]'"
    );
}

#[test]
fn instaladores_antes_de_descargar() {
    for installer in [
        json!(true),
        json!(false),
        json!("setup.exe"),
        json!([]),
        json!({"file": "setup.exe"}),
        json!({"script": "x", "args": "y"}),
        json!({"zeta": 1}),
        json!({"script": null}),
        json!({"script": 7}),
        json!({"script": ["a", 1]}),
    ] {
        assert_ne!(
            status(&with(json!({"installer": installer.clone()}))),
            "Compatible",
            "{installer}"
        );
    }
    for installer in [
        json!(null),
        json!({}),
        json!({"script": ""}),
        json!({"script": []}),
    ] {
        assert_eq!(
            status(&with(json!({"installer": installer.clone()}))),
            "Compatible",
            "{installer}"
        );
    }
    assert_eq!(
        status(&with(json!({"installer": {"file": "a.ps1", "args": 1}}))),
        "installer.args debe ser texto o una lista de líneas"
    );
}

#[test]
fn pre_install_e_innosetup() {
    assert_eq!(
        status(&with(json!({"pre_install": 5}))),
        "pre_install debe ser texto o una lista de líneas"
    );
    assert_eq!(
        status(&with(json!({"pre_install": ["a", 1]}))),
        "pre_install debe ser texto o una lista de líneas"
    );
    assert_eq!(status(&with(json!({"pre_install": null}))), "Compatible");
    assert_eq!(
        status(&with(json!({"pre_install": ["a", "b"]}))),
        "Compatible"
    );
    for value in [json!("true"), json!(1), json!(null)] {
        assert_eq!(
            status(&with(json!({"innosetup": value}))),
            "Pasos no soportados: innosetup"
        );
    }
    assert_eq!(status(&with(json!({"innosetup": true}))), "Compatible");
    // `post_install` no se valida ni se ejecuta.
    assert_eq!(status(&with(json!({"post_install": 5}))), "Compatible");
    assert_eq!(status(&with(json!({"psmodule": false}))), "Compatible");
    assert_eq!(
        status(&with(json!({"psmodule": {"name": "x"}}))),
        "Pasos no soportados: psmodule"
    );
}

#[test]
fn bucketsdir_dinamico_no_es_compatible() {
    let dynamic = with(json!({"pre_install": "$bucketsdir/$computed/scripts/x"}));
    assert_eq!(
        status(&dynamic),
        "Manifiesto no soportado: Pasos no soportados: referencia dinámica a $bucketsdir"
    );
    let literal = with(json!({"pre_install": "$bucketsdir/test/scripts/x"}));
    assert_eq!(status(&literal), "Compatible");
}

#[test]
fn persist_concreto_y_sin_solapes() {
    let cases = [
        (
            json!({"persist": ["data", "data/sub"]}),
            "persist contiene rutas solapadas",
        ),
        (
            json!({"persist": ["data/sub", "data"]}),
            "persist contiene rutas solapadas",
        ),
        (
            json!({"persist": ["Data", "data"]}),
            "persist contiene rutas solapadas",
        ),
        (
            json!({"persist": ["data", ["data", "x"]]}),
            "persist contiene rutas solapadas",
        ),
        (
            json!({"persist": "."}),
            "persist requiere una ruta concreta",
        ),
        (
            json!({"persist": [["a", ""]]}),
            "persist requiere una ruta concreta",
        ),
        (
            json!({"persist": [["a", "b", "c"]]}),
            "Manifiesto no soportado: persist debe ser una ruta o [origen, destino]",
        ),
        (
            json!({"persist": [["x"]]}),
            "Manifiesto no soportado: persist debe ser una ruta o [origen, destino]",
        ),
        (
            json!({"persist": "../x"}),
            "Manifiesto no soportado: Ruta no portable: ../x",
        ),
        (json!({"persist": ["data", "datos"]}), "Compatible"),
        // Solo se comparan los orígenes, no los destinos.
        (json!({"persist": [["a", "x"], ["b", "x"]]}), "Compatible"),
    ];
    for (changes, expected) in cases {
        assert_eq!(status(&with(changes.clone())), expected, "{changes}");
    }
}

#[test]
fn url_obligatoria_http_y_formato() {
    let cases = [
        (
            json!({"-url": null, "-hash": null}),
            "Falta URL de descarga",
        ),
        (json!({"url": null}), "Falta URL de descarga"),
        (json!({"url": [], "-hash": null}), "Falta URL de descarga"),
        (json!({"url": "", "-hash": null}), "Requiere HTTP o HTTPS"),
        (
            json!({"url": "ftp://example.org/a.zip"}),
            "Requiere HTTP o HTTPS",
        ),
        (json!({"url": "file:///c:/a.zip"}), "Requiere HTTP o HTTPS"),
        (json!({"url": 5}), "Requiere HTTP o HTTPS"),
        (json!({"url": "HTTPS://example.org/A.ZIP"}), "Compatible"),
        (
            json!({"url": "https://example.org/tool.iso"}),
            "Formato de archivo todavía no soportado",
        ),
        (
            json!({"url": "https://example.org/tool.tar.gz"}),
            "Compatible",
        ),
        (
            json!({"url": "https://example.org/download?format=zip"}),
            "Compatible",
        ),
        (
            json!({"url": "https://example.org/download?channel=release#/Tool%20File"}),
            "Compatible",
        ),
        (
            json!({"url": "https://example.org/download#/Tool%20File.iso"}),
            "Formato de archivo todavía no soportado",
        ),
    ];
    for (changes, expected) in cases {
        assert_eq!(status(&with(changes.clone())), expected, "{changes}");
    }
}

#[test]
fn los_nombres_de_descarga_estan_confinados() {
    let cases = [
        ("https://e.org/x#/..", "Ruta no portable: .."),
        (
            "https://e.org/x#/dir%2Ffile",
            "Nombre no válido: 'dir/file'",
        ),
        ("https://e.org/x#/C%3Aevil", "Ruta no portable: C:evil"),
        ("https://e.org/x#/NUL", "Ruta reservada: NUL"),
        ("https://e.org/x#/a%5Cb", "Nombre no válido: 'a\\\\b'"),
        ("https://e.org/x#/bad%3Fname", "Ruta reservada: bad?name"),
        ("https://e.org/x#/file.", "Ruta reservada: file."),
    ];
    for (url, message) in cases {
        let manifest = with(json!({"url": url}));
        assert_eq!(
            status(&manifest),
            format!("Manifiesto no soportado: {message}"),
            "{url}"
        );
    }
}

#[test]
fn hashes_opcionales_y_mixtos() {
    assert_eq!(status(&with(json!({"hash": ZEROS}))), "Compatible");
    for hash in [
        json!(null),
        json!(""),
        json!("skip"),
        json!("SKIP"),
        json!("   "),
    ] {
        assert_eq!(
            status(&with(json!({"hash": hash.clone()}))),
            "Compatible",
            "{hash}"
        );
    }
    assert_eq!(status(&with(json!({"-hash": null}))), "Compatible");
    let two = json!({
        "url": ["https://example.org/a.zip", "https://example.org/b.zip"],
        "hash": [null, format!("md5:{}", "0".repeat(32))]
    });
    assert_eq!(status(&with(two)), "Compatible");
    let mixed = json!({
        "url": ["https://example.org/a.zip", "https://example.org/b.zip"],
        "hash": ["skip", "skip"]
    });
    assert_eq!(status(&with(mixed)), "Compatible");
    assert_eq!(
        status(&with(json!({"hash": [ZEROS, ZEROS]}))),
        "Manifiesto no soportado: La cantidad de hashes debe coincidir con las URL"
    );
    assert_eq!(
        status(&with(json!({"hash": "md5:invalid"}))),
        "Manifiesto no soportado: Formato de hash incorrecto"
    );
}

#[test]
fn iconos_de_accesos_directos() {
    for icon in [
        json!("../outside.ico"),
        json!("C:/outside.ico"),
        json!("icon.dll,0"),
        json!("icon.png"),
        json!({"path": "icon.ico"}),
    ] {
        let manifest = with(json!({"shortcuts": [["tool.exe", "Tool", "", icon.clone()]]}));
        assert_ne!(status(&manifest), "Compatible", "{icon}");
    }
    assert_eq!(
        status(&with(
            json!({"shortcuts": [["tool.exe", "Tool", "", "icon.png"]]})
        )),
        "Formato de icono no soportado para el acceso directo: icon.png"
    );
    assert_eq!(
        status(&with(
            json!({"shortcuts": [["tool.exe", "Tool", "", "../outside.ico"]]})
        )),
        "Manifiesto no soportado: Ruta no portable: ../outside.ico"
    );
    for shortcuts in [
        json!([["tool.exe", "Tool", "", "icon.ICO"]]),
        json!([["tool.exe", "Tool", "", ""]]),
        json!([["tool.exe", "Tool", ""]]),
        json!(["tool.exe"]),
        json!({"a": 1}),
        json!("x"),
    ] {
        assert_eq!(
            status(&with(json!({"shortcuts": shortcuts.clone()}))),
            "Compatible",
            "{shortcuts}"
        );
    }
    assert_eq!(
        status(&with(json!({"shortcuts": null}))),
        "Manifiesto no soportado: 'NoneType' object is not iterable"
    );
}

#[test]
fn campos_de_ruta_aceptan_texto_o_lista_y_rechazan_otros_tipos() {
    assert_eq!(
        status(&with(
            json!({"extract_to": ["a", "b"], "extract_dir": ["x", null]})
        )),
        "Compatible"
    );
    assert_eq!(
        status(&with(json!({"env_add_path": ["bin", "../x"]}))),
        "Manifiesto no soportado: Ruta no portable: ../x"
    );
    assert_eq!(
        status(&with(json!({"extract_dir": 5}))),
        "Manifiesto no soportado: argument should be a str or an os.PathLike object where __fspath__ returns a str, not 'int'"
    );
}

#[test]
fn un_manifiesto_que_no_es_objeto_no_es_compatible() {
    for value in [json!(null), json!([1]), json!("x"), json!(3)] {
        assert_eq!(
            status(&value),
            "Manifiesto no soportado: El manifiesto debe ser un objeto JSON"
        );
    }
}

#[test]
fn el_estado_expone_motivo_y_texto() {
    let rejected = compatibility("main/tool", &Manifest::new(with(json!({"psmodule": true}))));
    assert!(!rejected.is_compatible());
    assert_eq!(
        rejected.reason().map(Message::template),
        Some("Pasos no soportados: psmodule")
    );
    assert_eq!(rejected.to_string(), "Pasos no soportados: psmodule");
    let wrapped = compatibility(
        "main/tool",
        &Manifest::new(with(json!({"persist": "../x"}))),
    );
    assert_eq!(
        wrapped.reason().map(Message::template),
        Some("Manifiesto no soportado: {error}")
    );
}
