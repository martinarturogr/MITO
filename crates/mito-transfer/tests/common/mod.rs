//! Servidores HTTP/HTTPS mínimos para los tests (equivalen al `ThreadingHTTPServer` de
//! los tests del original). Cada conexión atiende una sola petición y se cierra, como
//! HTTP/1.0: así un handler puede declarar un `Content-Length` mayor que lo que envía
//! para simular un corte.
#![allow(dead_code, reason = "cada test usa una parte distinta")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Petición recibida: método, ruta (o URL absoluta si llega a un proxy), cabeceras
/// (nombres en minúsculas) y cuerpo.
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

type Handler = dyn Fn(&Request, &mut dyn Write) + Send + Sync;

/// Servidor en `127.0.0.1:0` que se detiene al soltarlo.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
    scheme: &'static str,
}

impl Server {
    /// Servidor HTTP.
    pub fn http(handler: impl Fn(&Request, &mut dyn Write) + Send + Sync + 'static) -> Self {
        Self::start("http", None, handler)
    }

    /// Servidor HTTPS con el certificado de `tests/fixtures`.
    pub fn https(handler: impl Fn(&Request, &mut dyn Write) + Send + Sync + 'static) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        Self::start("https", Some(tls_config(&dir)), handler)
    }

    fn start(
        scheme: &'static str,
        tls: Option<Arc<rustls::ServerConfig>>,
        handler: impl Fn(&Request, &mut dyn Write) + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handler: Arc<Handler> = Arc::new(handler);
        thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = Arc::clone(&handler);
                        let tls = tls.clone();
                        thread::spawn(move || serve(stream, tls, &*handler));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { port, stop, scheme }
    }

    /// URL absoluta de `path` en este servidor.
    pub fn url(&self, path: &str) -> String {
        format!("{}://127.0.0.1:{}{path}", self.scheme, self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn tls_config(dir: &Path) -> Arc<rustls::ServerConfig> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};
    let certs: Vec<_> = CertificateDer::pem_file_iter(dir.join("tls_cert.pem"))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let key = PrivateKeyDer::from_pem_file(dir.join("tls_key.pem")).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    Arc::new(config)
}

fn read_request(stream: &mut dyn Read) -> Option<Request> {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => raw.push(byte[0]),
            _ => return None,
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let method = request_line.split(' ').next()?.to_owned();
    let path = request_line.split(' ').nth(1)?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

fn serve(stream: TcpStream, tls: Option<Arc<rustls::ServerConfig>>, handler: &Handler) {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    match tls {
        None => {
            let mut stream = stream;
            if let Some(request) = read_request(&mut stream) {
                handler(&request, &mut stream);
            }
            stream.flush().ok();
        }
        Some(config) => {
            let Ok(connection) = rustls::ServerConnection::new(config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(connection, stream);
            if let Some(request) = read_request(&mut tls) {
                handler(&request, &mut tls);
            }
            tls.conn.send_close_notify();
            tls.flush().ok();
        }
    }
}

/// Escribe una respuesta con `Connection: close`. `Content-Length` solo se añade si se
/// pide en `headers`.
pub fn respond(out: &mut dyn Write, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        204 => "No Content",
        302 => "Found",
        303 => "See Other",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        407 => "Proxy Authentication Required",
        504 => "Gateway Timeout",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes()).ok();
    out.write_all(body).ok();
    out.flush().ok();
}

/// Respuesta `200` con `Content-Length` correcto.
pub fn ok(out: &mut dyn Write, extra: &[(&str, &str)], body: &[u8]) {
    let length = body.len().to_string();
    let mut headers = vec![("Content-Length", length.as_str())];
    headers.extend_from_slice(extra);
    respond(out, 200, &headers, body);
}

/// Redirección con `Location`.
pub fn redirect(out: &mut dyn Write, status: u16, location: &str) {
    respond(
        out,
        status,
        &[("Location", location), ("Content-Length", "0")],
        b"",
    );
}
