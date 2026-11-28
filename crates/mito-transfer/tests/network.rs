//! Sesión HTTP, sondas y proxies de `mito_transfer::network` contra servidores y proxies
//! HTTP locales en `127.0.0.1`. Ninguna prueba usa la red real ni el proxy del equipo.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Request, Server, ok, redirect, respond};
use mito_core::Operation;
use mito_transfer::network::{ProbeKind, Proxies, Session, SessionOptions, portal_fields, probe};
use mito_transfer::{DownloadOptions, download};

const TIMEOUT: Duration = Duration::from_secs(5);

fn session(proxies: Proxies, cookies: bool) -> Session {
    Session::new(&SessionOptions {
        proxies,
        timeout: TIMEOUT,
        cookies,
    })
    .unwrap()
}

fn proxy_of(server: &Server) -> String {
    format!("http://127.0.0.1:{}", server.port)
}

/// Rutas pedidas, en orden.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Seen {
    fn push(&self, request: &Request) {
        self.0
            .lock()
            .unwrap()
            .push(format!("{} {}", request.method, request.path));
    }
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[test]
fn la_sesion_no_sigue_redirecciones_y_devuelve_el_location() {
    let server = Server::http(|req, out| {
        if req.path == "/start" {
            redirect(out, 302, "/final");
        } else {
            ok(out, &[], b"final");
        }
    });
    let reply = session(Proxies::default(), false)
        .get(&server.url("/start"))
        .unwrap();
    assert_eq!(reply.status, 302);
    assert_eq!(reply.location.as_deref(), Some("/final"));
    assert!(!reply.is_success());
    let direct = session(Proxies::default(), false)
        .get(&server.url("/final"))
        .unwrap();
    assert!(direct.is_success());
    assert_eq!(direct.body, b"final");
}

#[test]
fn el_cuerpo_se_limita_a_1_mib() {
    let big = vec![b'x'; 3 * 1024 * 1024];
    let server = Server::http(move |_, out| ok(out, &[], &big));
    let reply = session(Proxies::default(), false)
        .get(&server.url("/"))
        .unwrap();
    assert_eq!(reply.body.len(), mito_transfer::network::MAX_BODY);
}

#[test]
fn las_cookies_solo_viajan_si_la_sesion_las_guarda() {
    let server = Server::http(|req, out| {
        if req.path == "/set" {
            ok(out, &[("Set-Cookie", "sid=abc; Path=/")], b"");
        } else {
            let has = req.header("cookie").is_some_and(|c| c.contains("sid=abc"));
            ok(out, &[], if has { b"con" } else { b"sin" });
        }
    });
    for (cookies, expected) in [(true, "con"), (false, "sin")] {
        let session = session(Proxies::default(), cookies);
        session.get(&server.url("/set")).unwrap();
        let reply = session.get(&server.url("/leer")).unwrap();
        assert_eq!(String::from_utf8_lossy(&reply.body), expected);
    }
}

