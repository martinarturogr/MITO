//! Pruebas del proxy del bootstrap: equivalentes de `test_bootstrap_proxy.py` y de cada
//! rama de la sección 5.7. Un proxy HTTP local en `127.0.0.1` hace de proxy de empresa
//! (el host de la URL absoluta elige el comportamiento, como el `FakeProxy` del original);
//! el registro de Windows, PowerShell y las ventanas están sustituidos. Nada sale a
//! Internet ni toca el proxy real del equipo.
#![allow(
    clippy::assert_is_empty,
    clippy::similar_names,
    reason = "los tests comparan colecciones vacías y nombran pares de variables parecidos"
)]

use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use mito_core::Layout;
use mito_transfer::network::{Proxies, SystemProxy, portal_fields, probe};
use serde_json::Value;

use super::*;
use crate::console::testing::console;

/// Estado compartido del proxy falso.
#[derive(Clone, Default)]
struct State {
    authenticated: Arc<AtomicBool>,
    posts: Arc<Mutex<Vec<String>>>,
}

/// Proxy HTTP local.
struct FakeProxy {
    url: String,
    state: State,
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn reply(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &str) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn handle(mut stream: TcpStream, state: &State) {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 2048];
    let split = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&buffer[..n]),
        }
    };
    let head = String::from_utf8_lossy(&data[..split]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    while data.len() < split + length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&buffer[..n]),
        }
    }
    let body = String::from_utf8_lossy(&data[split..]).into_owned();
    let mut parts = head.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method == "CONNECT" {
        return reply(&mut stream, "502 Bad Gateway", &[], "");
    }
    let rest = target.strip_prefix("http://").unwrap_or(target);
    let (host, path_query) = rest.split_once('/').map_or((rest, "/"), |(h, p)| {
        (h, &target[target.len() - p.len() - 1..])
    });
    let host = host.split(':').next().unwrap_or("");
    let path = path_query.split('?').next().unwrap_or("");
    if method == "POST" {
        state.posts.lock().unwrap().push(body.clone());
        let fields = form(&body);
        let get = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        if get("pass") == Some("secret-pass") && get("csrf") == Some("tok&en") {
            state.authenticated.store(true, Ordering::SeqCst);
        }
        return reply(&mut stream, "302 Found", &[("Location", "/done")], "");
    }
    match path {
        "/generate_204" => {
            if host == "ok.invalid" || state.authenticated.load(Ordering::SeqCst) {
                reply(&mut stream, "204 No Content", &[], "");
            } else if host == "ntlm.invalid" {
                reply(
                    &mut stream,
                    "407 Proxy Authentication Required",
                    &[
                        ("Proxy-Authenticate", "NTLM"),
                        ("Proxy-Authenticate", "Negotiate"),
                    ],
                    "",
                );
            } else {
                reply(
                    &mut stream,
                    "302 Found",
                    &[("Location", "http://captive.invalid/login?from=check")],
                    "",
                );
            }
        }
        "/login" => {
            let action = if host == "foreign.invalid" {
                "http://other.invalid/submit"
            } else {
                "/submit"
            };
            let page = format!(
                "<html><form method=\"post\" action=\"{action}\">\
                 <input type=\"hidden\" name=\"csrf\" value=\"tok&amp;en\">\
                 <input type=\"text\" name=\"user\"><input type=\"password\" name=\"pass\">\
                 </form></html>"
            );
            reply(
                &mut stream,
                "200 OK",
                &[("Content-Type", "text/html")],
                &page,
            );
        }
        _ => reply(&mut stream, "404 Not Found", &[], ""),
    }
}

fn fake_proxy() -> FakeProxy {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = State::default();
    let shared = state.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let state = shared.clone();
            thread::spawn(move || handle(stream, &state));
        }
    });
    FakeProxy {
        url: format!("http://127.0.0.1:{port}"),
        state,
    }
}

/// Instancia temporal.
struct Instance {
    _dir: tempfile::TempDir,
    layout: Layout,
}

fn instance(config: &str) -> Instance {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    if !config.is_empty() {
        fs::write(layout.config_file(), config).unwrap();
    }
    Instance { _dir: dir, layout }
}

impl Instance {
    fn config(&self) -> String {
        fs::read_to_string(self.layout.config_file()).unwrap_or_default()
    }

