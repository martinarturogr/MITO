//! Port de `test_network.py` y de `DiagnoseTests` de `test_network_setup.py` del original,
//! contra servidores y proxies HTTP locales en `127.0.0.1`. Ninguna prueba usa la red real
//! (solo se resuelve el dominio reservado `.test`), el proxy del equipo ni el registro.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mito_core::{Layout, Operation};
use mito_engine::network::{Network, proxies_for_root};
use mito_engine::{
    ConfigNetwork, Connectivity, Credentials, DiagnosisKind, Env, Extensions, HttpDownloader,
    Network as NetworkExtension, apply, diagnose, intercept, proxy_environment,
};
use mito_transfer::network::{Proxies, SystemProxy};

// ---------------------------------------------------------------------------------------
// Servidor HTTP mínimo
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Request {
    /// Ruta, o URL absoluta cuando la petición llega a un proxy.
    path: String,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
    fn body(mut self, body: &str) -> Self {
        self.body = body.as_bytes().to_vec();
        self
    }
}

struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = Arc::clone(&handler);
                        std::thread::spawn(move || serve(stream, &*handler));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { port, stop }
    }

    /// Origen sin barra final: `http://127.0.0.1:puerto`.
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => raw.push(byte[0]),
            _ => return None,
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let mut lines = text.split("\r\n");
    let line = lines.next()?;
    let path = line.split(' ').nth(1)?.to_owned();
    let length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).ok()?;
    Some(Request { path, body })
}

fn serve(mut stream: TcpStream, handler: &(dyn Fn(&Request) -> Response + Send + Sync)) {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let response = handler(&request);
    let mut head = format!(
        "HTTP/1.1 {} X\r\nConnection: close\r\nContent-Length: {}\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).ok();
    stream.write_all(&response.body).ok();
    stream.flush().ok();
}

/// `urllib.parse.parse_qsl` de un cuerpo `application/x-www-form-urlencoded`.
fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    fn decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            let hex = bytes
                .get(index + 1..index + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            match (bytes[index], hex) {
                (b'+', _) => out.push(b' '),
                (b'%', Some(value)) => {
                    out.push(value);
                    index += 2;
                }
                (other, _) => out.push(other),
            }
            index += 1;
        }
        String::from_utf8(out).unwrap()
    }
    String::from_utf8_lossy(body)
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(name), decode(value))
        })
        .collect()
}

fn sorted(mut fields: Vec<(String, String)>) -> Vec<(String, String)> {
    fields.sort();
    fields
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

// ---------------------------------------------------------------------------------------
// Instancia temporal
// ---------------------------------------------------------------------------------------

struct Instance {
    dir: tempfile::TempDir,
    layout: Layout,
}

impl Instance {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        Self { dir, layout }
    }

    fn config(&self, text: &str) {
        fs::write(self.layout.config_file(), text).unwrap();
    }

    fn append(&self, text: &str) {
        let mut current = fs::read_to_string(self.layout.config_file()).unwrap();
        current.push('\n');
        current.push_str(text);
        self.config(&current);
    }

    /// Red sin proxies del sistema (ni el entorno ni el registro del equipo).
    fn network(&self) -> Network {
        Network::with_system(&self.layout, None, Proxies::default()).unwrap()
    }

    /// Contenido de todos los archivos salvo `config.properties`.
    fn logs(&self) -> String {
        let mut text = String::new();
        for (path, bytes) in files(self.dir.path()) {
            if path.file_name().is_some_and(|n| n != "config.properties") {
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        text
    }
}

fn files(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            let bytes = fs::read(&path).unwrap();
            found.push((path, bytes));
        }
    }
    found
}

fn assert_no_secret_on_disk(instance: &Instance, secret: &str) {
    for (path, bytes) in files(instance.dir.path()) {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(secret),
            "{} contiene el secreto",
            path.display()
        );
    }
}

fn credentials() -> Credentials {
    Credentials::new("test-user", "test-password")
}

// ---------------------------------------------------------------------------------------
// test_network.py
// ---------------------------------------------------------------------------------------

