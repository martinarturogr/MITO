//! Operaciones en el hilo de trabajo: inicio, informes, temporizadores y final.

use std::sync::Arc;

use std::time::{Duration, Instant};

use mito_core::Message;

use super::state::{App, DELAY, Page, Timer, TimerKind};
use crate::dialogs::{LongStepDialog, OperationDialog};
use crate::model::describe;
use crate::widgets::Severity;
use crate::worker::{self, Action, Failure, Report};

impl App {
    /// Arranque (`on_mount`): profile inicial, ajustes de la terminal, Inicio,
    /// identidad de la barra de tareas y comprobación de actualizaciones.
    pub fn mount(&mut self, now: Instant) {
        self.now = now;
        let wanted = self.initial_profile.clone();
        self.rebuild_profiles(wanted.as_deref());
        self.refresh_terminal_default();
        self.refresh_home();
        if self.identity {
            mito_terminal::identity::apply_safely(self.engine.layout());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = worker::spawn_translated(
            Arc::clone(&self.engine),
            Action::StartupUpdates,
            self.current_profile(),
            tx,
            self.tr.clone(),
        );
        self.startup = Some((handle, rx));
    }

    /// Pide una acción: las que necesitan profile avisan «Selecciona un profile» si no
    /// hay (guarda de profile del original).
    pub(super) fn request(&mut self, action: Action) {
        let needs_profile = matches!(
            action,
            Action::Toggle { .. } | Action::DeleteProfile | Action::Launch { .. }
        );
        if needs_profile && self.current_profile().is_none() {
            let text = self.text("Selecciona un profile");
            self.notify(text, Severity::Warning);
            return;
        }
        self.begin(action);
    }

    /// `begin`: marca ocupado, programa el indicador y lanza el hilo. Si la
    /// comprobación del arranque sigue en curso, la acción espera a que termine.
    pub(super) fn begin(&mut self, action: Action) {
        self.start_busy_indicator();
        self.busy.reload_catalog = matches!(
            action,
            Action::Refresh | Action::Install(_) | Action::StartupUpdates | Action::CheckUpdates
        );
        if self.startup.is_some() {
            self.queued = Some(action);
            return;
        }
        self.spawn(action);
    }

    fn spawn(&mut self, action: Action) {
        let profile = self.current_profile();
        self.worker = Some(worker::spawn_translated(
            Arc::clone(&self.engine),
            action,
            profile,
            self.reports_tx.clone(),
            self.tr.clone(),
        ));
    }

    /// `start_busy_indicator`: una secuencia nueva invalida los temporizadores viejos.
    pub(super) fn start_busy_indicator(&mut self) {
        self.busy.active = true;
        self.busy.sequence += 1;
        self.busy.indicator = false;
        let kind = TimerKind::Busy(self.busy.sequence);
        self.schedule(DELAY, kind);
    }

    /// `stop_busy_indicator`.
    fn stop_busy_indicator(&mut self) {
        self.busy.active = false;
        self.busy.sequence += 1;
        self.busy.indicator = false;
    }

    pub(super) fn schedule(&mut self, delay: Duration, kind: TimerKind) {
        self.timers.push(Timer {
            due: self.now + delay,
            kind,
        });
    }

    /// Procesa los informes que el hilo ya ha enviado, sin esperar.
    pub fn poll_reports(&mut self, now: Instant) {
        // Vacío o desconectado: no hay más informes que procesar.
        while let Ok(report) = self.reports_rx.try_recv() {
            self.on_report(report, now);
        }
        while let Some(report) = self.startup.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) {
            self.on_startup_report(report, now);
        }
    }

    /// Un informe de la comprobación del arranque: solo registra y, al terminar,
    /// refresca Inicio y el catálogo si no hay otra operación (`component_updates_done`).
    pub fn on_startup_report(&mut self, report: Report, now: Instant) {
        self.now = now;
        match report {
            Report::Log { message, error, .. } => {
                let text = self.tr.tr(&message);
                self.log.push(text, error);
            }
            // No pide confirmaciones; si llegara alguna, se rechaza.
            Report::Confirm { answer, .. } => {
                let _ = answer.send(false);
            }
            Report::Pick { answer, .. } => {
                let _ = answer.send(crate::worker::Pick::Cancel);
            }
            Report::Started(_) => {}
            Report::Done { .. } => {
                if let Some((handle, _)) = self.startup.take() {
                    let _ = handle.join();
                }
                self.catalog.data = None;
                if !self.busy.active {
                    self.refresh_home();
                    if self.page == Page::Catalog {
                        self.load_catalog();
                    }
                }
                if let Some(action) = self.queued.take() {
                    self.spawn(action);
                }
            }
        }
    }