    fn records(&self) -> Vec<Value> {
        fs::read_to_string(self.layout.logs().join("mito.log"))
            .map(|text| {
                text.lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn log_text(&self) -> String {
        let mut text = String::new();
        if let Ok(entries) = fs::read_dir(self.layout.logs()) {
            for entry in entries.flatten() {
                text.push_str(&fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
        text
    }

    /// Mensajes de las líneas de log de la fase `phase`.
    fn messages(&self, phase: &str) -> Vec<String> {
        self.records()
            .iter()
            .filter(|r| r["phase"] == phase)
            .map(|r| r["message"].as_str().unwrap().to_owned())
            .collect()
    }
}

/// Lo observado en una ejecución.
struct Run {
    result: anyhow::Result<Proxies>,
    out: String,
    err: String,
    probes: usize,
    windows_calls: usize,
    targets: Vec<String>,
}

/// Ejecuta `initialize` con servicios falsos.
struct Scenario<'a> {
    instance: &'a Instance,
    input: &'a str,
    interactive: bool,
    env: Vec<(String, String)>,
    system: Option<SystemProxy>,
    windows: bool,
    check: Option<&'a str>,
}

impl<'a> Scenario<'a> {
    fn new(instance: &'a Instance, input: &'a str) -> Self {
        Self {
            instance,
            input,
            interactive: true,
            env: Vec::new(),
            system: None,
            windows: false,
            check: None,
        }
    }

    fn run(self) -> Run {
        let probes = Arc::new(AtomicUsize::new(0));
        let windows_calls = Arc::new(AtomicUsize::new(0));
        let targets = Arc::new(Mutex::new(Vec::new()));
        let system = self.system.clone();
        let counted = Arc::clone(&probes);
        let seen = Arc::clone(&targets);
        let windows_counter = Arc::clone(&windows_calls);
        let windows = self.windows;
        let system_proxy = move |target: &str| {
            seen.lock().unwrap().push(target.to_owned());
            system.clone()
        };
        let counting_probe = move |url: &str, proxy: &str, timeout| {
            counted.fetch_add(1, Ordering::SeqCst);
            probe(url, proxy, timeout)
        };
        let windows_probe = move |_: &str, _: &str, _, _| {
            windows_counter.fetch_add(1, Ordering::SeqCst);
            windows
        };
        let services = Services {
            env: self.env.clone(),
            system_proxy: &system_proxy,
            probe: &counting_probe,
            windows_probe: &windows_probe,
            portal_fields: &portal_fields,
            system_proxies: Some(Proxies::default()),
        };
        let (console, out, err) = console(self.input);
        let target = self.check.unwrap_or("https://tools.invalid/tool.zip");
        let result = initialize(
            &self.instance.layout,
            target,
            self.interactive,
            &console,
            &services,
        );
        Run {
            result,
            out: out.text(),
            err: err.text(),
            probes: probes.load(Ordering::SeqCst),
            windows_calls: windows_calls.load(Ordering::SeqCst),
            targets: targets.lock().unwrap().clone(),
        }
    }
}

fn config_for(proxy: &FakeProxy, check_host: &str, extra: &str) -> String {
    format!(
        "https_proxy={}\nnetwork.check_url=http://{check_host}/generate_204\n{extra}",
        proxy.url
    )
}

const PORTAL_FIELDS: &str = "proxy_authentication_username_field=user\n\
                             proxy_authentication_password_field=pass\n\
                             proxy_authentication_require_https=false\n\
                             enable_proxy_authentication=true\n\
                             proxy_authentication_url=http://captive.invalid/login\n";

// --- sondas y mensajes por tipo (test_probe_and_portal_login_through_local_proxy) -------

#[test]
fn ok_el_proxy_permite_el_acceso() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ok.invalid", ""));
    let run = Scenario::new(&instance, "").run();
    assert!(
        run.out
            .contains("MITO: el proxy permite el acceso a Internet.\n")
    );
    let proxies = run.result.unwrap();
    assert_eq!(proxies.https.as_deref(), Some(proxy.url.as_str()));
    assert_eq!(
        instance.messages("proxy"),
        [format!("Proxy {} from config.properties: ok", proxy.url)]
    );
    let record = &instance.records()[0];
    assert_eq!(record["status"], 204);
    assert_eq!(record["authenticate"], "");
    assert_eq!(record["network_error"], "");
}

#[test]
fn un_407_ntlm_sin_credenciales_de_windows_es_auth_con_los_esquemas_ordenados() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ntlm.invalid", ""));
    let run = Scenario::new(&instance, "").run();
    assert!(
        run.out.contains(
            "MITO: el proxy exige autenticación HTTP 407 (NTLM, Negotiate), que MITO no implementa.\n"
        ),
        "{}",
        run.out
    );
    assert_eq!(run.windows_calls, 1);
    assert!(instance.messages("proxy")[0].ends_with(": auth"));
    assert_eq!(instance.records()[0]["authenticate"], "NTLM, Negotiate");
}

#[test]
fn un_407_ntlm_aceptado_con_la_sesion_de_windows_es_windows() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ntlm.invalid", PORTAL_FIELDS));
    let mut scenario = Scenario::new(&instance, "ana\n");
    scenario.windows = true;
    let run = scenario.run();
    assert!(
        run.out.contains(
            "MITO: el proxy acepta tu sesión de Windows (NTLM/Kerberos). MITO no puede usarla: algunas descargas podrían fallar con HTTP 407.\n"
        ),
        "{}",
        run.out
    );
    assert!(!run.out.contains("Python"));
    assert!(instance.messages("proxy")[0].ends_with(": windows"));
    // Con «windows» no se ofrece el login del portal aunque esté configurado.
    assert!(!run.out.contains("Usuario"));
}

#[test]
fn un_407_sin_ntlm_ni_negotiate_no_prueba_las_credenciales_de_windows() {
    let proxy = fake_proxy();
    // La sonda recibe 302 en cualquier otro host: no es 407.
    let instance = instance(&config_for(&proxy, "captive.invalid", ""));
    let run = Scenario::new(&instance, "").run();
    assert_eq!(run.windows_calls, 0);
}

#[test]
fn un_portal_se_detecta_y_su_url_se_muestra_sin_la_query() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "captive.invalid", ""));
    let run = Scenario::new(&instance, "").run();
    assert!(
        run.out.contains(
            "MITO: el proxy redirige a un portal de autenticación: http://captive.invalid/login"
        ),
        "{}",
        run.out
    );
    assert!(!run.out.contains("from=check"), "{}", run.out);
    assert!(instance.messages("proxy")[0].ends_with(": portal"));
}

