//! Descarga a disco con reintentos, reanudación y verificación de hash
//! (`download` de `transfer.py`).
//!
//! Contrato: `docs/spec/modules/transfer.md`. La red se lee en un hilo aparte
//! (módulo `stream`) para que [`Operation::cancel`] interrumpa también una lectura
//! bloqueada.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use mito_core::{Message, Operation};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderMap, LAST_MODIFIED};

use crate::error::{Error, Result};
use crate::hash::{ExpectedHash, Hasher, parse_hash};
use crate::stream::{ClientConfig, Event, Failure, RedirectRefused, Stream};
use crate::web::{self, content_range, is_http, public_url, validator, web_page, web_page_error};

/// Tamaño máximo de una descarga por defecto: 4 GiB.
pub const MAX_DOWNLOAD: u64 = 4 * 1024 * 1024 * 1024;

/// Bytes recibidos en un intento que renuevan el presupuesto de reintentos.
const RENEWAL: u64 = 1024 * 1024;
/// Bytes iniciales que se guardan para detectar páginas interceptadas.
const SNIFF: usize = 512;
/// Intervalo mínimo entre avisos de progreso.
const REPORT: Duration = Duration::from_millis(250);

/// Opciones de una descarga. Los valores por defecto son los del original; `mito-transfer`
/// no lee `config.properties` (ADR 0008, punto 10): el llamador las resuelve.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Intentos totales (mínimo 1). Por defecto 5.
    pub attempts: u32,
    /// Espera inicial entre intentos; se duplica tras cada reintento. Por defecto 2 s.
    pub delay: Duration,
    /// Bytes máximos de la descarga. Por defecto [`MAX_DOWNLOAD`].
    pub max_bytes: u64,
    /// Proxy explícito (`http://host:puerto`). `None` deja el comportamiento por
    /// defecto de `reqwest` (variables de entorno y proxy del sistema); `Some("")`
    /// desactiva cualquier proxy, como un `http_proxy=` vacío en `config.properties`.
    ///
    /// Pendiente para F9: resolver este valor desde `config.properties` y el proxy de
    /// Windows, y validarlo (`network.validate_url`).
    pub proxy: Option<String>,
    /// Lista `no_proxy` (hosts y sufijos separados por comas) que acompaña a `proxy`.
    pub no_proxy: Option<String>,
    /// Timeout de conexión y de cada lectura de red. Por defecto 60 s.
    pub timeout: Duration,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            attempts: 5,
            delay: Duration::from_secs(2),
            max_bytes: MAX_DOWNLOAD,
            proxy: None,
            no_proxy: None,
            timeout: Duration::from_secs(60),
        }
    }
}

impl DownloadOptions {
    /// Opciones de `versions.py` (`attempts=2, delay=1`).
    pub fn quick() -> Self {
        Self {
            attempts: 2,
            delay: Duration::from_secs(1),
            ..Self::default()
        }
    }
}

/// Resultado de un intento fallido, antes de decidir si se reintenta.
enum Attempt {
    Failed(Failure),
    Http(u16),
    Fatal(Error),
}

impl From<mito_core::Error> for Attempt {
    fn from(error: mito_core::Error) -> Self {
        Self::Fatal(error.into())
    }
}

/// Estado de la descarga que se conserva entre intentos para poder reanudar.
struct Download<'a> {
    address: &'a str,
    destination: &'a Path,
    options: &'a DownloadOptions,
    operation: &'a Operation,
    expected: Option<ExpectedHash>,
    attempts: i64,
    remaining: i64,
    pause: Duration,
    insecure: bool,
    received: u64,
    /// Bytes con los que empezó el intento actual (para renovar el presupuesto).
    before: u64,
    length: Option<u64>,
    validator: Option<String>,
    resumed: bool,
    hasher: Option<Hasher>,
    head: Vec<u8>,
    content_type: String,
    final_url: String,
}

