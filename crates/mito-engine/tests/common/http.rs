//! Servidor HTTP local mínimo (una petición por conexión).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Servidor que se detiene al soltarlo.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// URL absoluta de `path`.
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{path}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn start(status: u16, body: Vec<u8>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            let body = body.clone();
            std::thread::spawn(move || {
                stream.set_nonblocking(false).unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buffer[..n]),
                    }
                }
                let head = format!(
                    "HTTP/1.0 {status} X\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    Server { port, stop }
}

/// Responde `200` con `body` a cualquier ruta.
pub fn serve(body: Vec<u8>) -> Server {
    start(200, body)
}

/// Responde siempre con el código `status`.
pub fn serve_status(status: u16) -> Server {
    start(status, Vec::new())
}