#[test]
fn una_respuesta_inesperada_es_blocked_con_el_estado() {
    let proxy = fake_proxy();
    let instance = instance(&format!(
        "https_proxy={}\nnetwork.check_url=http://ok.invalid/otra\n",
        proxy.url
    ));
    let run = Scenario::new(&instance, "").run();
    assert!(
        run.out
            .contains("MITO: la comprobación de Internet a través del proxy respondió HTTP 404.\n"),
        "{}",
        run.out
    );
    assert_eq!(instance.records()[0]["status"], 404);
}

#[test]
fn un_proxy_que_no_responde_es_unreachable_con_el_error() {
    let instance = instance(
        "https_proxy=http://127.0.0.1:1\nnetwork.check_url=http://ok.invalid/generate_204\n",
    );
    let run = Scenario::new(&instance, "").run();
    assert!(
        run.out
            .contains("MITO: no se pudo comprobar Internet a través del proxy ("),
        "{}",
        run.out
    );
    assert!(instance.messages("proxy")[0].ends_with(": unreachable"));
    assert_eq!(instance.records()[0]["status"], 0);
    assert_ne!(instance.records()[0]["network_error"], "");
}

#[test]
fn una_sonda_https_que_falla_se_repite_por_http_para_detectar_el_portal() {
    let proxy = fake_proxy();
    // El túnel HTTPS falla (CONNECT rechazado) y la versión HTTP revela el portal.
    let instance = instance(&format!(
        "https_proxy={}\nnetwork.check_url=https://captive.invalid/generate_204\n",
        proxy.url
    ));
    let run = Scenario::new(&instance, "").run();
    assert_eq!(run.probes, 2);
    assert!(run.out.contains("portal de autenticación"), "{}", run.out);
}

#[test]
fn una_sonda_https_correcta_no_se_repite() {
    let proxy = fake_proxy();
    let instance = instance(&format!(
        "https_proxy={}\nnetwork.check_url=http://ok.invalid/generate_204\n",
        proxy.url
    ));
    assert_eq!(Scenario::new(&instance, "").run().probes, 1);
}

// --- configuración de proxy ----------------------------------------------------------

#[test]
fn sin_https_proxy_y_solo_http_proxy_no_hay_proxy_ni_sonda() {
    let instance = instance("http_proxy=http://127.0.0.1:9\n");
    let run = Scenario::new(&instance, "").run();
    assert!(run.result.is_ok());
    assert_eq!((run.probes, run.out.as_str()), (0, ""));
    assert!(run.targets.is_empty());
    assert!(instance.messages("proxy").is_empty());
}

#[test]
fn un_https_proxy_vacio_desactiva_el_proxy_y_se_registra() {
    let instance = instance("https_proxy=\n");
    let mut scenario = Scenario::new(&instance, "");
    scenario.system = Some(SystemProxy {
        url: "http://10.0.0.1:8080".to_owned(),
        source: "manual".to_owned(),
        no_proxy: "localhost".to_owned(),
    });
    let run = scenario.run();
    let proxies = run.result.unwrap();
    // Conexión directa, también frente al proxy de Windows.
    assert_eq!(proxies.https.as_deref(), Some(""));
    assert_eq!((run.probes, run.out.as_str()), (0, ""));
    assert!(run.targets.is_empty());
    assert_eq!(
        instance.messages("proxy"),
        ["Proxy disabled by config.properties"]
    );
}

#[test]
fn un_proxy_configurado_invalido_es_un_error() {
    for bad in ["ftp://p:1", "http://u:p@host:1", "sin esquema", "http://"] {
        let instance = instance(&format!("https_proxy={bad}\n"));
        let run = Scenario::new(&instance, "").run();
        assert_eq!(
            run.result.unwrap_err().to_string(),
            "Proxy: usa una URL HTTP/HTTPS sin credenciales",
            "{bad}"
        );
    }
}

