//! Diálogo de progreso con cancelación cooperativa (`OperationDialog`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use mito_core::operation::OperationState;
use mito_core::{Message, Operation};
use mito_i18n::Translator;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::layout::Content;
use super::{Context, Dialog, Outcome, Step, pressed};
use crate::input::Input;
use crate::theme;
use crate::widgets::modal::{self, Size};
use crate::widgets::{Button, Focus, Ui, Variant};

const CANCEL: &str = "operation-cancel";

/// Ventana de muestras para calcular la velocidad.
const SAMPLE_WINDOW: Duration = Duration::from_secs(5);

/// Intervalo mínimo entre la primera y la última muestra para mostrar velocidad.
const MIN_SPEED_INTERVAL: Duration = Duration::from_secs(1);

/// Formato de tamaños: `0 B`, `5.0 MB`. Divide entre 1024 en cada paso y se detiene en
/// GB, como `human_size` del original.
pub fn human_size(value: u64) -> String {
    // Los tamaños que importan caben de sobra en la mantisa de un `f64`.
    #[allow(clippy::cast_precision_loss, reason = "solo se muestra con un decimal")]
    let size = value as f64;
    format_size(size)
}

fn format_size(mut size: f64) -> String {
    for unit in ["B", "KB", "MB"] {
        if size < 1024.0 {
            return if unit == "B" {
                format!("{size:.0} B")
            } else {
                format!("{size:.1} {unit}")
            };
        }
        size /= 1024.0;
    }
    format!("{size:.1} GB")
}

/// Progreso de la operación en curso. Lee [`Operation::state`] en cada tic; no tiene
/// `Esc`. «Cancelar instalación» llama a [`Operation::cancel`] mientras la fase sea
/// cancelable, y el diálogo se cierra cuando la operación deja de estar activa.
#[derive(Debug)]
pub struct OperationDialog {
    focus: Focus,
    operation: Operation,
    started: Instant,
    phase: Option<Instant>,
    samples: VecDeque<(Instant, u64)>,
    finished: bool,
}

impl OperationDialog {
    /// Diálogo para `operation`; el reloj empieza ahora.
    pub fn new(operation: Operation) -> Self {
        Self::with_start(operation, Instant::now())
    }

    /// Como [`new`](Self::new) con el instante en que se abrió el diálogo.
    pub fn with_start(operation: Operation, started: Instant) -> Self {
        Self {
            focus: Focus::starting_at(CANCEL),
            operation,
            started,
            phase: None,
            samples: VecDeque::new(),
            finished: false,
        }
    }

    /// Cierra el diálogo en el siguiente tic.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// Anota una muestra de progreso en `now`. Una fase nueva o un contador que baja
    /// reinician las muestras.
    fn sample_at(&mut self, now: Instant, state: &OperationState) {
        if state.started != self.phase {
            self.phase = state.started;
            self.samples.clear();
        }
        let Some(received) = state.received else {
            return;
        };
        if self
            .samples
            .back()
            .is_some_and(|(_, last)| received < *last)
        {
            self.samples.clear();
        }
        self.samples.push_back((now, received));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > SAMPLE_WINDOW)
        {
            self.samples.pop_front();
        }
    }

    /// Texto de progreso de la fase (`detail` del original).
    fn detail(&self, state: &OperationState, tr: &Translator) -> String {
        let Some(received) = state.received else {
            return String::new();
        };
        let total = state.total.filter(|total| *total > 0);
        if state.unit == "files" {
            let total = total.map_or_else(|| "?".to_owned(), |total| total.to_string());
            return tr.tr(&Message::new("{p1}/{p2} archivos")
                .arg("p1", received)
                .arg("p2", total));
        }
        let mut text = match total {
            Some(total) => tr.tr(&Message::new("{p1} de {p2}")
                .arg("p1", human_size(received))
                .arg("p2", human_size(total))),
            None => tr.tr(&Message::new("{p1} recibidos").arg("p1", human_size(received))),
        };
        if let (Some((first_at, first)), Some((last_at, last))) =
            (self.samples.front(), self.samples.back())
        {
            let interval = last_at.duration_since(*first_at);
            if interval >= MIN_SPEED_INTERVAL {
                // Los bytes por segundo solo se muestran con un decimal.
                #[allow(clippy::cast_precision_loss, reason = "solo se muestra con un decimal")]
                let speed = (last - first) as f64 / interval.as_secs_f64();
                text = format!("{text} · {}/s", format_size(speed));
            }
        }
        text
    }

    /// Barra de progreso de `width` celdas: llena según `received`/`total` o, sin
    /// total, un bloque que va y viene.
    fn bar(state: &OperationState, width: usize, elapsed: Duration) -> String {
        let inner = width.max(8) - 2;
        if let Some(total) = state.total.filter(|total| *total > 0) {
            let received = state.received.unwrap_or(0).min(total);
            let filled = usize::try_from(
                u128::from(received) * u128::try_from(inner).unwrap_or(0) / u128::from(total),
            )
            .unwrap_or(inner);
            return format!("[{}{}]", "█".repeat(filled), "░".repeat(inner - filled));
        }
        let block = (inner / 4).max(2);
        let span = inner - block;
        let step = usize::try_from(elapsed.as_millis() / 250).unwrap_or(0) % (2 * span.max(1));
        let at = if step <= span { step } else { 2 * span - step };
        format!(
            "[{}{}{}]",
            "░".repeat(at),
            "█".repeat(block),
            "░".repeat(span - at)
        )
    }
}