#[test]
fn proxy_routing_and_explicit_empty() {
    let instance = Instance::new();
    instance.config("http_proxy=http://proxy.example:8080\nhttps_proxy=\nno_proxy=localhost");
    let mut env: Env = [("http_proxy", "old"), ("HTTPS_PROXY", "old")]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let values = mito_core::config::read_global(&instance.layout).unwrap();
    proxy_environment(&values, &mut env).unwrap();
    assert_eq!(env["HTTP_PROXY"], "http://proxy.example:8080");
    assert_eq!(env["HTTPS_PROXY"], "");
    assert!(!env.contains_key("http_proxy"));

    let network = Network::with_system(&instance.layout, Some(&env), Proxies::default()).unwrap();
    let proxies = network.proxies();
    assert_eq!(proxies.http.as_deref(), Some("http://proxy.example:8080"));
    assert_eq!(proxies.https.as_deref(), Some(""));
    assert_eq!(proxies.no_proxy.as_deref(), Some("localhost"));

    // El entorno del proceso, con la configuración aplicada, da lo mismo.
    let from_process = instance.network().proxies();
    assert_eq!(from_process.http, proxies.http);
    assert_eq!(from_process.https, proxies.https);

    instance.config("http_proxy=http://user:secret@proxy.example");
    let error = Network::open(&instance.layout, None).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Configura una URL HTTP/HTTPS sin credenciales"
    );
    let mut env = Env::new();
    let error = ConfigNetwork
        .proxy_environment(&instance.layout, &mut env)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Configura una URL HTTP/HTTPS sin credenciales"
    );
}

#[derive(Default)]
struct Portal {
    authenticated: bool,
    posts: usize,
}

#[test]
fn portal_post_hidden_fields_and_independent_connectivity() {
    let state = Arc::new(Mutex::new(Portal::default()));
    let shared = Arc::clone(&state);
    let server = Server::start(move |req| {
        let mut state = shared.lock().unwrap();
        match (req.body.is_empty(), req.path.as_str()) {
            (true, "/login") => Response::new(200).body(
                r#"<form action="/login"><input name="token" value="abc"><input name="password" type="password"></form>"#,
            ),
            (true, "/redirect") => Response::new(302).header("Location", "/login"),
            (true, _) => Response::new(if state.authenticated { 204 } else { 200 }),
            (false, _) => {
                state.posts += 1;
                state.authenticated = sorted(parse_form(&req.body))
                    == sorted(pairs(&[
                        ("token", "abc"),
                        ("username", "test-user"),
                        ("password", "test-password"),
                        ("extra", "value"),
                    ]));
                Response::new(302).header("Location", "/check")
            }
        }
    });
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "http_proxy=\nhttps_proxy=\nno_proxy=127.0.0.1\nnetwork.check_url={url}/check\nnetwork.timeout_seconds=1\nenable_proxy_authentication=true\nproxy_authentication_url={url}/login\nproxy_authentication_form_field.extra=value"
    ));
    let network = instance.network();
    assert!(!network.check(true));
    let error = network.authenticate(&credentials()).unwrap_err();
    assert!(
        error.to_string().starts_with("El POST requiere HTTPS"),
        "{error}"
    );
    assert_eq!(state.lock().unwrap().posts, 0);

    instance.append("proxy_authentication_require_https=false");
    let network = instance.network();
    assert!(network.authenticate(&credentials()).unwrap());
    assert_eq!(state.lock().unwrap().posts, 1);

    // Una comprobación contra una URL que redirige no es Internet.
    instance.append(&format!("network.check_url={url}/redirect"));
    assert!(!instance.network().check(true));
    assert_no_secret_on_disk(&instance, "test-password");
}

