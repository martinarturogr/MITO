//! Dibujo de la pantalla principal: cabecera, cuerpo, panel de log y pie.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::state::{App, Page};
use crate::theme;
use crate::widgets::{Button, Ui, notice};

/// Ancho de la barra lateral.
const SIDEBAR_WIDTH: u16 = 34;
/// Filas del panel de log (con su borde superior).
const LOG_HEIGHT: u16 = 8;
/// Ancho del botón «Copiar log».
const COPY_WIDTH: u16 = 14;

impl App {
    /// Dibuja la interfaz entera. Si un widget recién mostrado debía recibir el foco
    /// (Ctrl+F), se vuelve a dibujar con el foco ya puesto.
    pub fn render(&mut self, frame: &mut Frame) {
        self.render_once(frame);
        // Sin foco todavía (primer fotograma), lo tiene el primer widget.
        if self.focus.focused().is_none()
            && let Some(first) = self.focus.order().first().cloned()
        {
            self.focus.set(&first);
        }
        if let Some(id) = self.pending_focus.take()
            && self.dialogs.is_empty()
            && self.focus.is_registered(&id)
        {
            self.focus.set(&id);
            self.render_once(frame);
        }
    }

    fn render_once(&mut self, frame: &mut Frame) {
        let screen = frame.area();
        let tr = self.tr.clone();
        let mut ui = self.focus.frame(&tr);
        frame.buffer_mut().set_style(screen, theme::screen());
        let [header, body, log, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(LOG_HEIGHT),
            Constraint::Length(1),
        ])
        .areas(screen);
        render_header(frame, header);
        let [sidebar, page] =
            Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Fill(1)])
                .areas(body);
        self.render_sidebar(frame, sidebar, &mut ui);
        let page = Rect::new(
            page.x.saturating_add(2),
            page.y.saturating_add(1),
            page.width.saturating_sub(4),
            page.height.saturating_sub(1),
        );
        match self.page {
            Page::Home => self.render_home(frame, page, &mut ui),
            Page::Catalog => self.render_catalog(frame, page, &mut ui),
        }
        self.render_log(frame, log, &mut ui);
        render_footer(frame, footer, &ui);
        self.profile.render_overlay(frame, &mut ui);
        self.render_dialogs(frame, screen);
        notice::render(frame, screen, &self.notices);
    }

    /// Panel de log con «Copiar log» a la derecha.
    fn render_log(&mut self, frame: &mut Frame, area: Rect, ui: &mut Ui) {
        let border = Style::new().fg(theme::LOG_BORDER).bg(theme::SCREEN);
        let line = "─".repeat(usize::from(area.width));
        frame.render_widget(
            Paragraph::new(line).style(border),
            Rect::new(area.x, area.y, area.width, 1),
        );
        let body = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        );
        let log = Rect::new(
            body.x,
            body.y,
            body.width.saturating_sub(COPY_WIDTH + 2),
            body.height,
        );
        self.log.render(frame, log, ui);
        let button = Rect::new(
            body.right().saturating_sub(COPY_WIDTH + 1),
            body.y.saturating_add(1),
            COPY_WIDTH.min(body.width),
            1,
        );
        Button::new("copy-log", ui.tr.text("Copiar log")).render(frame, button, ui);
    }
}

/// Cabecera: `MITO · {versión}` y el subtítulo, centrados.
fn render_header(frame: &mut Frame, area: Rect) {
    let base = Style::new().fg(theme::TEXT).bg(theme::HEADER);
    frame.buffer_mut().set_style(area, base);
    let title = format!("MITO · {}", env!("CARGO_PKG_VERSION"));
    let line = Line::from(vec![
        Span::styled(title, base.add_modifier(Modifier::BOLD)),
        Span::styled(
            " — My Integrated Terminal Orchestrator",
            base.fg(theme::SUMMARY),
        ),
    ]);
    frame.render_widget(Paragraph::new(line).centered().style(base), area);
}

/// Pie con los atajos traducidos.
fn render_footer(frame: &mut Frame, area: Rect, ui: &Ui) {
    let base = Style::new().fg(theme::SUMMARY).bg(theme::SIDEBAR);
    let key = Style::new()
        .fg(theme::TITLE)
        .bg(theme::SIDEBAR)
        .add_modifier(Modifier::BOLD);
    frame.buffer_mut().set_style(area, base);
    let mut spans = Vec::new();
    for (shortcut, label) in [
        ("^q", "Salir"),
        ("^f", "Buscar componentes"),
        ("esc", "Inicio"),
    ] {
        spans.push(Span::styled(format!(" {shortcut} "), key));
        spans.push(Span::styled(ui.tr.text(label).to_owned(), base));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(base), area);
}
