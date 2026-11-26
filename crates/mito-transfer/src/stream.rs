//! Lado de red de un intento de descarga (`_Stream` del original).
//!
//! En Windows nada despierta de forma fiable una lectura de socket bloqueada desde otro
//! hilo, así que quien llama nunca lee: espera en un canal y puede salir en cuanto se
//! cancela. El hilo abandonado termina con el siguiente dato o al vencer el timeout, y
//! solo él toca y cierra la respuesta.

use std::error::Error as StdError;
use std::io::Read as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use mito_core::Message;
use mito_core::Operation;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use reqwest::redirect::Policy;

use crate::error::Error;

/// Tamaño del bloque de lectura (`read1(256 * 1024)`).
const BLOCK: usize = 256 * 1024;
/// Capacidad de la cola de eventos (`Queue(maxsize=16)`).
const QUEUE: usize = 16;
/// Intervalo de sondeo de la cancelación.
const POLL: Duration = Duration::from_millis(250);
/// Máximo de redirecciones (el `max_redirections` de `urllib`).
const MAX_REDIRECTS: usize = 10;

/// Redirección rechazada por la política (`WebRedirect` del original).
#[derive(Debug, thiserror::Error)]
pub(crate) enum RedirectRefused {
    /// El destino no es `http` ni `https`.
    #[error("La redirección debe usar HTTP o HTTPS")]
    Scheme,
    /// Se superó el máximo de redirecciones.
    #[error("demasiadas redirecciones")]
    TooMany(u16),
}

/// Política de redirecciones: solo `http`/`https` (también de HTTPS a HTTP) y un
/// máximo de [`MAX_REDIRECTS`].
pub(crate) fn redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        if !matches!(attempt.url().scheme(), "http" | "https") {
            return attempt.error(RedirectRefused::Scheme);
        }
        if attempt.previous().len() > MAX_REDIRECTS {
            let status = attempt.status().as_u16();
            return attempt.error(RedirectRefused::TooMany(status));
        }
        attempt.follow()
    })
}

/// Configuración del cliente de un intento.
#[derive(Debug, Clone)]
pub(crate) struct ClientConfig {
    /// No verificar el certificado TLS ni el nombre del host.
    pub insecure: bool,
    /// Timeout de conexión y de cada lectura.
    pub timeout: Duration,
    /// Proxy explícito; `Some("")` anula el del sistema; `None` usa el de `reqwest`.
    pub proxy: Option<String>,
    /// Lista `no_proxy` que acompaña a `proxy`.
    pub no_proxy: Option<String>,
}

fn build_client(config: &ClientConfig) -> Result<Client, Error> {
    let mut builder = Client::builder()
        .redirect(redirect_policy())
        .timeout(Some(config.timeout))
        .connect_timeout(config.timeout)
        .danger_accept_invalid_certs(config.insecure);
    match config.proxy.as_deref().map(str::trim) {
        None => {}
        Some("") => builder = builder.no_proxy(),
        Some(url) => {
            let mut proxy = reqwest::Proxy::all(url).map_err(|_| {
                Error::domain(Message::new(
                    "Configura una URL HTTP/HTTPS sin credenciales",
                ))
            })?;
            if let Some(list) = config.no_proxy.as_deref() {
                proxy = proxy.no_proxy(reqwest::NoProxy::from_string(list));
            }
            builder = builder.proxy(proxy);
        }
    }
    builder.build().map_err(|e| Error::Network {
        reason: describe(&e),
    })
}

/// Causa más profunda de un error, sin la URL (que puede llevar un token firmado).
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text = cause.to_string();
        source = cause.source();
    }
    text
}

/// Códigos HRESULT de SChannel/CryptoAPI que indican un fallo de verificación del
/// certificado (cadena no confiable, nombre, caducidad, revocación…).
const CERT_CODES: [u32; 11] = [
    0x800B_0101, // CERT_E_EXPIRED
    0x800B_0109, // CERT_E_UNTRUSTEDROOT
    0x800B_010A, // CERT_E_CHAINING
    0x800B_010C, // CERT_E_REVOKED
    0x800B_010D, // CERT_E_UNTRUSTEDTESTROOT
    0x800B_010F, // CERT_E_CN_NO_MATCH
    0x8009_0325, // SEC_E_UNTRUSTED_ROOT
    0x8009_0322, // SEC_E_WRONG_PRINCIPAL
    0x8009_030F, // SEC_E_MESSAGE_ALTERED (cadena inválida)
    0x8009_0349, // SEC_E_CERT_UNKNOWN
    0x8009_0328, // SEC_E_CERT_EXPIRED
];