#[test]
fn portal_url_is_rediscovered_because_saved_redirects_expire() {
    #[derive(Default)]
    struct State {
        authenticated: bool,
        session: u32,
        posts: Vec<Vec<(String, String)>>,
    }
    let state = Arc::new(Mutex::new(State::default()));
    let shared = Arc::clone(&state);
    let server = Server::start(move |req| {
        let mut state = shared.lock().unwrap();
        if !req.body.is_empty() {
            let fields = parse_form(&req.body);
            state.authenticated = sorted(fields.clone())
                == sorted(pairs(&[
                    ("magic", &state.session.to_string()),
                    ("username", "test-user"),
                    ("password", "test-password"),
                ]));
            state.posts.push(fields);
            return Response::new(200);
        }
        let (path, query) = req.path.split_once('?').unwrap_or((req.path.as_str(), ""));
        let current = format!("magic={}", state.session);
        if state.authenticated {
            Response::new(204)
        } else if path == "/check" {
            state.session += 1;
            Response::new(302).header("Location", &format!("/portal?magic={}", state.session))
        } else if path == "/portal" && query == current {
            Response::new(302).header("Location", &format!("/fgtauth?{query}"))
        } else if path == "/fgtauth" && query == current {
            Response::new(200).body(&format!(
                r#"<form action="/"><input type="hidden" name="magic" value="{}"><input name="username"><input name="password" type="password"></form>"#,
                state.session
            ))
        } else {
            Response::new(400)
        }
    });
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "http_proxy=\nhttps_proxy=\nno_proxy=127.0.0.1\nnetwork.check_url={url}/check\nnetwork.timeout_seconds=1\nenable_proxy_authentication=true\nproxy_authentication_require_https=false\nproxy_authentication_url={url}/portal?magic=expired"
    ));
    assert!(instance.network().authenticate(&credentials()).unwrap());
    assert_eq!(state.lock().unwrap().posts.len(), 1);
    assert_no_secret_on_disk(&instance, "test-password");
}