impl Dialog for OperationDialog {
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui) {
        let state = self.operation.state();
        let tr = ui.tr;
        let title = if state.title.is_empty() {
            tr.text("INSTALANDO").to_owned()
        } else {
            tr.tr(&state.title)
        };
        let elapsed = tr.tr(&Message::new("Tiempo transcurrido: {p1} s")
            .arg("p1", self.started.elapsed().as_secs()));
        let detail = self.detail(&state, tr);
        let mut content = Content::new();
        content.text(title, theme::title());
        if !state.text.is_empty() {
            content.gap(1).text(tr.tr(&state.text), theme::panel());
        }
        content.gap(1).slot("bar", 1);
        if !detail.is_empty() {
            content.text(detail, theme::panel());
        }
        if !state.note.is_empty() {
            content.gap(1).text(tr.tr(&state.note), theme::dim());
        }
        content
            .gap(1)
            .text(elapsed, theme::dim())
            .gap(1)
            .slot("cancel", 1);
        let width = Size::Cells(76).resolve(screen.width);
        let height = content.height(width.saturating_sub(4)).saturating_add(2);
        let area = modal::area(screen, Size::Cells(76), Size::Auto(height));
        let inner = modal::frame(frame, area, theme::BORDER_WARNING);
        let slots = content.render(frame, inner);
        let bar = slots.get("bar");
        let line = Self::bar(&state, usize::from(bar.width), self.started.elapsed());
        frame.render_widget(
            Paragraph::new(Span::styled(
                line,
                Style::new().fg(theme::BUSY).bg(theme::SIDEBAR),
            )),
            bar,
        );
        let cancelling = self.operation.is_cancelled();
        let label = if cancelling {
            tr.text("Cancelando…")
        } else {
            tr.text("Cancelar instalación")
        };
        Button::new(CANCEL, label)
            .variant(Variant::Error)
            .disabled(cancelling || !state.cancellable)
            .render(frame, slots.get("cancel"), ui);
    }

    fn handle(&mut self, input: &Input, _context: &mut Context) -> Step {
        if let Some(id) = pressed(&mut self.focus, input)
            && id == CANCEL
            && self.operation.state().cancellable
            && !self.operation.is_cancelled()
        {
            self.operation.cancel();
        }
        Step::Stay
    }

    fn tick(&mut self, _context: &mut Context) -> Step {
        let state = self.operation.state();
        if self.finished || !state.active {
            self.finished = true;
            return Step::Close(Outcome::Closed);
        }
        self.sample_at(Instant::now(), &state);
        Step::Stay
    }

    fn focus(&mut self) -> &mut Focus {
        &mut self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::testing::Harness;
    use crossterm::event::KeyCode;

    const MIB: u64 = 1024 * 1024;

    fn running() -> Operation {
        let operation = Operation::silent();
        operation.phase("DESCARGANDO", "Descargando main/tool 1.0 (1/1)", true);
        operation
    }

    #[test]
    fn human_size_como_el_original() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(5 * MIB), "5.0 MB");
        assert_eq!(human_size(3 * 1024 * MIB), "3.0 GB");
        assert_eq!(human_size(2048 * 1024 * MIB), "2048.0 GB");
    }

    #[test]
    fn muestra_los_bytes_recibidos_y_el_total() {
        let harness = Harness::spanish();
        let operation = running();
        operation.transfer(5 * MIB, Some(20 * MIB), "bytes");
        let mut dialog = OperationDialog::new(operation);
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(text.contains("5.0 MB de 20.0 MB"), "{text}");
        assert!(text.contains("DESCARGANDO"), "{text}");
        assert!(text.contains("Cancelar instalación"), "{text}");
        assert!(
            text.contains("[████████████"),
            "una cuarta parte llena: {text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn sin_total_la_barra_es_indeterminada_y_se_dice_recibidos() {
        let harness = Harness::spanish();
        let operation = running();
        operation.transfer(3 * MIB, None, "bytes");
        let mut dialog = OperationDialog::new(operation);
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(text.contains("3.0 MB recibidos"), "{text}");
        assert!(text.contains('█') && text.contains('░'), "{text}");
    }

    #[test]
    fn los_archivos_se_cuentan_con_total_o_interrogacion() {
        let harness = Harness::spanish();
        let operation = running();
        operation.transfer(3, Some(10), "files");
        let mut dialog = OperationDialog::new(operation.clone());
        assert!(harness.draw(&mut dialog, 80, 24).contains("3/10 archivos"));
        operation.transfer(3, None, "files");
        assert!(harness.draw(&mut dialog, 80, 24).contains("3/? archivos"));
    }

    #[test]
    fn la_velocidad_usa_las_muestras_de_los_ultimos_segundos() {
        let operation = running();
        let mut dialog = OperationDialog::new(operation.clone());
        let tr = Translator::new("español");
        let t0 = Instant::now();
        operation.transfer(MIB, Some(20 * MIB), "bytes");
        dialog.sample_at(t0, &operation.state());
        operation.transfer(5 * MIB, Some(20 * MIB), "bytes");
        dialog.sample_at(t0 + Duration::from_secs(2), &operation.state());
        assert_eq!(
            dialog.detail(&operation.state(), &tr),
            "5.0 MB de 20.0 MB · 2.0 MB/s"
        );
        // Una muestra a menos de un segundo de la primera no calcula velocidad.
        let mut quick = OperationDialog::new(operation.clone());
        quick.sample_at(t0, &operation.state());
        quick.sample_at(t0 + Duration::from_millis(500), &operation.state());
        assert_eq!(quick.detail(&operation.state(), &tr), "5.0 MB de 20.0 MB");
        // Las muestras de hace más de 5 s se descartan.
        dialog.sample_at(t0 + Duration::from_secs(9), &operation.state());
        assert_eq!(dialog.samples.len(), 1);
        // Un contador que baja reinicia las muestras.
        operation.transfer(MIB, Some(20 * MIB), "bytes");
        dialog.sample_at(t0 + Duration::from_secs(10), &operation.state());
        assert_eq!(dialog.samples.len(), 1);
    }

    #[test]
    fn cancelar_instalacion_cancela_la_operacion() {
        let mut harness = Harness::new();
        let operation = running();
        let mut dialog = OperationDialog::new(operation.clone());
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(dialog.focus().focused(), Some(CANCEL));
        assert!(!operation.is_cancelled());
        assert_eq!(harness.press(&mut dialog, KeyCode::Enter), Step::Stay);
        assert!(operation.is_cancelled());
        let text = harness.draw(&mut dialog, 80, 24);
        assert!(text.contains("Cancelling…"), "{text}");
    }

    #[test]
    fn el_clic_en_el_boton_cancela() {
        let mut harness = Harness::new();
        let operation = running();
        let mut dialog = OperationDialog::new(operation.clone());
        harness.draw(&mut dialog, 80, 24);
        harness.click(&mut dialog, CANCEL);
        assert!(operation.is_cancelled());
    }

    #[test]
    fn sin_fase_cancelable_el_boton_esta_deshabilitado() {
        let mut harness = Harness::new();
        let operation = Operation::silent();
        operation.phase("PUBLICANDO", "", false);
        let mut dialog = OperationDialog::new(operation.clone());
        harness.draw(&mut dialog, 80, 24);
        assert!(!dialog.focus().is_registered(CANCEL));
        harness.press(&mut dialog, KeyCode::Enter);
        assert!(!operation.is_cancelled());
    }

    #[test]
    fn esc_no_lo_cierra_y_se_cierra_al_terminar_la_operacion() {
        let mut harness = Harness::new();
        let operation = running();
        let mut dialog = OperationDialog::new(operation.clone());
        harness.draw(&mut dialog, 80, 24);
        assert_eq!(harness.press(&mut dialog, KeyCode::Esc), Step::Stay);
        assert_eq!(harness.tick(&mut dialog), Step::Stay);
        operation.finish();
        assert_eq!(harness.tick(&mut dialog), Step::Close(Outcome::Closed));
    }

    #[test]
    fn el_titulo_por_defecto_es_instalando() {
        let harness = Harness::spanish();
        let operation = Operation::silent();
        operation.phase("", "", true);
        let mut dialog = OperationDialog::new(operation);
        assert!(harness.draw(&mut dialog, 80, 24).contains("INSTALANDO"));
    }
}