#[test]
fn una_url_de_comprobacion_invalida_es_un_error() {
    let instance = instance("network.check_url=ftp://x\n");
    let run = Scenario::new(&instance, "").run();
    assert_eq!(
        run.result.unwrap_err().to_string(),
        "network.check_url: usa una URL HTTP/HTTPS sin credenciales"
    );
}

#[test]
fn un_proxy_configurado_no_ofrece_guardar_ni_dice_proxy_detectado() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ok.invalid", ""));
    let before = instance.config();
    let run = Scenario::new(&instance, "s\n").run();
    assert!(!run.out.contains("detectado"));
    assert!(!run.out.contains("Guardar"));
    assert_eq!(instance.config(), before);
}

// --- detección sin configuración ------------------------------------------------------

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn https_proxy_del_entorno_se_detecta_con_el_no_proxy_de_no_proxy() {
    let proxy = fake_proxy();
    let instance = instance("network.check_url=http://ok.invalid/generate_204\n");
    let mut scenario = Scenario::new(&instance, "n\n");
    let url = format!("{}/", proxy.url);
    scenario.env = env(&[
        ("https_proxy", url.as_str()),
        ("NO_PROXY", "<local>;*.corp.example;intranet;10.*"),
    ]);
    let run = scenario.run();
    assert!(
        run.out.contains(&format!(
            "MITO: proxy detectado: {} (variable de entorno HTTPS_PROXY)\n",
            proxy.url
        )),
        "{}",
        run.out
    );
    let proxies = run.result.unwrap();
    assert_eq!(proxies.https.as_deref(), Some(proxy.url.as_str()));
    assert_eq!(
        proxies.no_proxy.as_deref(),
        Some("localhost,127.0.0.1,::1,.corp.example,intranet")
    );
    // El proxy de Windows no se consulta.
    assert!(run.targets.is_empty());
}

#[test]
fn un_https_proxy_del_entorno_invalido_se_ignora_con_aviso_y_se_usa_el_de_windows() {
    let instance = instance("");
    let mut scenario = Scenario::new(&instance, "");
    scenario.env = env(&[("HTTPS_PROXY", "http://usuario:clave@127.0.0.1:9")]);
    let run = scenario.run();
    assert!(run.result.is_ok());
    assert_eq!(run.targets, ["https://tools.invalid/tool.zip"]);
    assert_eq!(
        instance.messages("proxy"),
        [
            "Ignoring HTTPS_PROXY with credentials or invalid URL",
            "No system proxy detected; direct connection"
        ]
    );
    let levels: Vec<String> = instance
        .records()
        .iter()
        .map(|r| r["level"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(levels, ["warning", "info"]);
    // La contraseña de la URL ignorada no llega al log.
    assert!(!instance.log_text().contains("clave"));
}

#[test]
fn sin_proxy_del_sistema_la_conexion_es_directa_y_se_registra() {
    let instance = instance("");
    let run = Scenario::new(&instance, "").run();
    assert!(run.result.is_ok());
    assert_eq!((run.probes, run.out.as_str()), (0, ""));
    assert_eq!(
        instance.messages("proxy"),
        ["No system proxy detected; direct connection"]
    );
}

#[test]
fn el_origen_del_proxy_del_sistema_se_describe_como_el_original() {
    for (source, text) in [
        ("manual", "configuración manual de Windows"),
        ("pac", "script PAC"),
        ("wpad", "autodetección WPAD"),
    ] {
        let proxy = fake_proxy();
        let instance = instance("network.check_url=http://ok.invalid/generate_204\n");
        let mut scenario = Scenario::new(&instance, "n\n");
        scenario.system = Some(SystemProxy {
            url: proxy.url.clone(),
            source: source.to_owned(),
            no_proxy: "localhost,127.0.0.1,::1".to_owned(),
        });
        let run = scenario.run();
        assert!(
            run.out
                .contains(&format!("MITO: proxy detectado: {} ({text})\n", proxy.url)),
            "{source}: {}",
            run.out
        );
    }
}

// --- guardado ---------------------------------------------------------------------------

fn detected(proxy: &FakeProxy, host: &str) -> (Instance, SystemProxy) {
    let instance = instance(&format!(
        "# Mi configuración\nbucket.main=x\nnetwork.check_url=http://{host}/generate_204\n"
    ));
    let system = SystemProxy {
        url: proxy.url.clone(),
        source: "manual".to_owned(),
        no_proxy: "localhost,127.0.0.1,::1,.corp.example".to_owned(),
    };
    (instance, system)
}

#[test]
fn guardar_por_defecto_conserva_comentarios_orden_y_claves_ajenas() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "ok.invalid");
    let mut scenario = Scenario::new(&instance, "\n");
    scenario.system = Some(system);
    let run = scenario.run();
    assert!(
        run.out
            .contains("\nSe añadirán estas claves a config.properties:\n")
    );
    assert!(run.out.contains(&format!("  http_proxy={}\n", proxy.url)));
    assert!(
        run.out
            .contains("  no_proxy=localhost,127.0.0.1,::1,.corp.example\n")
    );
    assert!(
        run.out
            .contains("¿Guardar esta configuración de red? [S/n]: ")
    );
    assert!(
        run.out
            .contains("MITO: configuración de red guardada en config.properties.\n")
    );
    assert!(!run.out.contains("sólo durante esta preparación"));
    assert_eq!(
        instance.config(),
        format!(
            "# Mi configuración\nbucket.main=x\nnetwork.check_url=http://ok.invalid/generate_204\n\
             http_proxy={0}\nhttps_proxy={0}\nno_proxy=localhost,127.0.0.1,::1,.corp.example\n",
            proxy.url
        )
    );
    assert!(!instance.config().starts_with('\u{feff}'));
    assert!(
        instance
            .messages("proxy")
            .contains(&"Detected proxy saved to config.properties".to_owned())
    );
}

