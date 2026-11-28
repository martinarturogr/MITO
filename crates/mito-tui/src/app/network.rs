//! Botón INTERNET (`ui.py`: `check_network`, `network_result`, `network_diagnosis`, sondeo)
//! y su flujo hasta `NetworkSetupDialog` y `ProxyDialog`.
//!
//! Las comprobaciones corren en hilos desechables; cada una lleva una generación y solo
//! vale la última (el `exclusive=True` del grupo `network` del original).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use mito_core::config::read_global;
use mito_core::diagnostics::{self, LogLevel};
use mito_engine::Error;
use mito_engine::network::{Diagnosis, DiagnosisKind, Network, apply, poll_interval};

use super::dialogs::Purpose;
use super::state::App;
use crate::dialogs::{Authenticator, NetworkSetupDialog, ProxyDialog};
use crate::model::describe;
use crate::widgets::{Notice, Severity, Variant};

/// Intervalo del sondeo si `network.poll_seconds` no es válido.
const DEFAULT_POLL: Duration = Duration::from_secs(20);
/// Duración de los avisos del diagnóstico sin cambios.
const LONG_NOTICE: Duration = Duration::from_secs(10);

/// Qué dice el botón INTERNET.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    /// `Comprobando Internet…`.
    Checking,
    /// `Internet accesible`.
    Online,
    /// `Internet no accesible`.
    NoInternet,
    /// `Sin conexión de red`.
    NoNetwork,
    /// `Diagnosticando la red…`.
    Diagnosing,
}

/// Resultado de un hilo de red.
pub(super) enum Event {
    Checked {
        generation: u64,
        prompt: bool,
        quiet: bool,
        result: Result<(bool, bool), Error>,
    },
    Diagnosed {
        generation: u64,
        result: Result<Diagnosis, Error>,
    },
}

/// Estado del botón INTERNET, del sondeo y de los hilos de red.
pub(super) struct NetState {
    pub status: Status,
    /// Cuántas veces cambió la etiqueta o la variante (el repintado del original).
    pub repaints: u32,
    /// Generación de la comprobación o diagnóstico vigente.
    pub generation: u64,
    /// La generación vigente aún no ha respondido.
    pub running: bool,
    pub tx: Sender<Event>,
    pub rx: Receiver<Event>,
    /// Intervalo del sondeo (`None`: desactivado).
    pub poll: Option<Duration>,
    pub next_poll: Option<Instant>,
    /// Último error del sondeo silencioso ya registrado (F3: no repetirlo).
    pub last_quiet_error: Option<String>,
}

impl NetState {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            status: Status::Checking,
            repaints: 0,
            generation: 0,
            running: false,
            tx,
            rx,
            poll: None,
            next_poll: None,
            last_quiet_error: None,
        }
    }
}

impl App {
    /// Etiqueta y variante del botón INTERNET.
    pub(super) fn network_button(&self) -> (String, Variant) {
        let (key, variant) = match self.net.status {
            Status::Checking => ("Comprobando Internet…", Variant::Default),
            Status::Online => ("Internet accesible", Variant::Default),
            Status::NoInternet => ("Internet no accesible", Variant::Error),
            Status::NoNetwork => ("Sin conexión de red", Variant::Error),
            Status::Diagnosing => ("Diagnosticando la red…", Variant::Error),
        };
        (self.text(key), variant)
    }

    fn set_status(&mut self, status: Status) {
        if self.net.status != status {
            self.net.status = status;
            self.net.repaints += 1;
        }
    }

    /// Arranque: comprobación inicial y sondeo (`start_network_poll`). Un
    /// `config.properties` ilegible deja los 20 s por defecto.
    pub(super) fn mount_network(&mut self) {
        self.net.poll = match read_global(self.engine.layout()) {
            Ok(values) => poll_interval(&values),
            Err(_) => Some(DEFAULT_POLL),
        };
        self.net.next_poll = self.net.poll.map(|every| self.now + every);
        self.check_network();
    }

