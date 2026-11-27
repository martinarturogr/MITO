//! Medida y ajuste de texto por celdas de pantalla.

use ratatui::text::Span;

/// Ancho en celdas de pantalla de `text`.
pub fn width(text: &str) -> usize {
    Span::raw(text).width()
}

/// Recorta `text` a `max` celdas, con `…` al final si no cabe.
pub fn truncate(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_width(c);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn char_width(c: char) -> usize {
    let mut buffer = [0u8; 4];
    width(c.encode_utf8(&mut buffer))
}

/// Parte `text` en líneas de como máximo `max` celdas. Respeta los saltos de línea,
/// ajusta por palabras y parte las palabras que no caben. Un `max` de 0 se trata como 1.
pub fn wrap(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        wrap_paragraph(paragraph.trim_end_matches('\r'), max, &mut lines);
    }
    lines
}

fn wrap_paragraph(paragraph: &str, max: usize, lines: &mut Vec<String>) {
    if paragraph.is_empty() {
        lines.push(String::new());
        return;
    }
    let mut current = String::new();
    let mut used = 0;
    for word in paragraph.split(' ') {
        let word_width = width(word);
        let separator = usize::from(used > 0);
        if used > 0 && used + separator + word_width > max {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        if word_width > max {
            // Palabra más larga que la línea (la línea anterior ya se cerró): se parte
            // por caracteres.
            for c in word.chars() {
                let w = char_width(c);
                if used + w > max {
                    lines.push(std::mem::take(&mut current));
                    used = 0;
                }
                current.push(c);
                used += w;
            }
            continue;
        }
        if used > 0 {
            current.push(' ');
            used += 1;
        }
        current.push_str(word);
        used += word_width;
    }
    lines.push(current);
}

/// Número de líneas que ocupa `text` ajustado a `max` celdas.
pub fn height(text: &str, max: usize) -> u16 {
    u16::try_from(wrap(text, max).len()).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ajusta_por_palabras_y_respeta_saltos() {
        assert_eq!(
            wrap("uno dos tres\ncuatro", 7),
            vec!["uno dos", "tres", "cuatro"]
        );
        assert_eq!(wrap("", 5), vec![""]);
        assert_eq!(wrap("a\n\nb", 5), vec!["a", "", "b"]);
    }

    #[test]
    fn parte_las_palabras_largas() {
        assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap("x abcdefgh", 4), vec!["x", "abcd", "efgh"]);
    }

    #[test]
    fn cuenta_celdas_no_caracteres() {
        assert_eq!(width("añb"), 3);
        assert_eq!(width("日本"), 4);
        assert_eq!(wrap("日本語", 4), vec!["日本", "語"]);
    }

    #[test]
    fn recorta_con_puntos_suspensivos() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(height("uno dos\ntres", 3), 4);
    }
}