#[test]
fn las_respuestas_afirmativas_y_la_entrada_vacia_guardan() {
    for answer in ["s", "si", "sí", "y", "yes", "S", " Sí ", ""] {
        let proxy = fake_proxy();
        let (instance, system) = detected(&proxy, "ok.invalid");
        let input = format!("{answer}\n");
        let mut scenario = Scenario::new(&instance, &input);
        scenario.system = Some(system);
        scenario.run();
        assert!(instance.config().contains("https_proxy="), "{answer:?}");
    }
}

#[test]
fn las_demas_respuestas_y_el_fin_de_la_entrada_no_guardan() {
    for input in ["n\n", "no\n", "x\n", "quizá\n", ""] {
        let proxy = fake_proxy();
        let (instance, system) = detected(&proxy, "ok.invalid");
        let before = instance.config();
        let mut scenario = Scenario::new(&instance, input);
        scenario.system = Some(system);
        let run = scenario.run();
        assert_eq!(instance.config(), before, "{input:?}");
        assert!(
            run.out
                .contains("MITO: se usará el proxy detectado sólo durante esta preparación.\n"),
            "{input:?}"
        );
        // El proxy se usa en las descargas de esta preparación.
        let proxies = run.result.unwrap();
        assert_eq!(proxies.https.as_deref(), Some(proxy.url.as_str()));
        assert_eq!(proxies.http.as_deref(), Some(proxy.url.as_str()));
        assert_eq!(
            proxies.no_proxy.as_deref(),
            Some("localhost,127.0.0.1,::1,.corp.example")
        );
    }
}

#[test]
fn con_approved_nunca_se_pregunta_ni_se_guarda() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "captive.invalid");
    let before = instance.config();
    let mut scenario = Scenario::new(&instance, "s\nana\nsecret-pass\n");
    scenario.system = Some(system);
    scenario.interactive = false;
    let run = scenario.run();
    assert!(!run.out.contains("Guardar"));
    assert!(!run.out.contains("Usuario"));
    assert!(!run.out.contains("Se añadirán"));
    assert!(run.out.contains("sólo durante esta preparación"));
    assert_eq!(instance.config(), before);
    assert!(run.result.is_ok());
    assert!(!proxy.state.authenticated.load(Ordering::SeqCst));
}

#[test]
fn la_propuesta_incluye_el_portal_y_los_nombres_de_campo_no_predeterminados() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "captive.invalid");
    let mut scenario = Scenario::new(&instance, "s\n\n");
    scenario.system = Some(system);
    let run = scenario.run();
    for expected in [
        "  enable_proxy_authentication=true\n",
        "  proxy_authentication_url=http://captive.invalid/login?[redacted]\n",
        "  proxy_authentication_username_field=user\n",
        "  proxy_authentication_password_field=pass\n",
        "Tu usuario y contraseña nunca se guardan: MITO los pedirá cuando el portal lo requiera.\n",
    ] {
        assert!(run.out.contains(expected), "{expected}: {}", run.out);
    }
    let config = instance.config();
    assert!(
        config.contains("enable_proxy_authentication=true\n"),
        "{config}"
    );
    // La URL guardada es la real, con su query.
    assert!(
        config.contains("proxy_authentication_url=http://captive.invalid/login?from=check\n"),
        "{config}"
    );
    assert!(config.contains("proxy_authentication_username_field=user\n"));
    assert!(config.contains("proxy_authentication_password_field=pass\n"));
}