    /// `check_network()`: comprobación con registro, sin diálogos.
    pub(super) fn check_network(&mut self) {
        self.start_check(false, false);
    }

    /// Pulsación del botón INTERNET (`check_network(prompt=True)`).
    pub(super) fn press_network(&mut self) {
        self.start_check(true, false);
    }

    fn start_check(&mut self, prompt: bool, quiet: bool) {
        self.net.generation += 1;
        self.net.running = true;
        let generation = self.net.generation;
        let integration = Arc::clone(&self.integration);
        let layout = self.engine.layout().clone();
        let tx = self.net.tx.clone();
        std::thread::spawn(move || {
            let result = (integration.network.check)(&layout, !quiet);
            // Sin receptor: la interfaz ya terminó.
            let _ = tx.send(Event::Checked {
                generation,
                prompt,
                quiet,
                result,
            });
        });
    }

    fn start_diagnose(&mut self) {
        self.net.generation += 1;
        self.net.running = true;
        let generation = self.net.generation;
        let integration = Arc::clone(&self.integration);
        let layout = self.engine.layout().clone();
        let tx = self.net.tx.clone();
        std::thread::spawn(move || {
            let result = (integration.network.diagnose)(&layout);
            let _ = tx.send(Event::Diagnosed { generation, result });
        });
    }

    /// Procesa los resultados de red ya disponibles.
    pub(super) fn poll_network_events(&mut self) {
        while let Ok(event) = self.net.rx.try_recv() {
            match event {
                Event::Checked {
                    generation,
                    prompt,
                    quiet,
                    result,
                } if generation == self.net.generation => {
                    self.net.running = false;
                    self.checked(prompt, quiet, result);
                }
                Event::Diagnosed { generation, result } if generation == self.net.generation => {
                    self.net.running = false;
                    match result {
                        Ok(diagnosis) => self.network_diagnosis(Some(diagnosis)),
                        Err(error) => {
                            self.capture(&error, "network.diagnose");
                            self.network_diagnosis(None);
                        }
                    }
                }
                // Una comprobación superada por otra más reciente.
                _ => {}
            }
        }
    }

    fn checked(&mut self, prompt: bool, quiet: bool, result: Result<(bool, bool), Error>) {
        match result {
            Ok((online, local)) => {
                self.net.last_quiet_error = None;
                self.network_result(Some((online, local)), prompt);
            }
            Err(error) => {
                // F3: el sondeo silencioso no repite el mismo error en cada intervalo.
                let text = error.to_string();
                if !quiet || self.net.last_quiet_error.as_deref() != Some(text.as_str()) {
                    self.capture(&error, "network.check");
                }
                self.net.last_quiet_error = quiet.then_some(text);
                self.network_result(None, false);
            }
        }
    }

    /// `network_result`: pinta el botón y decide si diagnosticar.
    /// `result` es `None` si la comprobación falló (equivale a `(False, True)` sin
    /// `Network`, así que nunca se diagnostica).
    fn network_result(&mut self, result: Option<(bool, bool)>, prompt: bool) {
        let have_network = result.is_some();
        let (online, local) = result.unwrap_or((false, true));
        let status = if online {
            Status::Online
        } else if local {
            Status::NoInternet
        } else {
            Status::NoNetwork
        };
        self.set_status(status);
        if prompt && !online && local && have_network {
            self.set_status(Status::Diagnosing);
            self.start_diagnose();
        } else if prompt && !online {
            let text = self.text("Conecta el equipo a una red");
            self.notify(text, Severity::Warning);
        }
    }

    /// Cada tic: el sondeo silencioso. No se solapa con una comprobación o diagnóstico
    /// en curso ni interrumpe un diálogo abierto.
    pub(super) fn network_tick(&mut self, now: Instant) {
        let (Some(every), Some(due)) = (self.net.poll, self.net.next_poll) else {
            return;
        };
        if now < due {
            return;
        }
        self.net.next_poll = Some(now + every);
        if self.dialogs.is_empty() && !self.net.running {
            self.start_check(false, true);
        }
    }

