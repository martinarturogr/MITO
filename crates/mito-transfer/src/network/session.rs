//! Cliente HTTP sin redirecciones (`NoRedirect` + `opener` de `network.py`).

use std::error::Error as StdError;
use std::fmt::Write as _;
use std::io::Read as _;
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_TYPE, LOCATION, PROXY_AUTHENTICATE, USER_AGENT};
use reqwest::redirect::Policy;

use super::proxies::Proxies;

/// Bytes máximos que se leen del cuerpo (`page.read(1024 * 1024)`).
pub const MAX_BODY: usize = 1024 * 1024;

/// Opciones de una [`Session`].
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// Proxies de las peticiones (`opener`).
    pub proxies: Proxies,
    /// Timeout de conexión y de cada lectura.
    pub timeout: Duration,
    /// Guardar las cookies de las respuestas (`HTTPCookieProcessor`). Viven solo mientras
    /// vive la sesión.
    pub cookies: bool,
}

/// Respuesta, sea cual sea su estado: las redirecciones no se siguen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Código de estado.
    pub status: u16,
    /// Cabecera `Location`, si la hay.
    pub location: Option<String>,
    /// Todas las cabeceras `Proxy-Authenticate`.
    pub proxy_authenticate: Vec<String>,
    /// Los primeros [`MAX_BODY`] bytes del cuerpo.
    pub body: Vec<u8>,
}

impl Reply {
    /// ¿Estado 2xx? `urllib` solo devuelve respuestas así; el resto es un `HTTPError`.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Fallo de transporte (`URLError` u `OSError` del original).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Clase del error (`URLError`, `TimeoutError`, `OSError`).
    pub error: String,
    /// Texto de la causa más profunda, sin la URL (que puede llevar un token).
    pub message: String,
    /// Estado de la respuesta del proxy al `CONNECT` fallido. `reqwest` solo distingue
    /// `407`; cualquier otro rechazo se informa como `502` porque no expone su estado.
    pub tunnel_status: Option<u16>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Cliente sin redirecciones con cookies opcionales.
#[derive(Debug)]
pub struct Session {
    client: Client,
}

impl Session {
    /// Construye el cliente. Nunca sigue redirecciones: un portal no se acepta como si
    /// fuera Internet ni se reenvía un POST con credenciales.
    ///
    /// # Errors
    ///
    /// [`Failure`] si `reqwest` no puede construir el cliente.
    pub fn new(options: &SessionOptions) -> Result<Self, Failure> {
        let proxies = options.proxies.clone();
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Some(options.timeout))
            .connect_timeout(options.timeout)
            .cookie_store(options.cookies)
            .no_proxy()
            .proxy(reqwest::Proxy::custom(move |url| proxies.proxy_for(url)))
            .build()
            .map_err(failure)?;
        Ok(Self { client })
    }

    /// `GET`.
    ///
    /// # Errors
    ///
    /// [`Failure`] si no hay respuesta (conexión, timeout, TLS, túnel del proxy).
    pub fn get(&self, url: &str) -> Result<Reply, Failure> {
        send(self.client.get(url))
    }

    /// `POST` de un formulario `application/x-www-form-urlencoded`.
    ///
    /// # Errors
    ///
    /// Los de [`Session::get`].
    pub fn post_form(&self, url: &str, fields: &[(String, String)]) -> Result<Reply, Failure> {
        let request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(urlencode(fields));
        send(request)
    }
}

fn send(request: reqwest::blocking::RequestBuilder) -> Result<Reply, Failure> {
    let response = request
        .header(USER_AGENT, "MITO/0.1")
        .send()
        .map_err(failure)?;
    reply(response)
}

/// `urllib.parse.urlencode`: `quote_plus` de cada par (`_.-~` y alfanuméricos sin escapar,
/// espacio como `+`).
fn urlencode(fields: &[(String, String)]) -> String {
    fn quote(text: &str, out: &mut String) {
        for byte in text.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                    out.push(char::from(byte));
                }
                b' ' => out.push('+'),
                other => {
                    // Escribir en un `String` no puede fallar.
                    let _ = write!(out, "%{other:02X}");
                }
            }
        }
    }
    let mut out = String::new();
    for (index, (name, value)) in fields.iter().enumerate() {
        if index > 0 {
            out.push('&');
        }
        quote(name, &mut out);
        out.push('=');
        quote(value, &mut out);
    }
    out
}

fn reply(response: Response) -> Result<Reply, Failure> {
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let proxy_authenticate = response
        .headers()
        .get_all(PROXY_AUTHENTICATE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect();
    let mut body = Vec::new();
    response
        .take(MAX_BODY as u64)
        .read_to_end(&mut body)
        .map_err(|error| Failure {
            error: "OSError".to_owned(),
            message: error.to_string(),
            tunnel_status: None,
        })?;
    Ok(Reply {
        status,
        location,
        proxy_authenticate,
        body,
    })
}

/// Clasifica un error de `reqwest` sin conservar la URL.
fn failure(error: reqwest::Error) -> Failure {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut tunnel_status = None;
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if text.contains("proxy authorization required") {
            tunnel_status = Some(407);
        } else if text.contains("tunnel error: unsuccessful") {
            tunnel_status = Some(502);
        }
        message = text;
        source = cause.source();
    }
    let class = if error.is_timeout() {
        "TimeoutError"
    } else if error.is_connect() || error.is_request() {
        "URLError"
    } else {
        "OSError"
    };
    Failure {
        error: class.to_owned(),
        message,
        tunnel_status,
    }
}

#[cfg(test)]
mod tests {
    use super::urlencode;

    #[test]
    fn la_codificacion_es_la_de_urlencode() {
        let fields = [
            ("csrf".to_owned(), "tok&en".to_owned()),
            ("user".to_owned(), "ana maría".to_owned()),
            ("safe".to_owned(), "a_b.c-d~".to_owned()),
            ("star".to_owned(), "*/=".to_owned()),
        ];
        assert_eq!(
            urlencode(&fields),
            "csrf=tok%26en&user=ana+mar%C3%ADa&safe=a_b.c-d~&star=%2A%2F%3D"
        );
        assert_eq!(urlencode(&[]), "");
    }
}