/// ¿Falló la verificación del certificado? (`ssl.SSLCertVerificationError`)
fn is_certificate_failure(error: &reqwest::Error) -> bool {
    if !error.is_connect() {
        return false;
    }
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && let Some(code) = io.raw_os_error()
            // Los HRESULT llegan como i32 con el bit de signo.
            && CERT_CODES.contains(&code.cast_unsigned())
        {
            return true;
        }
        let text = cause.to_string().to_lowercase();
        if text.contains("certificate") || text.contains("certificado") {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Fallo de un intento, ya clasificado para decidir si se reintenta.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Verificación del certificado TLS (`URLError` con `SSLCertVerificationError`).
    Certificate,
    /// Redirección rechazada: no se reintenta.
    Redirect(RedirectRefused),
    /// Error antes de recibir la respuesta (`URLError`).
    Network(String),
    /// Error leyendo el cuerpo (`OSError`/`HTTPException`).
    Interrupted(String),
    /// Fallo local no reintentable (cliente imposible de construir…).
    Fatal(Error),
    /// El hilo de red terminó sin avisar.
    Lost,
}

fn classify(error: reqwest::Error) -> Failure {
    // La URL puede llevar un token firmado: nunca debe llegar a un mensaje.
    let error = error.without_url();
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(refused) = cause.downcast_ref::<RedirectRefused>() {
            return Failure::Redirect(match refused {
                RedirectRefused::Scheme => RedirectRefused::Scheme,
                RedirectRefused::TooMany(status) => RedirectRefused::TooMany(*status),
            });
        }
        source = cause.source();
    }
    if is_certificate_failure(&error) {
        return Failure::Certificate;
    }
    Failure::Network(describe(&error))
}

/// Respuesta recibida: estado, cabeceras y URL final.
#[derive(Debug)]
pub(crate) struct Head {
    /// Código de estado.
    pub status: u16,
    /// Cabeceras de la respuesta.
    pub headers: HeaderMap,
    /// URL tras las redirecciones.
    pub final_url: String,
}

/// Evento del hilo de red.
#[derive(Debug)]
pub(crate) enum Event {
    /// Cabeceras de la respuesta.
    Response(Head),
    /// Bloque de datos; vacío al terminar.
    Chunk(Vec<u8>),
    /// El intento falló.
    Failure(Failure),
}

/// Un intento de descarga en su propio hilo.
pub(crate) struct Stream {
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Stream {
    /// Lanza el hilo que hace la petición `GET` con `headers` y lee la respuesta.
    pub fn start(config: ClientConfig, url: String, headers: Vec<(&'static str, String)>) -> Self {
        let (sender, events) = sync_channel(QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        // Si no se puede lanzar el hilo, el canal se cierra y `next` informa de ello.
        let _ = thread::Builder::new()
            .name("mito-download".to_owned())
            .spawn(move || run(&config, &url, &headers, &sender, &flag));
        Self { events, stop }
    }

    /// Siguiente evento. Comprueba la cancelación cada 250 ms mientras espera, de modo
    /// que una lectura bloqueada no impide cancelar.
    ///
    /// # Errors
    ///
    /// [`mito_core::Error::Cancelled`] si se cancela la operación.
    pub fn next(&self, operation: &Operation) -> Result<Event, mito_core::Error> {
        loop {
            operation.check()?;
            match self.events.recv_timeout(POLL) {
                Ok(event) => return Ok(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Ok(Event::Failure(Failure::Lost));
                }
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Entrega `event` esperando a que haya hueco; `false` si hay que abandonar.
fn put(sender: &SyncSender<Event>, stop: &AtomicBool, mut event: Event) -> bool {
    while !stop.load(Ordering::SeqCst) {
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Full(back)) => {
                event = back;
                thread::sleep(Duration::from_millis(10));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    false
}

fn run(
    config: &ClientConfig,
    url: &str,
    headers: &[(&'static str, String)],
    sender: &SyncSender<Event>,
    stop: &AtomicBool,
) {
    let client = match build_client(config) {
        Ok(client) => client,
        Err(error) => {
            put(sender, stop, Event::Failure(Failure::Fatal(error)));
            return;
        }
    };
    let mut request = client.get(url);
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            request = request.header(name, value);
        }
    }
    request = request.header(USER_AGENT, "MITO/0.1");
    let mut response: Response = match request.send() {
        Ok(response) => response,
        Err(error) => {
            put(sender, stop, Event::Failure(classify(error)));
            return;
        }
    };
    let head = Head {
        status: response.status().as_u16(),
        headers: response.headers().clone(),
        final_url: response.url().to_string(),
    };
    if !put(sender, stop, Event::Response(head)) {
        return;
    }
    let mut buffer = vec![0_u8; BLOCK];
    while !stop.load(Ordering::SeqCst) {
        match response.read(&mut buffer) {
            Ok(0) => {
                put(sender, stop, Event::Chunk(Vec::new()));
                return;
            }
            Ok(read) => {
                if !put(sender, stop, Event::Chunk(buffer[..read].to_vec())) {
                    return;
                }
            }
            Err(error) => {
                let mut reason = error.to_string();
                let mut source = StdError::source(&error);
                while let Some(cause) = source {
                    reason = cause.to_string();
                    source = cause.source();
                }
                put(sender, stop, Event::Failure(Failure::Interrupted(reason)));
                return;
            }
        }
    }
}