    /// `network_diagnosis`: qué pantalla o aviso sigue al diagnóstico.
    fn network_diagnosis(&mut self, diagnosis: Option<Diagnosis>) {
        let Some(diagnosis) = diagnosis else {
            self.check_network();
            return;
        };
        if diagnosis.kind == DiagnosisKind::Ok && diagnosis.changes.is_empty() {
            self.check_network();
            return;
        }
        if diagnosis.kind == DiagnosisKind::Offline {
            self.check_network();
            let text = self.text("Conecta el equipo a una red");
            self.notify(text, Severity::Warning);
            return;
        }
        if !diagnosis.changes.is_empty() {
            let dialog = NetworkSetupDialog::new(&diagnosis);
            self.push_dialog(dialog, Purpose::NetworkSetup(diagnosis));
        } else if diagnosis.kind == DiagnosisKind::Portal {
            self.network_login();
        } else {
            self.check_network();
            self.diagnosis_notice(&diagnosis);
        }
    }

    fn diagnosis_notice(&mut self, diagnosis: &Diagnosis) {
        let text = match diagnosis.kind {
            DiagnosisKind::Auth => {
                let schemes = if diagnosis.authenticate.is_empty() {
                    "Basic"
                } else {
                    diagnosis.authenticate.as_str()
                };
                self.tr.tr(&mito_core::Message::new(
                    "El proxy {p0} exige autenticación HTTP 407 ({p1}), que MITO no admite.",
                )
                .arg("p0", &diagnosis.proxy)
                .arg("p1", schemes))
            }
            DiagnosisKind::Blocked => self.tr.tr(&mito_core::Message::new(
                "La comprobación de Internet respondió HTTP {p0}.",
            )
            .arg("p0", diagnosis.status)),
            _ => self
                .text("No se pudo llegar a Internet ni con el proxy de Windows ni directamente."),
        };
        self.notices
            .push(Notice::with_timeout(text, Severity::Warning, LONG_NOTICE));
    }

    /// `saved(accepted)` de `NetworkSetupDialog`: guarda las claves y sigue con el login.
    pub(super) fn network_saved(&mut self, accepted: bool, diagnosis: &Diagnosis) {
        if !accepted {
            self.check_network();
            return;
        }
        if let Err(error) = apply(self.engine.layout(), &diagnosis.changes) {
            let text = describe(&error, &self.tr);
            self.notify(text, Severity::Error);
            self.check_network();
            return;
        }
        let text = self.text("Configuración de red guardada en config.properties");
        self.write_log(text, false);
        if diagnosis.kind == DiagnosisKind::Portal {
            self.network_login();
        } else {
            self.check_network();
        }
    }

    /// `login()`: `ProxyDialog` solo si el portal está activado. Cualquier error de
    /// configuración (F2) se avisa.
    fn network_login(&mut self) {
        match Network::open(self.engine.layout(), None) {
            Err(error) => {
                let text = describe(&error, &self.tr);
                self.notify(text, Severity::Error);
                self.check_network();
            }
            Ok(network) if network.enabled() => {
                let dialog = ProxyDialog::new(self.authenticator());
                self.push_dialog(dialog, Purpose::Proxy);
            }
            Ok(_) => self.check_network(),
        }
    }

    /// Autenticación para el hilo del diálogo: los errores de configuración (F1) vuelven
    /// como texto y se registran sin credenciales.
    fn authenticator(&self) -> Authenticator {
        let integration = Arc::clone(&self.integration);
        let layout = self.engine.layout().clone();
        let tr = self.tr.clone();
        Arc::new(move |credentials| {
            (integration.network.authenticate)(&layout, &credentials).map_err(|error| {
                diagnostics::write(
                    &layout,
                    LogLevel::Warning,
                    "network.authenticate",
                    &diagnostics::message(&layout, &error),
                    &[],
                );
                describe(&error, &tr)
            })
        })
    }
}