#[test]
fn un_portal_ya_configurado_no_se_sustituye() {
    let proxy = fake_proxy();
    let instance = instance(
        "network.check_url=http://captive.invalid/generate_204\n\
         proxy_authentication_url=http://captive.invalid/otro\n",
    );
    let mut scenario = Scenario::new(&instance, "n\n");
    scenario.system = Some(SystemProxy {
        url: proxy.url.clone(),
        source: "manual".to_owned(),
        no_proxy: "localhost".to_owned(),
    });
    let run = scenario.run();
    assert!(!run.out.contains("  proxy_authentication_url="));
    assert!(!run.out.contains("enable_proxy_authentication"));
}

#[test]
fn un_fallo_al_guardar_se_avisa_y_se_sigue_con_el_proxy_temporal() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "ok.invalid");
    // Un directorio en lugar del archivo: no se puede leer ni escribir.
    fs::remove_file(instance.layout.config_file()).unwrap();
    fs::create_dir(instance.layout.config_file()).unwrap();
    let mut scenario = Scenario::new(&instance, "s\n");
    scenario.system = Some(system);
    let run = scenario.run();
    assert!(
        run.out
            .contains("MITO: no se pudo guardar config.properties: "),
        "{}",
        run.out
    );
    assert!(run.out.contains("sólo durante esta preparación"));
    assert!(run.result.is_ok());
    assert!(
        instance
            .records()
            .iter()
            .any(|r| r["level"] == "warning" && r["phase"] == "proxy")
    );
}

// --- login del portal -------------------------------------------------------------------

fn portal_instance(proxy: &FakeProxy, extra: &str) -> Instance {
    instance(&config_for(
        proxy,
        "captive.invalid",
        &format!("{PORTAL_FIELDS}{extra}"),
    ))
}

#[test]
fn login_correcto_envia_ocultos_y_usuario_y_no_registra_la_contrasena() {
    let proxy = fake_proxy();
    let instance = portal_instance(&proxy, "");
    // Contraseña errónea y después la correcta.
    let run = Scenario::new(&instance, "ana\nwrong\nana\nsecret-pass\n").run();
    assert!(
        run.out.contains(
            "\nInicia sesión en el portal de red para continuar. Deja el usuario vacío para omitirlo.\n"
        ),
        "{}",
        run.out
    );
    assert!(run.out.contains("Usuario: "));
    assert!(run.out.contains("Contraseña: "));
    assert!(
        run.out
            .contains("MITO: no se pudo verificar Internet. Revisa usuario y contraseña.\n")
    );
    assert!(run.out.contains("MITO: autenticación correcta.\n"));
    // Los dos intentos llegan al POST, con campos ocultos y usuario.
    let posts = proxy.state.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 2);
    assert_eq!(
        form(&posts[1]),
        [
            ("csrf".to_owned(), "tok&en".to_owned()),
            ("user".to_owned(), "ana".to_owned()),
            ("pass".to_owned(), "secret-pass".to_owned()),
        ]
    );
    assert_eq!(
        instance.messages("proxy-authentication"),
        [
            "Portal authentication not verified",
            "Portal authentication verified"
        ]
    );
    // La contraseña no se muestra, ni se registra, ni se guarda.
    assert!(!run.out.contains("secret-pass") && !run.out.contains("wrong"));
    assert!(!run.err.contains("secret-pass"));
    assert!(!instance.log_text().contains("secret-pass"));
    assert!(!instance.log_text().contains("wrong"));
    assert!(!instance.config().contains("secret-pass"));
}

#[test]
fn el_login_se_limita_a_tres_intentos() {
    let proxy = fake_proxy();
    let instance = portal_instance(&proxy, "");
    let run = Scenario::new(
        &instance,
        "ana\nuno\nana\ndos\nana\ntres\nana\nsecret-pass\n",
    )
    .run();
    assert_eq!(proxy.state.posts.lock().unwrap().len(), 3);
    assert!(!run.out.contains("autenticación correcta"));
    assert_eq!(
        run.out
            .matches("MITO: no se pudo verificar Internet.")
            .count(),
        3
    );
    assert!(!proxy.state.authenticated.load(Ordering::SeqCst));
}

#[test]
fn un_usuario_vacio_o_el_fin_de_la_entrada_omiten_el_login() {
    for input in ["\n", "   \n", ""] {
        let proxy = fake_proxy();
        let instance = portal_instance(&proxy, "");
        let run = Scenario::new(&instance, input).run();
        assert!(run.out.contains("Usuario: "), "{input:?}");
        assert!(!run.out.contains("Contraseña"), "{input:?}");
        assert!(proxy.state.posts.lock().unwrap().is_empty());
        assert!(run.result.is_ok());
    }
}

