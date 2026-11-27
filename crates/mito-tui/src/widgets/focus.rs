//! Foco y zonas pulsables de una pantalla (principal o diálogo).
//!
//! Los widgets se dibujan en modo inmediato: en cada fotograma registran su zona y su
//! posición en el orden de foco en un [`Ui`]; el [`Focus`] de la pantalla, que sobrevive
//! entre fotogramas, resuelve con ese registro las teclas Tab/Mayús+Tab y los clics.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mito_i18n::Translator;
use ratatui::layout::{Position, Rect};

/// Identificador lógico de un widget (los ids del original: `"install"`, `"remove-3"`…).
pub type WidgetId = String;

#[derive(Debug, Default)]
struct Registry {
    /// Widgets enfocables, en orden de registro (= orden de Tab).
    order: Vec<(WidgetId, Rect)>,
    /// Zonas pulsables (los enfocables y las que solo reciben clics).
    zones: Vec<(WidgetId, Rect)>,
    /// Zonas superpuestas (listas desplegables), que ganan a cualquier otra.
    overlays: Vec<(WidgetId, Rect)>,
}

type Shared = Arc<Mutex<Registry>>;

fn lock(shared: &Shared) -> MutexGuard<'_, Registry> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Contexto de un fotograma de una pantalla (principal o diálogo).
#[derive(Debug)]
pub struct Ui<'t> {
    /// Traductor de la interfaz.
    pub tr: &'t Translator,
    focused: Option<WidgetId>,
    shared: Shared,
}

impl Ui<'_> {
    /// Registra un widget enfocable en `area`. El orden de registro es el orden de Tab.
    /// Un widget deshabilitado u oculto no se registra.
    pub fn focusable(&mut self, id: &str, area: Rect) {
        let mut registry = lock(&self.shared);
        registry.order.push((id.to_owned(), area));
        registry.zones.push((id.to_owned(), area));
    }

    /// Registra una zona que solo recibe clics (no entra en el orden de Tab).
    pub fn zone(&mut self, id: &str, area: Rect) {
        lock(&self.shared).zones.push((id.to_owned(), area));
    }

    /// Registra una zona superpuesta (lista desplegable): gana a las zonas de debajo.
    pub fn overlay(&mut self, id: &str, area: Rect) {
        lock(&self.shared).overlays.push((id.to_owned(), area));
    }

    /// ¿Tiene `id` el foco en este fotograma?
    ///
    /// Si ningún widget tiene el foco todavía (primer fotograma), lo tiene el primero
    /// que se registre.
    pub fn is_focused(&self, id: &str) -> bool {
        match &self.focused {
            Some(focused) => focused == id,
            None => lock(&self.shared)
                .order
                .first()
                .is_some_and(|(first, _)| first == id),
        }
    }

    /// Id con el foco en este fotograma, si lo hay.
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }
}

/// Foco y zonas de una pantalla, que sobreviven entre fotogramas.
#[derive(Debug, Default)]
pub struct Focus {
    focused: Option<WidgetId>,
    shared: Shared,
}

impl Focus {
    /// Foco vacío: el primer widget que se registre lo recibe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Foco que empieza en `id`.
    pub fn starting_at(id: &str) -> Self {
        let mut focus = Self::new();
        focus.set(id);
        focus
    }

    /// Id enfocado.
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// Enfoca `id`. Si no se registra en el siguiente fotograma, el foco pasará al
    /// primer widget registrado.
    pub fn set(&mut self, id: &str) {
        self.focused = Some(id.to_owned());
    }

    /// ¿Está registrado `id` (enfocable) en el último fotograma?
    pub fn is_registered(&self, id: &str) -> bool {
        lock(&self.shared).order.iter().any(|(i, _)| i == id)
    }