#[test]
fn failed_portal_login_is_logged_without_secrets() {
    let server = Server::start(|_| Response::new(400));
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "http_proxy=\nhttps_proxy=\nno_proxy=127.0.0.1\nnetwork.check_url={url}/check\nnetwork.timeout_seconds=1\nenable_proxy_authentication=true\nproxy_authentication_require_https=false\nproxy_authentication_url={url}/portal?magic=secret-token"
    ));
    assert!(!instance.network().authenticate(&credentials()).unwrap());
    let logs = instance.logs();
    assert!(logs.contains("Portal page unavailable"), "{logs}");
    assert!(logs.contains(r#""status": 400"#), "{logs}");
    assert!(logs.contains(r#""portal": ["http", "127.0.0.1:"#), "{logs}");
    assert!(!logs.contains("secret-token"), "{logs}");
    assert!(!logs.contains("test-password"), "{logs}");
    assert!(!logs.contains("/portal"), "{logs}");
}

/// `Network` simulada por el test: comprobación y red local fijas.
struct Fake {
    online: bool,
    local: bool,
    checks: Cell<u32>,
}

impl Fake {
    fn new(online: bool, local: bool) -> Self {
        Self {
            online,
            local,
            checks: Cell::new(0),
        }
    }
}

impl Connectivity for Fake {
    fn check(&self, _log: bool) -> bool {
        self.checks.set(self.checks.get() + 1);
        self.online
    }
    fn local_network(&self) -> bool {
        self.local
    }
}

/// Servidor de portal que acepta `test-user` / `secret` y cuenta los POST.
fn portal_server() -> (Server, Arc<Mutex<Portal>>) {
    let state = Arc::new(Mutex::new(Portal::default()));
    let shared = Arc::clone(&state);
    let server = Server::start(move |req| {
        let mut state = shared.lock().unwrap();
        if !req.body.is_empty() {
            state.posts += 1;
            let fields = parse_form(&req.body);
            state.authenticated = fields.contains(&("password".to_owned(), "secret".to_owned()));
            return Response::new(302).header("Location", "/check");
        }
        match req.path.as_str() {
            "/login" => Response::new(200).body(
                r#"<form action="/login"><input name="username"><input name="password" type="password"></form>"#,
            ),
            _ => Response::new(if state.authenticated { 204 } else { 200 }),
        }
    });
    (server, state)
}

fn portal_instance(server: &Server) -> Instance {
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "http_proxy=\nhttps_proxy=\nno_proxy=127.0.0.1\nnetwork.check_url={url}/check\nnetwork.timeout_seconds=1\nenable_proxy_authentication=true\nproxy_authentication_require_https=false\nproxy_authentication_url={url}/login"
    ));
    instance
}

#[test]
fn interceptor_cancellation_offline_and_success() {
    let (server, state) = portal_server();
    let instance = portal_instance(&server);
    let network = instance.network();
    let layout = &instance.layout;

    // (a) Sin Internet, con red local y portal activado: pregunta una vez; cancelar
    // continúa sin autenticar.
    let prompts = RefCell::new(Vec::new());
    let mut cancel = |failed: bool| {
        prompts.borrow_mut().push(failed);
        None
    };
    mito_engine::network::intercept_with(layout, &network, &Fake::new(false, true), &mut cancel);
    assert_eq!(*prompts.borrow(), [false]);
    assert_eq!(state.lock().unwrap().posts, 0);

    // (b) Con Internet no pregunta.
    prompts.borrow_mut().clear();
    let mut cancel = |failed: bool| {
        prompts.borrow_mut().push(failed);
        None
    };
    let online = Fake::new(true, true);
    mito_engine::network::intercept_with(layout, &network, &online, &mut cancel);
    assert!(prompts.borrow().is_empty());
    assert_eq!(online.checks.get(), 1);

    // (c) Sin red local no pregunta.
    mito_engine::network::intercept_with(layout, &network, &Fake::new(false, false), &mut cancel);
    assert!(prompts.borrow().is_empty());

    // (d) Credenciales correctas: autentica una vez con ellas.
    let mut right = |failed: bool| {
        prompts.borrow_mut().push(failed);
        Some(Credentials::new("test", "secret"))
    };
    mito_engine::network::intercept_with(layout, &network, &Fake::new(false, true), &mut right);
    assert_eq!(*prompts.borrow(), [false]);
    assert_eq!(state.lock().unwrap().posts, 1);
    assert!(state.lock().unwrap().authenticated);
}

#[test]
fn interceptor_retries_with_failed_flag_until_cancelled_or_accepted() {
    let (server, state) = portal_server();
    let instance = portal_instance(&server);
    let network = instance.network();
    let attempts = RefCell::new(Vec::new());
    let mut prompt = |failed: bool| {
        let mut attempts = attempts.borrow_mut();
        attempts.push(failed);
        match attempts.len() {
            1 => Some(Credentials::new("test", "mala")),
            2 => Some(Credentials::new("test", "secret")),
            _ => None,
        }
    };
    mito_engine::network::intercept_with(
        &instance.layout,
        &network,
        &Fake::new(false, true),
        &mut prompt,
    );
    assert_eq!(*attempts.borrow(), [false, true]);
    assert_eq!(state.lock().unwrap().posts, 2);
    assert!(instance.logs().contains("Internet still unavailable"));
}

#[test]
fn interceptor_logs_configuration_errors_instead_of_hiding_them() {
    // F1: HTTPS obligatorio (portal HTTP sin `proxy_authentication_require_https=false`).
    let (server, state) = portal_server();
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "http_proxy=\nhttps_proxy=\nno_proxy=127.0.0.1\nnetwork.check_url={url}/check\nnetwork.timeout_seconds=1\nenable_proxy_authentication=true\nproxy_authentication_url={url}/login"
    ));
    let calls = Cell::new(0);
    let mut prompt = |_: bool| {
        calls.set(calls.get() + 1);
        Some(Credentials::new("test", "clave-que-no-debe-salir"))
    };
    let env: Env = std::env::vars().collect();
    intercept(&instance.layout, Some(&env), &mut prompt);
    // La red real del equipo no interviene: la comprobación va a `127.0.0.1` y falla; el
    // prompt se invoca una vez y el error no se repite.
    assert_eq!(calls.get(), 1);
    assert_eq!(state.lock().unwrap().posts, 0);
    let logs = instance.logs();
    assert!(
        logs.contains(r#""operation": "network.authenticate""#),
        "{logs}"
    );
    assert!(logs.contains("El POST requiere HTTPS"), "{logs}");
    assert!(!logs.contains("clave-que-no-debe-salir"), "{logs}");
}