#[test]
fn sin_portal_activado_o_sin_url_no_se_pide_el_login() {
    let proxy = fake_proxy();
    let disabled = instance(&config_for(
        &proxy,
        "captive.invalid",
        "enable_proxy_authentication=false\nproxy_authentication_url=http://captive.invalid/login\n",
    ));
    assert!(
        !Scenario::new(&disabled, "ana\n")
            .run()
            .out
            .contains("Usuario")
    );
    let no_url = instance(&config_for(
        &proxy,
        "captive.invalid",
        "enable_proxy_authentication=true\n",
    ));
    assert!(
        !Scenario::new(&no_url, "ana\n")
            .run()
            .out
            .contains("Usuario")
    );
}

#[test]
fn con_el_proxy_ok_no_se_pide_el_login_aunque_este_configurado() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ok.invalid", PORTAL_FIELDS));
    assert!(
        !Scenario::new(&instance, "ana\n")
            .run()
            .out
            .contains("Usuario")
    );
}

#[test]
fn un_formulario_de_otro_origen_se_rechaza_en_rojo_y_se_registra() {
    let proxy = fake_proxy();
    let config = config_for(&proxy, "captive.invalid", PORTAL_FIELDS)
        .replace("captive.invalid/login", "foreign.invalid/login");
    let instance = instance(&config);
    let run = Scenario::new(&instance, "ana\nsecret-pass\nana\nsecret-pass\n").run();
    assert!(
        run.out.contains("MITO: El formulario apunta a otro origen"),
        "{}",
        run.out
    );
    // El error corta los intentos: un solo usuario leído.
    assert_eq!(run.out.matches("Usuario: ").count(), 1);
    assert!(proxy.state.posts.lock().unwrap().is_empty());
    let warnings = instance.messages("proxy-authentication");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("otro origen"), "{warnings:?}");
    assert!(!instance.log_text().contains("secret-pass"));
}

#[test]
fn el_post_exige_https_salvo_exclusion_explicita() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(
        &proxy,
        "captive.invalid",
        "enable_proxy_authentication=true\nproxy_authentication_url=http://captive.invalid/login\n\
         proxy_authentication_username_field=user\nproxy_authentication_password_field=pass\n",
    ));
    let run = Scenario::new(&instance, "ana\nx\n").run();
    assert!(
        run.out.contains("MITO: El POST requiere HTTPS"),
        "{}",
        run.out
    );
    assert!(proxy.state.posts.lock().unwrap().is_empty());
}

#[test]
fn el_login_funciona_con_el_proxy_detectado_aunque_no_se_guarde() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "captive.invalid");
    // No se guarda; la configuración en memoria (portal y campos) basta para el login.
    // Los POST por HTTP exigen la exclusión explícita, que sí está en el archivo.
    fs::write(
        instance.layout.config_file(),
        "network.check_url=http://captive.invalid/generate_204\n\
         proxy_authentication_require_https=false\n",
    )
    .unwrap();
    let before = instance.config();
    let mut scenario = Scenario::new(&instance, "n\nana\nsecret-pass\n");
    scenario.system = Some(system);
    let run = scenario.run();
    assert!(
        run.out.contains("MITO: autenticación correcta.\n"),
        "{}",
        run.out
    );
    assert_eq!(instance.config(), before);
    let posts = proxy.state.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1);
    // Los nombres de campo detectados (`user` y `pass`) se usaron.
    assert!(posts[0].contains("user=ana") && posts[0].contains("pass=secret-pass"));
    assert!(!instance.log_text().contains("secret-pass"));
}

#[test]
fn el_login_funciona_con_la_configuracion_recien_guardada() {
    let proxy = fake_proxy();
    let (instance, system) = detected(&proxy, "captive.invalid");
    fs::write(
        instance.layout.config_file(),
        "network.check_url=http://captive.invalid/generate_204\n\
         proxy_authentication_require_https=false\n",
    )
    .unwrap();
    let mut scenario = Scenario::new(&instance, "s\nana\nsecret-pass\n");
    scenario.system = Some(system);
    let run = scenario.run();
    assert!(
        run.out.contains("MITO: autenticación correcta.\n"),
        "{}",
        run.out
    );
    assert!(instance.config().contains("proxy_authentication_url="));
    assert!(!instance.config().contains("secret-pass"));
}

// --- integración con el bootstrap -----------------------------------------------------

use crate::bootstrap::{Downloads, Options, run_with};
use mito_core::Operation;
use mito_engine::Downloader;

/// Descargador falso: anota las URL y escribe un ZIP mínimo.
struct Recording(Arc<Mutex<Vec<String>>>);

impl Downloader for Recording {
    fn download(
        &self,
        url: &str,
        destination: &std::path::Path,
        _expected: Option<&str>,
        _operation: &Operation,
    ) -> mito_transfer::Result<()> {
        self.0.lock().unwrap().push(url.to_owned());
        Err(mito_core::Error::io(destination, std::io::Error::other("sin red en pruebas")).into())
    }
}