    /// Ids enfocables del último fotograma, en orden de Tab.
    pub fn order(&self) -> Vec<WidgetId> {
        lock(&self.shared)
            .order
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Zona que ocupó `id` en el último fotograma.
    pub fn area_of(&self, id: &str) -> Option<Rect> {
        lock(&self.shared)
            .zones
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, area)| *area)
    }

    /// Pasa el foco al siguiente widget (Tab), de forma cíclica.
    pub fn next(&mut self) {
        self.step(true);
    }

    /// Pasa el foco al widget anterior (Mayús+Tab), de forma cíclica.
    pub fn previous(&mut self) {
        self.step(false);
    }

    fn step(&mut self, forward: bool) {
        let registry = lock(&self.shared);
        let count = registry.order.len();
        if count == 0 {
            return;
        }
        // Sin foco todavía, el primero registrado es el que se ve enfocado.
        let current = self
            .focused
            .as_deref()
            .and_then(|id| registry.order.iter().position(|(i, _)| i == id))
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        let id = registry.order[next].0.clone();
        drop(registry);
        self.focused = Some(id);
    }

    /// Id de la zona más interna bajo la celda (`column`, `row`) en el último
    /// fotograma. Las listas desplegables ganan a todo lo demás; entre zonas
    /// superpuestas gana la de menor área y, a igualdad, la registrada después.
    ///
    /// Difiere de la firma del diseño (`Option<&str>`) en que devuelve el id por valor,
    /// porque las zonas viven detrás de un registro compartido con [`Ui`].
    pub fn hit(&self, column: u16, row: u16) -> Option<WidgetId> {
        let position = Position::new(column, row);
        let registry = lock(&self.shared);
        if let Some((id, _)) = registry
            .overlays
            .iter()
            .rev()
            .find(|(_, area)| area.contains(position))
        {
            return Some(id.clone());
        }
        let mut best: Option<&(WidgetId, Rect)> = None;
        for zone in &registry.zones {
            if !zone.1.contains(position) {
                continue;
            }
            if best.is_none_or(|b| zone.1.area() <= b.1.area()) {
                best = Some(zone);
            }
        }
        best.map(|(id, _)| id.clone())
    }

    /// Empieza un fotograma: olvida las zonas del anterior y devuelve el contexto de
    /// dibujo. Si el id enfocado ya no estaba registrado, el foco pasa al primero
    /// registrado en el fotograma anterior.
    pub fn frame<'t>(&mut self, tr: &'t Translator) -> Ui<'t> {
        let mut registry = lock(&self.shared);
        let first = registry.order.first().map(|(id, _)| id.clone());
        let still_there = self
            .focused
            .as_deref()
            .is_some_and(|id| registry.order.iter().any(|(i, _)| i == id));
        if !still_there && first.is_some() {
            self.focused = first;
        }
        registry.order.clear();
        registry.zones.clear();
        registry.overlays.clear();
        drop(registry);
        Ui {
            tr,
            focused: self.focused.clone(),
            shared: Arc::clone(&self.shared),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(focus: &mut Focus, tr: &Translator, ids: &[(&str, Rect)]) {
        let mut ui = focus.frame(tr);
        for (id, area) in ids {
            ui.focusable(id, *area);
        }
    }

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn tab_recorre_en_orden_de_registro_y_es_ciclico() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        register(
            &mut focus,
            &tr,
            &[
                ("a", rect(0, 0, 3, 1)),
                ("b", rect(4, 0, 3, 1)),
                ("c", rect(8, 0, 3, 1)),
            ],
        );
        // Sin foco explícito se ve enfocado el primero, así que Tab pasa al segundo.
        focus.next();
        assert_eq!(focus.focused(), Some("b"));
        focus.next();
        assert_eq!(focus.focused(), Some("c"));
        focus.next();
        assert_eq!(focus.focused(), Some("a"));
        focus.previous();
        assert_eq!(focus.focused(), Some("c"));
    }

    #[test]
    fn el_primer_fotograma_enfoca_al_primero_registrado() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        {
            let mut ui = focus.frame(&tr);
            ui.focusable("a", rect(0, 0, 3, 1));
            ui.focusable("b", rect(4, 0, 3, 1));
            assert!(ui.is_focused("a"));
            assert!(!ui.is_focused("b"));
        }
        let ui = focus.frame(&tr);
        assert!(ui.is_focused("a"));
        assert_eq!(focus.focused(), Some("a"));
    }

    #[test]
    fn si_el_enfocado_desaparece_pasa_al_primero() {
        let tr = Translator::new("english");
        let mut focus = Focus::starting_at("c");
        register(
            &mut focus,
            &tr,
            &[("a", rect(0, 0, 3, 1)), ("c", rect(4, 0, 3, 1))],
        );
        assert_eq!(focus.focused(), Some("c"));
        register(
            &mut focus,
            &tr,
            &[("a", rect(0, 0, 3, 1)), ("b", rect(4, 0, 3, 1))],
        );
        // El foco se resuelve al empezar el siguiente fotograma.
        focus.frame(&tr);
        assert_eq!(focus.focused(), Some("a"));
    }

    #[test]
    fn un_id_no_registrado_no_recibe_foco_ni_clics() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        register(&mut focus, &tr, &[("a", rect(0, 0, 3, 1))]);
        assert!(!focus.is_registered("off"));
        focus.next();
        focus.next();
        assert_eq!(focus.focused(), Some("a"));
        assert_eq!(focus.hit(10, 0), None);
    }

    #[test]
    fn el_clic_elige_la_zona_mas_interna() {
        let tr = Translator::new("english");
        let mut focus = Focus::new();
        {
            let mut ui = focus.frame(&tr);
            ui.zone("outer", rect(0, 0, 20, 5));
            ui.focusable("inner", rect(2, 1, 4, 1));
            ui.overlay("drop", rect(3, 1, 2, 3));
        }
        assert_eq!(focus.hit(2, 1).as_deref(), Some("inner"));
        assert_eq!(focus.hit(10, 3).as_deref(), Some("outer"));
        assert_eq!(focus.hit(3, 1).as_deref(), Some("drop"));
        assert_eq!(focus.hit(30, 3), None);
    }

    #[test]
    fn el_foco_no_cambia_sin_widgets() {
        let mut focus = Focus::new();
        focus.next();
        focus.previous();
        assert_eq!(focus.focused(), None);
    }
}
