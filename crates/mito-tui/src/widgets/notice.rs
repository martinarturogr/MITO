//! Avisos temporales (los `notify` del original), apilados abajo a la derecha.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::text;
use crate::theme;

/// Duración por defecto de un aviso.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Duración del aviso del cambio de idioma.
pub const LANGUAGE_TIMEOUT: Duration = Duration::from_secs(10);

/// Ancho máximo de un aviso.
const MAX_WIDTH: u16 = 60;

/// Gravedad de un aviso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    /// Informativo (sin severidad en el original).
    #[default]
    Info,
    /// Advertencia.
    Warning,
    /// Error.
    Error,
}

impl Severity {
    fn color(self) -> Color {
        match self {
            Self::Info => theme::TITLE,
            Self::Warning => theme::BORDER_WARNING,
            Self::Error => theme::BORDER_DANGER,
        }
    }
}

/// Aviso con texto ya traducido y hora de caducidad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Texto, ya traducido.
    pub text: String,
    /// Gravedad.
    pub severity: Severity,
    /// Instante a partir del cual el aviso desaparece.
    pub until: Instant,
}

impl Notice {
    /// Aviso que dura [`DEFAULT_TIMEOUT`].
    pub fn new(text: impl Into<String>, severity: Severity) -> Self {
        Self::with_timeout(text, severity, DEFAULT_TIMEOUT)
    }

    /// Aviso con una duración propia.
    pub fn with_timeout(text: impl Into<String>, severity: Severity, timeout: Duration) -> Self {
        Self {
            text: text.into(),
            severity,
            until: Instant::now() + timeout,
        }
    }

    /// ¿Ha caducado en `now`?
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.until
    }
}

/// Dibuja los avisos apilados en la esquina inferior derecha de `screen`, el más
/// reciente (el último) abajo, sin tapar la última fila (el pie de la pantalla).
pub fn render(frame: &mut Frame, screen: Rect, notices: &[Notice]) {
    let mut bottom = screen.bottom().saturating_sub(1);
    let width = MAX_WIDTH.min(screen.width.saturating_sub(2));
    if width < 8 {
        return;
    }
    for notice in notices.iter().rev() {
        let lines = text::wrap(&notice.text, usize::from(width - 2));
        let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
        if bottom < screen.y + height {
            break;
        }
        let area = Rect::new(
            screen.right().saturating_sub(width + 1),
            bottom - height,
            width,
            height,
        );
        bottom -= height;
        frame.render_widget(Clear, area);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(notice.severity.color()).bg(theme::SIDEBAR))
            .style(theme::panel());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(lines.join("\n")).style(theme::panel()),
            inner,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::testing;

    #[test]
    fn caducan_a_los_cinco_segundos() {
        let notice = Notice::new("hola", Severity::Info);
        assert!(!notice.expired(Instant::now()));
        assert!(notice.expired(Instant::now() + Duration::from_secs(6)));
        let long = Notice::with_timeout("x", Severity::Warning, LANGUAGE_TIMEOUT);
        assert!(!long.expired(Instant::now() + Duration::from_secs(6)));
    }

    #[test]
    fn se_apilan_abajo_a_la_derecha_con_el_ultimo_abajo() {
        let notices = vec![
            Notice::new("Primero", Severity::Info),
            Notice::new(
                "Segundo aviso con un texto bastante largo para que ocupe varias líneas",
                Severity::Error,
            ),
        ];
        let buffer = testing::render_buffer(40, 10, |frame, area| {
            render(frame, area, &notices);
        });
        insta::assert_snapshot!(testing::buffer_text(&buffer));
        assert_eq!(buffer[(1, 5)].fg, theme::BORDER_DANGER);
        assert_eq!(buffer[(1, 2)].fg, theme::TITLE);
    }
}