#[test]
fn interceptor_never_fails_and_logs_an_invalid_configuration() {
    let instance = Instance::new();
    instance.config("network.timeout_seconds=abc\n");
    let mut prompt = |_: bool| -> Option<Credentials> { panic!("no debe preguntar") };
    intercept(&instance.layout, None, &mut prompt);
    let logs = instance.logs();
    assert!(logs.contains("network.timeout_seconds"), "{logs}");
    // Archivo que no es UTF-8.
    fs::write(instance.layout.config_file(), [0xff, 0xfe, 0x41]).unwrap();
    intercept(&instance.layout, None, &mut prompt);
}

// ---------------------------------------------------------------------------------------
// test_network_setup.py (DiagnoseTests)
// ---------------------------------------------------------------------------------------

const CHECK: &str = "http://check.test/generate_204";
const PORTAL: &str = "http://portal.test/login";

#[derive(Clone, Copy)]
enum Mode {
    Ok,
    Auth,
    Portal,
}

/// Proxy HTTP local: recibe URLs absolutas y responde según el modo.
fn proxy(mode: Mode) -> Server {
    Server::start(move |req| {
        match mode {
        Mode::Auth => Response::new(407)
            .header("Proxy-Authenticate", "Negotiate")
            .header("Proxy-Authenticate", "NTLM"),
        _ if req.path == PORTAL => Response::new(200).body(
            r#"<form action="/login"><input name="token" type="hidden" value="x"><input name="user" type="text"><input name="pass" type="password"></form>"#,
        ),
        Mode::Portal => Response::new(302).header("Location", PORTAL),
        Mode::Ok => Response::new(204),
    }
    })
}

fn diag_instance(extra: &str) -> Instance {
    let instance = Instance::new();
    instance.config(&format!(
        "# comentario\nnetwork.check_url={CHECK}\nnetwork.timeout_seconds=2\n{extra}"
    ));
    instance
}

fn found(url: &str, source: &str, no_proxy: &str) -> SystemProxy {
    SystemProxy {
        url: url.to_owned(),
        source: source.to_owned(),
        no_proxy: no_proxy.to_owned(),
    }
}

#[test]
fn stale_configured_proxy_is_replaced_by_the_windows_route() {
    let proxy = proxy(Mode::Ok);
    let instance = diag_instance("http_proxy=http://127.0.0.1:1\nhttps_proxy=http://127.0.0.1:1\n");
    let network = instance.network();
    let route = proxy.url();
    let result = diagnose(&network, &Fake::new(false, true), &|_| {
        Some(found(&route, "pac", "localhost"))
    });
    assert_eq!(
        (result.kind, result.proxy.as_str(), result.source.as_str()),
        (DiagnosisKind::Ok, route.as_str(), "pac")
    );
    assert_eq!(
        result.changes,
        pairs(&[
            ("http_proxy", &route),
            ("https_proxy", &route),
            ("no_proxy", "localhost")
        ])
    );
}

#[test]
fn portal_behind_detected_proxy_proposes_portal_keys_without_credentials() {
    let proxy = proxy(Mode::Portal);
    let instance = diag_instance("");
    let route = proxy.url();
    let result = diagnose(&instance.network(), &Fake::new(false, true), &|_| {
        Some(found(&route, "wpad", "localhost"))
    });
    assert_eq!(
        (result.kind, result.portal.as_str()),
        (DiagnosisKind::Portal, PORTAL)
    );
    assert_eq!(
        result.changes,
        pairs(&[
            ("http_proxy", &route),
            ("https_proxy", &route),
            ("no_proxy", "localhost"),
            ("proxy_authentication_url", PORTAL),
            ("proxy_authentication_username_field", "user"),
            ("proxy_authentication_password_field", "pass"),
            ("enable_proxy_authentication", "true"),
        ])
    );
}

#[test]
fn configured_portal_needs_no_changes() {
    let proxy = proxy(Mode::Portal);
    let route = proxy.url();
    let instance = diag_instance(&format!(
        "http_proxy={route}\nhttps_proxy={route}\nenable_proxy_authentication=true\nproxy_authentication_url={PORTAL}\n"
    ));
    let result = diagnose(&instance.network(), &Fake::new(false, true), &|_| None);
    assert_eq!(result.kind, DiagnosisKind::Portal);
    assert_eq!(result.changes, []);
}