/// Descarga `url` en `destination`.
///
/// Sigue redirecciones `http`/`https`, reintenta los errores transitorios con espera
/// exponencial y, tras una interrupción, reanuda con `Range` si el resultado sigue
/// siendo verificable (validador `If-Range` o hash declarado). `expected` es un hash
/// `algoritmo:digest`, un digest sin prefijo, `None`, vacío o `skip`.
///
/// `operation` recibe el progreso y los avisos, y puede cancelar en cualquier momento,
/// también durante una lectura bloqueada. Si la descarga falla o se cancela no queda
/// ningún archivo parcial en `destination`.
///
/// # Errors
///
/// - Dominio: esquema que no es HTTP/HTTPS, hash inválido, redirección a otro esquema
///   y descarga que supera `max_bytes`.
/// - [`Error::Http`]: respuesta 4xx permanente, o transitoria tras agotar los intentos.
/// - [`Error::Network`]: fallo de red tras agotar los intentos.
/// - [`Error::Integrity`]: hash incorrecto o página de portal en lugar del archivo.
/// - [`mito_core::Error::Cancelled`] (dentro de [`Error::Core`]) si se cancela.
pub fn download(
    url: &str,
    destination: &Path,
    expected: Option<&str>,
    options: &DownloadOptions,
    operation: &Operation,
) -> Result<()> {
    if !is_http(url) {
        return Err(Error::domain(Message::new(
            "Las descargas requieren HTTP o HTTPS",
        )));
    }
    let expected = parse_hash(expected)?;
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    }
    let address = url.split('#').next().unwrap_or(url);
    let attempts = i64::from(options.attempts.max(1));
    let mut download = Download {
        address,
        destination,
        options,
        operation,
        hasher: expected.as_ref().map(ExpectedHash::hasher),
        expected,
        attempts,
        remaining: attempts,
        pause: options.delay,
        insecure: false,
        received: 0,
        before: 0,
        length: None,
        validator: None,
        resumed: false,
        head: Vec::new(),
        content_type: String::new(),
        final_url: address.to_owned(),
    };
    let result = download.run();
    if result.is_err() {
        discard(destination);
    }
    result
}

fn discard(path: &Path) {
    // Un archivo ausente no es un problema; otro fallo no puede mejorar el error.
    let _ = fs::remove_file(path);
}

fn content_type_of(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_lowercase()
        })
        .filter(|v| v.contains('/'))
        .unwrap_or_else(|| "text/plain".to_owned())
}

fn text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

