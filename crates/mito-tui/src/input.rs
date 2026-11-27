//! Entradas de teclado, ratón y pegado, independientes de crossterm.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

/// Una entrada del usuario ya filtrada: pulsación de tecla, clic izquierdo, rueda o
/// texto pegado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Pulsación de tecla (no se entregan las liberaciones).
    Key(KeyEvent),
    /// Clic izquierdo en la celda (`column`, `row`).
    Click {
        /// Columna de la celda.
        column: u16,
        /// Fila de la celda.
        row: u16,
    },
    /// Giro de la rueda sobre la celda (`column`, `row`).
    Scroll {
        /// Columna de la celda.
        column: u16,
        /// Fila de la celda.
        row: u16,
        /// `true` si la rueda baja el contenido (hacia el final).
        down: bool,
    },
    /// Texto pegado (pegado entre corchetes del terminal).
    Paste(String),
}

impl Input {
    /// Convierte un evento de crossterm. Solo cuentan las pulsaciones
    /// ([`KeyEventKind::Press`]), los clics izquierdos al pulsar, la rueda y el pegado;
    /// el resto (cambios de tamaño, foco, movimiento del ratón…) da `None`.
    pub fn from_event(event: Event) -> Option<Self> {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => Some(Self::Key(key)),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => Some(Self::Click {
                    column: mouse.column,
                    row: mouse.row,
                }),
                MouseEventKind::ScrollDown => Some(Self::Scroll {
                    column: mouse.column,
                    row: mouse.row,
                    down: true,
                }),
                MouseEventKind::ScrollUp => Some(Self::Scroll {
                    column: mouse.column,
                    row: mouse.row,
                    down: false,
                }),
                _ => None,
            },
            Event::Paste(text) => Some(Self::Paste(text)),
            _ => None,
        }
    }

    /// Pulsación de `code` sin modificadores.
    pub fn key(code: KeyCode) -> Self {
        Self::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// Pulsación de un carácter (con Mayús implícito si es mayúscula, como crossterm).
    pub fn character(c: char) -> Self {
        let modifiers = if c.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        Self::Key(KeyEvent::new(KeyCode::Char(c), modifiers))
    }

    /// Pulsación de Ctrl más un carácter.
    pub fn ctrl(c: char) -> Self {
        Self::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    /// Código de la tecla, si es una pulsación.
    pub fn code(&self) -> Option<KeyCode> {
        match self {
            Self::Key(key) => Some(key.code),
            _ => None,
        }
    }

    /// ¿Es una pulsación de `code` con o sin Mayús, sin Ctrl ni Alt?
    pub fn is(&self, code: KeyCode) -> bool {
        match self {
            Self::Key(key) => {
                key.code == code
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            }
            _ => false,
        }
    }

    /// ¿Es Tab (sin Mayús)?
    pub fn is_tab(&self) -> bool {
        matches!(self, Self::Key(key)
            if key.code == KeyCode::Tab
                && !key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT))
    }

    /// ¿Es Mayús+Tab? (crossterm lo informa como `BackTab`, y algunos terminales como
    /// Tab con Mayús).
    pub fn is_back_tab(&self) -> bool {
        matches!(self, Self::Key(key)
            if key.code == KeyCode::BackTab
                || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT)))
    }

    /// ¿Activa un botón? (Intro o Espacio).
    pub fn is_activation(&self) -> bool {
        self.is(KeyCode::Enter) || self.is(KeyCode::Char(' '))
    }

    /// Carácter escrito, si la pulsación inserta texto (sin Ctrl; `AltGr` cuenta, porque
    /// Windows lo informa como Ctrl+Alt).
    pub fn typed(&self) -> Option<char> {
        let Self::Key(key) = self else {
            return None;
        };
        let KeyCode::Char(c) = key.code else {
            return None;
        };
        let modifiers = key.modifiers;
        let alt_gr = modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let control = modifiers.contains(KeyModifiers::CONTROL) && !alt_gr;
        let alt = modifiers.contains(KeyModifiers::ALT) && !alt_gr;
        (!control && !alt && !c.is_control()).then_some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, MouseEvent};

    fn key_event(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        })
    }

    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 7,
            row: 3,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn solo_cuentan_las_pulsaciones() {
        assert_eq!(
            Input::from_event(key_event(KeyCode::Enter, KeyEventKind::Press)),
            Some(Input::key(KeyCode::Enter))
        );
        assert_eq!(
            Input::from_event(key_event(KeyCode::Enter, KeyEventKind::Release)),
            None
        );
        assert_eq!(
            Input::from_event(key_event(KeyCode::Enter, KeyEventKind::Repeat)),
            None
        );
    }

    #[test]
    fn el_clic_izquierdo_y_la_rueda_llevan_la_celda() {
        assert_eq!(
            Input::from_event(mouse(MouseEventKind::Down(MouseButton::Left))),
            Some(Input::Click { column: 7, row: 3 })
        );
        assert_eq!(
            Input::from_event(mouse(MouseEventKind::Down(MouseButton::Right))),
            None
        );
        assert_eq!(Input::from_event(mouse(MouseEventKind::Moved)), None);
        assert_eq!(
            Input::from_event(mouse(MouseEventKind::Up(MouseButton::Left))),
            None
        );
        assert_eq!(
            Input::from_event(mouse(MouseEventKind::ScrollDown)),
            Some(Input::Scroll {
                column: 7,
                row: 3,
                down: true
            })
        );
        assert_eq!(
            Input::from_event(mouse(MouseEventKind::ScrollUp)),
            Some(Input::Scroll {
                column: 7,
                row: 3,
                down: false
            })
        );
    }

    #[test]
    fn el_pegado_y_los_eventos_ajenos() {
        assert_eq!(
            Input::from_event(Event::Paste("hola".into())),
            Some(Input::Paste("hola".into()))
        );
        assert_eq!(Input::from_event(Event::Resize(80, 24)), None);
        assert_eq!(Input::from_event(Event::FocusGained), None);
    }

    #[test]
    fn typed_acepta_texto_y_altgr_pero_no_atajos() {
        assert_eq!(Input::character('a').typed(), Some('a'));
        assert_eq!(Input::character('Ñ').typed(), Some('Ñ'));
        assert_eq!(Input::ctrl('a').typed(), None);
        assert_eq!(Input::key(KeyCode::Enter).typed(), None);
        let alt_gr = Input::Key(KeyEvent::new(
            KeyCode::Char('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ));
        assert_eq!(alt_gr.typed(), Some('@'));
        let alt = Input::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert_eq!(alt.typed(), None);
    }

    #[test]
    fn tab_y_mayus_tab() {
        assert!(Input::key(KeyCode::Tab).is_tab());
        assert!(!Input::key(KeyCode::Tab).is_back_tab());
        assert!(Input::key(KeyCode::BackTab).is_back_tab());
        let shifted = Input::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
        assert!(shifted.is_back_tab() && !shifted.is_tab());
        assert!(!Input::character('a').is_tab());
    }

    #[test]
    fn is_ignora_mayus_pero_no_ctrl() {
        assert!(Input::key(KeyCode::Tab).is(KeyCode::Tab));
        assert!(Input::key(KeyCode::Char(' ')).is_activation());
        assert!(Input::key(KeyCode::Enter).is_activation());
        assert!(!Input::ctrl(' ').is_activation());
        assert!(!Input::Click { column: 0, row: 0 }.is(KeyCode::Enter));
    }
}
