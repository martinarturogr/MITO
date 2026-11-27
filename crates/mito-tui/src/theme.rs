//! Colores y estilos del CSS del original (`YitoApp.CSS`, ver `ui.md`, «Estilo»).
//!
//! Todos los colores son literales fijos, como en el original: no hay tokens de tema.

use ratatui::style::{Color, Modifier, Style};

/// Fondo de la pantalla (`Screen`).
pub const SCREEN: Color = Color::from_u32(0x0010_2820);
/// Color del texto (`Screen`).
pub const TEXT: Color = Color::from_u32(0x00e7_efdf);
/// Fondo de la cabecera (`Header`).
pub const HEADER: Color = Color::from_u32(0x0021_5139);
/// Fondo de la barra lateral y de los paneles de los diálogos.
pub const SIDEBAR: Color = Color::from_u32(0x0015_3429);
/// Color de los títulos (`.title`) y del borde por defecto.
pub const TITLE: Color = Color::from_u32(0x00b5_dc64);
/// Fondo del detalle (`#details`).
pub const DETAILS: Color = Color::from_u32(0x001b_3b2e);
/// Fila oscura de las listas (`.component-row-dark`).
pub const ROW_DARK: Color = Color::from_u32(0x001b_3b2e);
/// Fila clara de las listas (`.component-row-light`).
pub const ROW_LIGHT: Color = Color::from_u32(0x0029_513d);
/// Texto de resumen (`.summary`).
pub const SUMMARY: Color = Color::from_u32(0x0095_a9bd);
/// Borde superior del panel de log (`#log-panel`).
pub const LOG_BORDER: Color = Color::from_u32(0x0026_4858);
/// Indicador de ocupado (`#busy-indicator`).
pub const BUSY: Color = Color::from_u32(0x00ff_d166);
/// Casilla encendida.
pub const CHECK_ON: Color = Color::from_u32(0x0069_ff64);
/// Casilla apagada.
pub const CHECK_OFF: Color = Color::from_u32(0x008a_4b50);
/// Borde de los diálogos por defecto.
pub const BORDER: Color = TITLE;
/// Borde de los diálogos destructivos o de integridad.
pub const BORDER_DANGER: Color = Color::from_u32(0x00e2_7878);
/// Borde de los diálogos de aviso (descargas sin hash, progreso, paso largo).
pub const BORDER_WARNING: Color = Color::from_u32(0x00e5_ba65);
/// Texto de error.
pub const ERROR_TEXT: Color = Color::from_u32(0x00ff_7070);
/// Texto de error de la etiqueta del icono (`#badge-error`).
pub const BADGE_ERROR: Color = Color::from_u32(0x00ff_9090);
/// Aviso de catálogo vacío.
pub const EMPTY_CATALOG: Color = Color::from_u32(0x004d_a6ff);
/// Fondo del botón de cierre con foco (`.dialog-close:focus`).
pub const CLOSE_FOCUS: Color = Color::from_u32(0x0056_3330);
/// Fondo de los botones sin variante.
pub const BUTTON_DEFAULT: Color = Color::from_u32(0x0046_5a50);
/// Botón primario (`primary` de Textual).
pub const BUTTON_PRIMARY: Color = Color::from_u32(0x0001_78d4);
/// Botón de éxito (`success` de Textual).
pub const BUTTON_SUCCESS: Color = Color::from_u32(0x004e_bf71);
/// Botón de advertencia (`warning` de Textual).
pub const BUTTON_WARNING: Color = Color::from_u32(0x00ff_a62b);
/// Botón de error (`error` de Textual).
pub const BUTTON_ERROR: Color = Color::from_u32(0x00ba_3c5b);
/// Fondo de los campos de texto y selectores.
pub const INPUT_BG: Color = Color::from_u32(0x000d_2019);
/// Texto atenuado (marcadores de posición, elementos deshabilitados).
pub const DIM: Color = Color::from_u32(0x0072_8a7d);

/// Estilo base de la pantalla: texto sobre fondo de pantalla.
pub fn screen() -> Style {
    Style::new().fg(TEXT).bg(SCREEN)
}

/// Estilo del panel de un diálogo.
pub fn panel() -> Style {
    Style::new().fg(TEXT).bg(SIDEBAR)
}

/// Estilo de un título (`.title`): negrita en `#b5dc64`.
pub fn title() -> Style {
    Style::new()
        .fg(TITLE)
        .bg(SIDEBAR)
        .add_modifier(Modifier::BOLD)
}

/// Estilo de un texto de error sobre el panel.
pub fn error() -> Style {
    Style::new().fg(ERROR_TEXT).bg(SIDEBAR)
}

/// Estilo de un texto atenuado sobre el panel.
pub fn dim() -> Style {
    Style::new().fg(DIM).bg(SIDEBAR)
}

/// Estilo de un campo de texto o selector.
pub fn input() -> Style {
    Style::new().fg(TEXT).bg(INPUT_BG)
}

/// Estilo de un campo de texto con el foco.
pub fn input_focused() -> Style {
    Style::new().fg(TEXT).bg(ROW_LIGHT)
}

/// Estilo del texto de solo lectura (manifiesto, detalle, descargas).
pub fn details() -> Style {
    Style::new().fg(TEXT).bg(DETAILS)
}

/// Estilo del elemento con el cursor en una lista o tabla.
pub fn cursor() -> Style {
    Style::new()
        .fg(SCREEN)
        .bg(TITLE)
        .add_modifier(Modifier::BOLD)
}

/// Oscurece un color RGB mezclándolo con negro (`factor` entre 0 y 100, en %).
/// Los colores que no son RGB se devuelven sin cambios.
pub fn darken(color: Color, factor: u16) -> Color {
    match color {
        Color::Rgb(r, g, b) => {
            let scale = |c: u8| {
                let value = u16::from(c) * (100 - factor.min(100)) / 100;
                u8::try_from(value).unwrap_or(u8::MAX)
            };
            Color::Rgb(scale(r), scale(g), scale(b))
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_colores_son_los_del_css_original() {
        assert_eq!(SCREEN, Color::Rgb(0x10, 0x28, 0x20));
        assert_eq!(TITLE, Color::Rgb(0xb5, 0xdc, 0x64));
        assert_eq!(BORDER_DANGER, Color::Rgb(0xe2, 0x78, 0x78));
        assert_eq!(BUTTON_PRIMARY, Color::Rgb(0x01, 0x78, 0xd4));
        assert_eq!(CHECK_ON, Color::Rgb(0x69, 0xff, 0x64));
    }

    #[test]
    fn oscurecer_mezcla_con_negro() {
        assert_eq!(
            darken(Color::Rgb(200, 100, 50), 50),
            Color::Rgb(100, 50, 25)
        );
        assert_eq!(darken(Color::Reset, 50), Color::Reset);
        assert_eq!(darken(Color::Rgb(9, 9, 9), 100), Color::Rgb(0, 0, 0));
    }
}