fn lock_file(layout: &Layout) {
    fs::create_dir_all(layout.core()).unwrap();
    let lock = serde_json::json!({
        "schema": 1,
        "tools": [{
            "id": "7zip", "version": "1", "required": ["7z.exe"],
            "artifacts": [{"fileName": "7z.exe", "url": "http://tools.invalid/7z.exe",
                "sha256": "00", "install": {"type": "file", "destination": ".", "target": "7z.exe"}}]
        }]
    });
    fs::write(layout.bootstrap_lock(), serde_json::to_vec(&lock).unwrap()).unwrap();
}

#[test]
fn el_bootstrap_decide_el_proxy_antes_de_descargar_y_las_descargas_lo_usan() {
    let proxy = fake_proxy();
    let instance = instance(&config_for(&proxy, "ok.invalid", ""));
    lock_file(&instance.layout);
    let seen = Arc::new(Mutex::new(Vec::<Option<Proxies>>::new()));
    let urls = Arc::new(Mutex::new(Vec::new()));
    let (made, recorded) = (Arc::clone(&seen), Arc::clone(&urls));
    let make = move |proxies: Option<Proxies>| -> Box<dyn Downloader> {
        made.lock().unwrap().push(proxies);
        Box::new(Recording(Arc::clone(&recorded)))
    };
    let system = |_: &str| None;
    let windows = |_: &str, _: &str, _, _| false;
    let services = Services {
        env: Vec::new(),
        system_proxy: &system,
        probe: &probe,
        windows_probe: &windows,
        portal_fields: &portal_fields,
        system_proxies: Some(Proxies::default()),
    };
    let (console, out, _) = console("s\n");
    let code = run_with(
        &instance.layout,
        Options::default(),
        &console,
        &Operation::silent(),
        &Downloads::Proxied {
            make: &make,
            services: &services,
        },
    );
    // La descarga falsa falla (código 1), pero el proxy ya estaba decidido.
    assert_eq!(code, 1);
    let text = out.text();
    let welcome = text.find("¿Deseas continuar?").unwrap();
    let probe_message = text
        .find("MITO: el proxy permite el acceso a Internet.")
        .unwrap();
    assert!(welcome < probe_message, "{text}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].as_ref().unwrap().https.as_deref(),
        Some(proxy.url.as_str())
    );
    assert_eq!(*urls.lock().unwrap(), ["http://tools.invalid/7z.exe"]);
}

#[test]
fn un_proxy_invalido_aborta_el_bootstrap_con_la_fase_proxy_y_sin_descargar() {
    let instance = instance("https_proxy=ftp://x\n");
    lock_file(&instance.layout);
    let urls = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&urls);
    let make = move |_: Option<Proxies>| -> Box<dyn Downloader> {
        Box::new(Recording(Arc::clone(&recorded)))
    };
    let system = |_: &str| None;
    let windows = |_: &str, _: &str, _, _| false;
    let services = Services {
        env: Vec::new(),
        system_proxy: &system,
        probe: &probe,
        windows_probe: &windows,
        portal_fields: &portal_fields,
        system_proxies: Some(Proxies::default()),
    };
    let (console, _, err) = console("");
    let options = Options {
        approved: true,
        preview_welcome: false,
    };
    let code = run_with(
        &instance.layout,
        options,
        &console,
        &Operation::silent(),
        &Downloads::Proxied {
            make: &make,
            services: &services,
        },
    );
    assert_eq!(code, 1);
    assert!(
        err.text()
            .starts_with("MITO: Proxy: usa una URL HTTP/HTTPS sin credenciales\n"),
        "{}",
        err.text()
    );
    assert!(urls.lock().unwrap().is_empty());
    let record = instance
        .records()
        .into_iter()
        .find(|r| r["level"] == "error")
        .unwrap();
    assert_eq!(record["phase"], "proxy");
}

#[test]
fn sin_herramientas_pendientes_no_se_resuelve_el_proxy() {
    let instance = instance("https_proxy=ftp://x\n");
    fs::create_dir_all(instance.layout.core()).unwrap();
    fs::write(
        instance.layout.bootstrap_lock(),
        br#"{"schema":1,"tools":[]}"#,
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let make = move |_: Option<Proxies>| -> Box<dyn Downloader> {
        counted.fetch_add(1, Ordering::SeqCst);
        Box::new(Recording(Arc::default()))
    };
    let system = |_: &str| None;
    let windows = |_: &str, _: &str, _, _| false;
    let services = Services {
        env: Vec::new(),
        system_proxy: &system,
        probe: &probe,
        windows_probe: &windows,
        portal_fields: &portal_fields,
        system_proxies: Some(Proxies::default()),
    };
    let (console, _, err) = console("");
    let code = run_with(
        &instance.layout,
        Options::default(),
        &console,
        &Operation::silent(),
        &Downloads::Proxied {
            make: &make,
            services: &services,
        },
    );
    // Con ftp:// habría fallado de haberse evaluado; no hay nada que descargar.
    assert_eq!(code, 0, "{}", err.text());
}