impl Download<'_> {
    fn has_hash(&self) -> bool {
        self.expected.is_some()
    }

    /// Aviso de la descarga (el original lo imprimía y lo enviaba a la operación).
    fn warn(&self, message: impl Into<Message>) {
        let message = message.into();
        tracing::debug!(operation = "download.notice", "{message}");
        self.operation.note(message);
    }

    fn stop_if_cancelled(&self) -> Result<()> {
        self.operation.check().map_err(Error::from)
    }

    /// Avisa, espera (cancelable) y duplica la espera.
    fn retry(&mut self, message: &str) -> Result<()> {
        let number = self.attempts - self.remaining + 1;
        self.warn(format!(
            "{message}; reintentando la descarga ({number}/{})",
            self.attempts
        ));
        self.operation.wait(self.pause)?;
        self.pause *= 2;
        Ok(())
    }

    fn restart(&mut self) {
        self.received = 0;
        self.length = None;
    }

    fn run(&mut self) -> Result<()> {
        let cert_notice = if self.has_hash() {
            "Aviso: certificado TLS no verificable; reintentando sin verificarlo (integridad garantizada por el hash declarado)"
        } else {
            "Aviso: certificado TLS no verificable; reintentando sin verificarlo ni comprobar la integridad (descarga sin hash)"
        };
        loop {
            self.remaining -= 1;
            self.stop_if_cancelled()?;
            let resumable = self.received > 0
                && (self.validator.is_some() || self.has_hash())
                && fs::metadata(self.destination)
                    .is_ok_and(|m| m.is_file() && m.len() == self.received);
            self.before = self.received;
            let outcome = self.attempt(resumable);
            match outcome {
                Ok(()) => return self.finish(),
                Err(Attempt::Fatal(error)) => return Err(error),
                Err(Attempt::Http(status)) => {
                    self.stop_if_cancelled()?;
                    if resumable && status == 416 && self.remaining > 0 {
                        // El parcial ya no corresponde al servidor: empezar de nuevo.
                        self.restart();
                        self.warn(
                            "Aviso: el servidor rechazó reanudar; la descarga empieza de nuevo",
                        );
                        continue;
                    }
                    // 5xx, 408 y 429 son transitorios; el resto de 4xx, permanentes.
                    if self.remaining > 0 && (status >= 500 || status == 408 || status == 429) {
                        self.retry(&format!("Aviso: HTTP {status}"))?;
                        continue;
                    }
                    return Err(self.http_error(status));
                }
                Err(Attempt::Failed(failure)) => {
                    self.stop_if_cancelled()?;
                    match failure {
                        Failure::Redirect(RedirectRefused::Scheme) => {
                            return Err(Error::domain(Message::new(
                                "La redirección debe usar HTTP o HTTPS",
                            )));
                        }
                        Failure::Redirect(RedirectRefused::TooMany(status)) => {
                            return Err(self.http_error(status));
                        }
                        Failure::Fatal(error) => return Err(error),
                        Failure::Certificate if !self.insecure => {
                            self.insecure = true;
                            self.warn(cert_notice);
                        }
                        Failure::Certificate => {
                            let reason = "certificado TLS no verificable".to_owned();
                            if self.remaining > 0 {
                                self.retry(&format!("Aviso: error de red ({reason})"))?;
                            } else {
                                return Err(Error::Network { reason });
                            }
                        }
                        Failure::Network(reason) => {
                            if self.remaining > 0 {
                                self.retry(&format!("Aviso: error de red ({reason})"))?;
                            } else {
                                return Err(Error::Network { reason });
                            }
                        }
                        Failure::Interrupted(reason) => self.interrupted(reason)?,
                        Failure::Lost => {
                            self.interrupted("el hilo de red terminó inesperadamente".to_owned())?;
                        }
                    }
                }
            }
        }
    }

    /// Corte de conexión, timeout o lectura incompleta: se reintenta conservando lo
    /// recibido. Si el intento avanzó al menos 1 MiB, vuelve el presupuesto completo.
    fn interrupted(&mut self, reason: String) -> Result<()> {
        if self.received.saturating_sub(self.before) >= RENEWAL {
            self.remaining = self.attempts;
            self.pause = self.options.delay;
        }
        if self.remaining > 0 {
            self.retry(&format!("Aviso: conexión interrumpida ({reason})"))
        } else {
            Err(Error::Network { reason })
        }
    }

    fn http_error(&self, status: u16) -> Error {
        Error::Http {
            status,
            reason: StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or_default()
                .to_owned(),
            url: public_url(self.address),
        }
    }

    /// Un intento: petición, cabeceras y cuerpo hasta el final.
    fn attempt(&mut self, resumable: bool) -> std::result::Result<(), Attempt> {
        let mut headers = Vec::new();
        if resumable {
            headers.push(("Range", format!("bytes={}-", self.received)));
            if let Some(validator) = &self.validator {
                headers.push(("If-Range", validator.clone()));
            }
        }
        let config = ClientConfig {
            insecure: self.insecure,
            timeout: self.options.timeout,
            proxy: self.options.proxy.clone(),
            no_proxy: self.options.no_proxy.clone(),
        };
        let stream = Stream::start(config, self.address.to_owned(), headers);
        let head = match stream.next(self.operation)? {
            Event::Response(head) => head,
            Event::Failure(failure) => return Err(Attempt::Failed(failure)),
            Event::Chunk(_) => return Err(Attempt::Failed(Failure::Lost)),
        };
        // `reqwest` no sigue una redirección a otro esquema y devuelve el 3xx tal cual.
        if (300..400).contains(&head.status)
            && text(&head.headers, reqwest::header::LOCATION).is_some_and(web::has_foreign_scheme)
        {
            return Err(Attempt::Fatal(Error::domain(Message::new(
                "La redirección debe usar HTTP o HTTPS",
            ))));
        }
        if !(200..300).contains(&head.status) {
            return Err(Attempt::Http(head.status));
        }
        let partial = head.status == 206;
        let span = partial
            .then(|| content_range(text(&head.headers, reqwest::header::CONTENT_RANGE)))
            .flatten();
        if partial && !(resumable && span.is_some_and(|(start, _)| start == self.received)) {
            // Un fragmento que no continúa el archivo: el siguiente intento, desde cero.
            self.restart();
            self.validator = None;
            return Err(Attempt::Failed(Failure::Interrupted(
                "respuesta parcial inesperada del servidor".to_owned(),
            )));
        }
        let append = if partial {
            if let Some((_, Some(total))) = span {
                self.length = Some(total);
            }
            self.resumed = true;
            self.warn(format!(
                "Aviso: reanudando la descarga desde {} bytes",
                self.received
            ));
            true
        } else {
            if resumable {
                self.warn("Aviso: el servidor no admite reanudar; la descarga empieza de nuevo");
            }
            self.received = 0;
            self.before = 0;
            self.resumed = false;
            self.hasher = self.expected.as_ref().map(ExpectedHash::hasher);
            self.head.clear();
            self.final_url.clone_from(&head.final_url);
            self.content_type = content_type_of(&head.headers);
            self.length = text(&head.headers, CONTENT_LENGTH)
                .map(str::trim)
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|v| v.parse().ok());
            self.validator = validator(
                text(&head.headers, ETAG),
                text(&head.headers, LAST_MODIFIED),
            );
            false
        };
        self.operation.transfer(self.received, self.length, "bytes");
        let mut output = self.open(append)?;
        self.copy_body(&stream, &mut output)?;
        drop(output);
        self.operation.transfer(self.received, self.length, "bytes");
        if let Some(length) = self.length
            && self.received < length
        {
            return Err(Attempt::Failed(Failure::Interrupted(format!(
                "IncompleteRead({} bytes read, {} more expected)",
                self.received,
                length - self.received
            ))));
        }
        Ok(())
    }

    fn open(&self, append: bool) -> std::result::Result<File, Attempt> {
        let mut options = OpenOptions::new();
        if append {
            options.append(true).create(true);
        } else {
            options.write(true).create(true).truncate(true);
        }
        options
            .open(self.destination)
            .map_err(|e| Attempt::Fatal(mito_core::Error::io(self.destination, e).into()))
    }

    /// Copia los bloques de la respuesta al archivo, calcula el hash y notifica el
    /// progreso (el primer bloque siempre, después cada 250 ms como máximo).
    fn copy_body(
        &mut self,
        stream: &Stream,
        output: &mut File,
    ) -> std::result::Result<(), Attempt> {
        let mut reported: Option<Instant> = None;
        loop {
            let chunk = match stream.next(self.operation)? {
                Event::Chunk(chunk) => chunk,
                Event::Failure(failure) => return Err(Attempt::Failed(failure)),
                Event::Response(_) => continue,
            };
            if chunk.is_empty() {
                return Ok(());
            }
            let size = chunk.len() as u64;
            if self.received.saturating_add(size) > self.options.max_bytes {
                let message = if self.options.max_bytes == MAX_DOWNLOAD {
                    Message::new("Descarga excede el límite de 4 GiB")
                } else {
                    // El original no traducía este texto; con `{p1}` la clave es estable.
                    Message::new("Descarga excede el límite de {p1} bytes")
                        .arg("p1", self.options.max_bytes)
                };
                return Err(Attempt::Fatal(Error::domain(message)));
            }
            output
                .write_all(&chunk)
                .map_err(|e| Attempt::Fatal(mito_core::Error::io(self.destination, e).into()))?;
            if self.head.len() < SNIFF {
                let take = (SNIFF - self.head.len()).min(chunk.len());
                self.head.extend_from_slice(&chunk[..take]);
            }
            if let Some(hasher) = &mut self.hasher {
                hasher.update(&chunk);
            }
            self.received += size;
            if reported.is_none_or(|at| at.elapsed() >= REPORT) {
                self.operation.transfer(self.received, self.length, "bytes");
                reported = Some(Instant::now());
            }
        }
    }

    /// Comprobaciones tras una descarga completa: página interceptada y hash.
    fn finish(&mut self) -> Result<()> {
        let total = self.received;
        // Antes del hash y también sin él: un artefacto sin hash nunca publica la página
        // de un portal como si fuera el componente.
        if web_page(self.address, &self.content_type, &self.head) {
            return Err(web_page_error(&self.final_url, self.address, Some(total)));
        }
        let (Some(hasher), Some(expected)) = (&self.hasher, &self.expected) else {
            return Ok(());
        };
        if hasher.hex_digest() == expected.digest {
            return Ok(());
        }
        if self.resumed && self.remaining > 0 {
            // Se unieron partes de un archivo que cambió entretanto: decide una descarga
            // completa dentro del mismo presupuesto.
            self.warn("Aviso: el hash no coincide tras reanudar; la descarga empieza de nuevo");
            self.restart();
            self.validator = None;
            self.resumed = false;
            discard(self.destination);
            return self.run();
        }
        let host = web::hostname(&self.final_url).unwrap_or_else(|| "?".to_owned());
        tracing::warn!(
            operation = "download.hash",
            host = %host,
            bytes = total,
            content_type = %self.content_type,
            algorithm = hasher.algorithm().name(),
            "Hash mismatch"
        );
        let url = public_url(self.address);
        Err(Error::Integrity {
            message: Box::new(
                Message::new(
                    "Hash incorrecto en {p1}: se recibieron {p2} bytes desde {p3}. Un proxy o antivirus puede estar alterando la descarga; prueba desde otra red o avisa a soporte con esta URL. Se ha descartado la descarga.",
                )
                .arg("p1", &url)
                .arg("p2", total)
                .arg("p3", &host),
            ),
            url,
            size: Some(total),
            host,
            portal: false,
        })
    }
}