#[test]
fn el_post_envia_un_formulario_codificado() {
    let seen = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&seen);
    let server = Server::http(move |req, out| {
        *sink.lock().unwrap() = Some((
            req.method.clone(),
            req.header("content-type").map(str::to_owned),
            String::from_utf8_lossy(&req.body).into_owned(),
        ));
        redirect(out, 303, "/check");
    });
    let fields = [
        ("csrf".to_owned(), "tok&en".to_owned()),
        ("user".to_owned(), "ana maría".to_owned()),
    ];
    let reply = session(Proxies::default(), false)
        .post_form(&server.url("/login"), &fields)
        .unwrap();
    assert_eq!(reply.status, 303);
    let (method, content_type, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(method, "POST");
    assert_eq!(
        content_type.as_deref(),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(body, "csrf=tok%26en&user=ana+mar%C3%ADa");
}

#[test]
fn con_proxy_el_servidor_recibe_la_url_absoluta_y_con_bypass_va_directo() {
    let seen = Seen::default();
    let log = seen.clone();
    let proxy = Server::http(move |req, out| {
        log.push(req);
        ok(out, &[], b"por-proxy");
    });
    let target = Server::http(|_, out| ok(out, &[], b"directo"));
    let url = target.url("/p?q=1");
    let proxies = Proxies::single(&proxy_of(&proxy));
    let reply = session(proxies, false).get(&url).unwrap();
    assert_eq!(reply.body, b"por-proxy");
    assert_eq!(seen.all(), [format!("GET {url}")]);

    let bypass = Proxies {
        http: Some(proxy_of(&proxy)),
        no_proxy: Some("127.0.0.1".to_owned()),
        ..Proxies::default()
    };
    let reply = session(bypass, false).get(&url).unwrap();
    assert_eq!(reply.body, b"directo");
    assert_eq!(seen.all().len(), 1);

    // Una entrada vacía anula el proxy heredado.
    let empty = Proxies::default().overridden(&[("HTTP_PROXY".to_owned(), String::new())]);
    assert_eq!(session(empty, false).get(&url).unwrap().body, b"directo");
}

#[test]
fn se_capturan_todas_las_cabeceras_proxy_authenticate() {
    let proxy = Server::http(|_, out| {
        respond(
            out,
            407,
            &[
                ("Proxy-Authenticate", "Negotiate"),
                ("Proxy-Authenticate", "NTLM"),
                ("Content-Length", "0"),
            ],
            b"",
        );
    });
    let reply = session(Proxies::single(&proxy_of(&proxy)), false)
        .get("http://destino.invalid/")
        .unwrap();
    assert_eq!(reply.status, 407);
    assert_eq!(reply.proxy_authenticate, ["Negotiate", "NTLM"]);
}

#[test]
fn las_sondas_clasifican_ok_auth_portal_y_bloqueo() {
    let direct = Server::http(|req, out| match req.path.as_str() {
        "/204" => respond(out, 204, &[("Content-Length", "0")], b""),
        "/portal" => redirect(out, 302, "http://captive.invalid/login"),
        "/blocked" => respond(out, 403, &[("Content-Length", "0")], b""),
        _ => ok(out, &[], b"200"),
    });
    let kind = |path: &str, expected: u16| {
        let result = probe(&direct.url(path), "", TIMEOUT);
        (result.status, result.kind(expected), result)
    };
    assert_eq!(kind("/204", 204).1, ProbeKind::Ok);
    let (status, portal, result) = kind("/portal", 204);
    assert_eq!((status, portal), (302, ProbeKind::Portal));
    assert_eq!(result.location, "http://captive.invalid/login");
    assert_eq!(result.url, direct.url("/portal"));
    assert_eq!(kind("/blocked", 204).1, ProbeKind::Blocked);
    assert_eq!(kind("/x", 204).1, ProbeKind::Blocked);
    assert_eq!(kind("/x", 200).1, ProbeKind::Ok);
}

#[test]
fn la_sonda_a_traves_de_un_proxy_con_407_informa_los_esquemas() {
    let proxy = Server::http(|_, out| {
        respond(
            out,
            407,
            &[
                ("Proxy-Authenticate", "Negotiate"),
                ("Proxy-Authenticate", "NTLM"),
                ("Content-Length", "0"),
            ],
            b"",
        );
    });
    let result = probe("http://check.invalid/204", &proxy_of(&proxy), TIMEOUT);
    assert_eq!(result.status, 407);
    assert_eq!(result.authenticate, "NTLM, Negotiate");
    assert_eq!(result.kind(204), ProbeKind::Auth);
}

#[test]
fn la_sonda_nunca_aplica_las_exclusiones_del_sistema() {
    // F7: aunque el host sea local, el candidato se prueba de verdad.
    let seen = Seen::default();
    let log = seen.clone();
    let proxy = Server::http(move |req, out| {
        log.push(req);
        respond(out, 204, &[("Content-Length", "0")], b"");
    });
    let result = probe("http://localhost/204", &proxy_of(&proxy), TIMEOUT);
    assert_eq!(result.kind(204), ProbeKind::Ok);
    assert_eq!(seen.all(), ["GET http://localhost/204"]);
}

#[test]
fn el_connect_rechazado_por_el_proxy_se_clasifica_por_su_estado() {
    let proxy = Server::http(|req, out| {
        assert_eq!(req.method, "CONNECT");
        respond(
            out,
            407,
            &[("Proxy-Authenticate", "NTLM"), ("Content-Length", "0")],
            b"",
        );
    });
    let result = probe("https://check.invalid/204", &proxy_of(&proxy), TIMEOUT);
    assert_eq!(result.status, 407, "{result:?}");
    // El estado del CONNECT es lo único que se conserva: sin esquemas.
    assert_eq!(result.authenticate, "");
    assert_eq!(result.kind(204), ProbeKind::Auth);

    let blocked = Server::http(|_, out| {
        respond(out, 403, &[("Content-Length", "0")], b"");
    });
    let result = probe("https://check.invalid/204", &proxy_of(&blocked), TIMEOUT);
    assert_eq!(result.kind(204), ProbeKind::Blocked, "{result:?}");
}

#[test]
fn sin_servidor_o_con_proxy_caido_la_sonda_es_inalcanzable() {
    // Puerto cerrado: se reserva y se suelta.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let closed = format!("http://127.0.0.1:{port}");
    let direct = probe(&format!("{closed}/204"), "", TIMEOUT);
    assert_eq!(
        (direct.status, direct.kind(204)),
        (0, ProbeKind::Unreachable)
    );
    assert_eq!(direct.error, "URLError");
    let target = Server::http(|_, out| ok(out, &[], b""));
    let proxied = probe(&target.url("/204"), &closed, TIMEOUT);
    assert_eq!(proxied.kind(204), ProbeKind::Unreachable);
}

#[test]
fn portal_fields_solo_devuelve_nombres_distintos_de_los_predeterminados() {
    let server = Server::http(|req, out| {
        match req.path.as_str() {
        "/custom" => ok(
            out,
            &[],
            br#"<form action="/login"><input name="token" type="hidden" value="x"><input name="user" type="text"><input name="pass" type="password"></form>"#,
        ),
        "/default" => ok(
            out,
            &[],
            br#"<form><input name="username"><input name="password" type="password"></form>"#,
        ),
        "/only-user" => ok(
            out,
            &[],
            br#"<form><input name="login"><input name="password" type="password"></form>"#,
        ),
        "/nopassword" => ok(out, &[], br#"<form><input name="q"></form>"#),
        "/redirect" => redirect(out, 302, "/custom"),
        _ => respond(out, 404, &[("Content-Length", "0")], b""),
    }
    });
    let fields = |path: &str| portal_fields(&server.url(path), "", TIMEOUT);
    assert_eq!(
        fields("/custom"),
        [
            (
                "proxy_authentication_username_field".to_owned(),
                "user".to_owned()
            ),
            (
                "proxy_authentication_password_field".to_owned(),
                "pass".to_owned()
            ),
        ]
    );
    assert_eq!(fields("/default"), []);
    assert_eq!(
        fields("/only-user"),
        [(
            "proxy_authentication_username_field".to_owned(),
            "login".to_owned()
        )]
    );
    assert_eq!(fields("/nopassword"), []);
    // No sigue redirecciones ni lee páginas de error.
    assert_eq!(fields("/redirect"), []);
    assert_eq!(fields("/404"), []);
}

fn fast(proxies: Option<Proxies>) -> DownloadOptions {
    DownloadOptions {
        attempts: 1,
        delay: Duration::ZERO,
        proxies,
        ..DownloadOptions::default()
    }
}

#[test]
fn las_descargas_usan_los_proxies_de_las_opciones() {
    let seen = Seen::default();
    let log = seen.clone();
    let proxy = Server::http(move |req, out| {
        log.push(req);
        ok(out, &[], b"por-proxy");
    });
    let target = Server::http(|_, out| ok(out, &[], b"directo"));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f");
    let url = target.url("/archivo");

    let via_proxy = fast(Some(Proxies::single(&proxy_of(&proxy))));
    download(&url, &file, None, &via_proxy, &Operation::detached()).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"por-proxy");
    assert_eq!(seen.all(), [format!("GET {url}")]);

    let excluded = Proxies::single(&proxy_of(&proxy))
        .overridden(&[("no_proxy".to_owned(), "127.0.0.1".to_owned())]);
    download(
        &url,
        &file,
        None,
        &fast(Some(excluded)),
        &Operation::detached(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"directo");
    assert_eq!(seen.all().len(), 1);

    download(
        &url,
        &file,
        None,
        &fast(Some(Proxies::default())),
        &Operation::detached(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"directo");
}