#[test]
fn proxy_407_is_reported_and_not_saved() {
    let proxy = proxy(Mode::Auth);
    let instance = diag_instance("");
    let route = proxy.url();
    let result = diagnose(&instance.network(), &Fake::new(false, true), &|_| {
        Some(found(&route, "manual", ""))
    });
    assert_eq!(
        (result.kind, result.status, result.authenticate.as_str()),
        (DiagnosisKind::Auth, 407, "NTLM, Negotiate")
    );
    assert_eq!(result.changes, []);
}

#[test]
fn offline_and_online_short_circuit() {
    let instance = diag_instance("");
    let network = instance.network();
    let resolved = Cell::new(false);
    let resolve = |_: &str| {
        resolved.set(true);
        None
    };
    let result = diagnose(&network, &Fake::new(false, false), &resolve);
    assert_eq!(result.kind, DiagnosisKind::Offline);
    let result = diagnose(&network, &Fake::new(true, true), &resolve);
    assert_eq!(result.kind, DiagnosisKind::Ok);
    assert!(!resolved.get());
}

#[test]
fn an_unconfigured_failure_without_system_proxy_tries_the_direct_route() {
    // Sin proxies configurados ni del sistema solo queda la ruta directa; con el proxy
    // 404 del propio servidor no se llega a `.test`.
    let server = Server::start(|_| Response::new(403));
    let url = server.url();
    let instance = Instance::new();
    instance.config(&format!(
        "network.check_url={url}/x\nnetwork.timeout_seconds=2\n"
    ));
    let result = diagnose(&instance.network(), &Fake::new(false, true), &|_| None);
    assert_eq!(result.kind, DiagnosisKind::Blocked);
    assert_eq!((result.source.as_str(), result.status), ("direct", 403));
    assert_eq!(result.changes, []);
}

#[test]
fn apply_keeps_comments_and_other_keys() {
    let instance = diag_instance("terminal.defaultShell=powershell\n");
    apply(
        &instance.layout,
        &pairs(&[
            ("https_proxy", "http://proxy.example:8080"),
            ("no_proxy", "localhost"),
        ]),
    )
    .unwrap();
    let text = fs::read_to_string(instance.layout.config_file()).unwrap();
    assert!(text.contains("# comentario"), "{text}");
    let values = mito_core::config::parse(&text);
    assert_eq!(values.get("terminal.defaultShell"), Some("powershell"));
    assert_eq!(values.get("https_proxy"), Some("http://proxy.example:8080"));
    assert_eq!(values.get("no_proxy"), Some("localhost"));
    assert_eq!(values.get("network.check_url"), Some(CHECK));
}

#[test]
fn apply_rejects_invalid_values_without_touching_the_file() {
    let instance = diag_instance("");
    let before = fs::read(instance.layout.config_file()).unwrap();
    assert!(apply(&instance.layout, &pairs(&[("http_proxy", "ftp://x")])).is_err());
    assert_eq!(fs::read(instance.layout.config_file()).unwrap(), before);
}

// ---------------------------------------------------------------------------------------
// Integración con el motor
// ---------------------------------------------------------------------------------------

#[test]
fn the_default_extension_reads_the_configuration_on_every_call() {
    let instance = Instance::new();
    let extensions = Extensions::default();
    let mut env = Env::new();
    extensions
        .network
        .proxy_environment(&instance.layout, &mut env)
        .unwrap();
    assert!(env.is_empty());
    instance.config("http_proxy=http://p:1\nno_proxy=\n");
    extensions
        .network
        .proxy_environment(&instance.layout, &mut env)
        .unwrap();
    assert_eq!(env["HTTP_PROXY"], "http://p:1");
    assert_eq!(env["NO_PROXY"], "");
    instance.config("http_proxy=\n");
    extensions
        .network
        .proxy_environment(&instance.layout, &mut env)
        .unwrap();
    assert_eq!(env["HTTP_PROXY"], "");
}