    /// Un informe del hilo de trabajo.
    pub fn on_report(&mut self, report: Report, now: Instant) {
        self.now = now;
        match report {
            Report::Log {
                message,
                error,
                long_step,
            } => {
                // El hilo ya lo escribió en `logs/mito.log`: aquí solo va al panel.
                let text = self.tr.tr(&message);
                self.log.push(text, error);
                if let Some(active) = long_step {
                    self.long_step(message, active);
                }
            }
            Report::Confirm { request, answer } => self.open_confirmation(request, answer),
            Report::Pick { answer, .. } => {
                let _ = answer.send(crate::worker::Pick::Cancel);
            }
            Report::Started(operation) => {
                self.busy.operation = Some(operation);
                self.busy.operation_sequence += 1;
                let kind = TimerKind::Progress(self.busy.operation_sequence);
                self.schedule(DELAY, kind);
            }
            Report::Done { profile, failure } => self.operation_done(profile.as_deref(), failure),
        }
    }

    /// `long_step(line)`: el aviso «PREPARANDO ARCHIVOS» a los 2 s de empezar un paso
    /// sin progreso medible.
    fn long_step(&mut self, message: Message, active: bool) {
        let measurable = self
            .busy
            .operation
            .as_ref()
            .is_some_and(|operation| operation.state().active);
        if measurable {
            return;
        }
        self.busy.long_sequence += 1;
        if active {
            let kind = TimerKind::LongStep(self.busy.long_sequence, message);
            self.schedule(DELAY, kind);
        } else {
            self.finish_long_step();
        }
    }

    /// `operation_done`: rehabilita, recarga profiles, Inicio y, si procede, el catálogo.
    fn operation_done(&mut self, profile: Option<&str>, failure: Option<Failure>) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.busy.operation = None;
        // Invalida los temporizadores pendientes de esta operación.
        self.busy.operation_sequence += 1;
        self.busy.long_sequence += 1;
        self.close_operation_dialogs();
        self.stop_busy_indicator();
        self.rebuild_profiles(profile);
        self.refresh_home();
        if self.busy.reload_catalog {
            self.catalog.data = None;
            self.busy.reload_catalog = false;
        }
        if self.page == Page::Catalog {
            self.load_catalog();
        }
        if let Some(info) = failure.and_then(|failure| failure.integrity) {
            self.open_integrity(info);
        }
    }

    /// Tic periódico (cada 250 ms): avisos caducados, temporizadores y relojes de los
    /// diálogos.
    pub fn tick(&mut self, now: Instant) {
        self.now = now;
        self.notices.retain(|notice| !notice.expired(now));
        let (due, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timers)
            .into_iter()
            .partition(|timer| timer.due <= now);
        self.timers = pending;
        for timer in due {
            self.fire(timer.kind);
        }
        self.tick_dialogs();
    }

    fn fire(&mut self, kind: TimerKind) {
        match kind {
            TimerKind::Busy(sequence) => {
                if self.busy.active && sequence == self.busy.sequence {
                    self.busy.indicator = true;
                }
            }
            TimerKind::Progress(sequence) => {
                let operation = self.busy.operation.clone();
                if let Some(operation) = operation
                    && sequence == self.busy.operation_sequence
                    && operation.state().active
                    && !self.has_progress()
                {
                    self.open_progress(OperationDialog::with_start(operation, self.now));
                }
            }
            TimerKind::LongStep(sequence, message) => {
                if sequence == self.busy.long_sequence
                    && !self.has_long_step()
                    && !self.has_progress()
                {
                    self.open_long_step(LongStepDialog::with_start(message, self.now));
                }
            }
        }
    }

    /// `action_quit` (Ctrl+Q): no se sale durante una operación.
    pub(super) fn action_quit(&mut self) {
        if self.busy.active {
            let text = self.text("Espera a que termine la operación antes de salir");
            self.notify(text, Severity::Info);
        } else {
            self.quit = true;
        }
    }

    /// Texto de un error del motor para el usuario.
    pub(super) fn describe_error(&self, error: &mito_engine::Error) -> String {
        describe(error, &self.tr)
    }
}