#[test]
fn downloads_for_a_root_resolve_the_proxies_on_every_download() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let proxy = Server::start(move |req| {
        log.lock().unwrap().push(req.path.clone());
        Response::new(200).body("por-proxy")
    });
    let target = Server::start(|_| Response::new(200).body("directo"));
    let instance = Instance::new();
    let downloader = HttpDownloader::for_root_with_system(&instance.layout, Proxies::default());
    let file = instance.dir.path().join("descarga");
    let url = format!("{}/archivo", target.url());
    let fetch = || {
        mito_engine::Downloader::download(&downloader, &url, &file, None, &Operation::detached())
    };

    instance.config(&format!("http_proxy={}\nno_proxy=\n", proxy.url()));
    fetch().unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"por-proxy");
    assert_eq!(*seen.lock().unwrap(), std::slice::from_ref(&url));

    // Cambia la configuración: la siguiente descarga ya la ve.
    instance.config("http_proxy=\nhttps_proxy=\nno_proxy=\n");
    fetch().unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"directo");

    instance.config(&format!("http_proxy={}\nno_proxy=127.0.0.1\n", proxy.url()));
    fetch().unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"directo");
    assert_eq!(seen.lock().unwrap().len(), 1);

    // Un proxy inválido es el error de la descarga.
    instance.config("http_proxy=http://user:secret@p:1\n");
    let error = fetch().unwrap_err();
    assert_eq!(
        error.to_string(),
        "Configura una URL HTTP/HTTPS sin credenciales"
    );
}

#[test]
fn the_system_proxies_are_summed_with_the_configuration() {
    let instance = Instance::new();
    instance.config("no_proxy=config.test\n");
    let system = Proxies::from_environment(&[
        ("HTTP_PROXY".to_owned(), "http://sistema:1".to_owned()),
        ("NO_PROXY".to_owned(), "sistema.test".to_owned()),
    ]);
    let proxies = proxies_for_root(&instance.layout, Some(&system)).unwrap();
    assert_eq!(proxies.http.as_deref(), Some("http://sistema:1"));
    assert_eq!(proxies.no_proxy.as_deref(), Some("config.test"));
    // Las exclusiones del sistema se suman a las de la configuración (F4 del original).
    for host in ["http://a.config.test/", "http://a.sistema.test/"] {
        let url = mito_transfer::network::validate_url(host).unwrap();
        assert_eq!(proxies.proxy_for(&url), None, "{host}");
    }
    let other = mito_transfer::network::validate_url("http://otro.test/").unwrap();
    assert_eq!(
        proxies.proxy_for(&other).as_deref(),
        Some("http://sistema:1")
    );
}

// ---------------------------------------------------------------------------------------
// Secretos
// ---------------------------------------------------------------------------------------

#[test]
fn the_password_is_nowhere_on_disk_nor_in_any_debug_output() {
    let (server, _) = portal_server();
    let instance = portal_instance(&server);
    let secret = "S3creta-única-0451";
    let network = instance.network();
    let diagnosis = diagnose(&network, &Fake::new(false, true), &|_| None);
    // La autenticación falla (la contraseña no es la que acepta el portal) y no deja rastro.
    let wrong = Credentials::new("test-user", secret);
    assert!(!network.authenticate(&wrong).unwrap());
    let _ = intercept_prompt_once(&instance, secret);
    let debug = format!("{wrong:?} {wrong:#?} {network:?} {diagnosis:?}");
    assert!(!debug.contains(secret), "{debug}");
    assert_no_secret_on_disk(&instance, secret);
    assert!(!instance.logs().contains(secret));
}

fn intercept_prompt_once(instance: &Instance, secret: &str) -> bool {
    let attempts = Cell::new(0);
    let mut prompt = |_: bool| {
        attempts.set(attempts.get() + 1);
        (attempts.get() == 1).then(|| Credentials::new("test-user", secret))
    };
    let network = instance.network();
    mito_engine::network::intercept_with(
        &instance.layout,
        &network,
        &Fake::new(false, true),
        &mut prompt,
    );
    attempts.get() == 2
}
