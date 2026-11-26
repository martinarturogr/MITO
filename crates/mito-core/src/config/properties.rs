//! Tabla de propiedades reconocidas de `config.properties` (`PROPERTIES` del original).
//!
//! Las claves `posthog.*` del original no están: MITO no envía informes remotos
//! (ADR 0007). Si el archivo las tiene, son claves desconocidas y se conservan.

/// Idiomas de la interfaz, en el orden del original. Los nombres son los de los
/// bundles de `mito-i18n` y los valores válidos de `mito.language`.
pub const LANGUAGES: [&str; 24] = [
    "español",
    "english",
    "български",
    "hrvatski",
    "čeština",
    "dansk",
    "nederlands",
    "eesti",
    "suomi",
    "français",
    "deutsch",
    "ελληνικά",
    "magyar",
    "gaeilge",
    "italiano",
    "latviešu",
    "lietuvių",
    "malti",
    "polski",
    "português",
    "română",
    "slovenčina",
    "slovenščina",
    "svenska",
];

/// Valor de `mito.language` que elige el idioma de Windows.
pub const DEFAULT_SETTING: &str = "system";

/// Valores válidos de `mito.language`: `system` y los 24 idiomas.
pub const LANGUAGE_CHOICES: [&str; 25] = [
    DEFAULT_SETTING,
    LANGUAGES[0],
    LANGUAGES[1],
    LANGUAGES[2],
    LANGUAGES[3],
    LANGUAGES[4],
    LANGUAGES[5],
    LANGUAGES[6],
    LANGUAGES[7],
    LANGUAGES[8],
    LANGUAGES[9],
    LANGUAGES[10],
    LANGUAGES[11],
    LANGUAGES[12],
    LANGUAGES[13],
    LANGUAGES[14],
    LANGUAGES[15],
    LANGUAGES[16],
    LANGUAGES[17],
    LANGUAGES[18],
    LANGUAGES[19],
    LANGUAGES[20],
    LANGUAGES[21],
    LANGUAGES[22],
    LANGUAGES[23],
];

/// Prefijos de las familias dinámicas de claves.
pub const PREFIXES: [&str; 3] = ["bucket.", "proxy_authentication_form_field.", "pocketool."];

/// Una propiedad reconocida: su lista cerrada de valores (si la tiene) y su valor por
/// defecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property {
    /// Nombre de la clave.
    pub key: &'static str,
    /// Valores permitidos; `None` si el texto es libre.
    pub choices: Option<&'static [&'static str]>,
    /// Valor por defecto cuando la clave no está en el archivo.
    pub default: &'static str,
}

const BOOLEAN: &[&str] = &["true", "false"];
const SHELLS: &[&str] = &["cmd", "powershell", "mito"];
const COLORS: &[&str] = &[
    "negro", "rojo", "azul", "naranja", "rosa", "verde", "marron", "morado",
];

const fn free(key: &'static str, default: &'static str) -> Property {
    Property {
        key,
        choices: None,
        default,
    }
}

const fn closed(
    key: &'static str,
    choices: &'static [&'static str],
    default: &'static str,
) -> Property {
    Property {
        key,
        choices: Some(choices),
        default,
    }
}

/// Propiedades reconocidas, en el orden del original.
pub const PROPERTIES: [Property; 18] = [
    closed("terminal.defaultShell", SHELLS, "cmd"),
    closed("mcp.jobs.monitor", BOOLEAN, "false"),
    closed("mito.language", &LANGUAGE_CHOICES, DEFAULT_SETTING),
    free(
        "mito.update_url",
        "https://api.github.com/repos/danielgube/mito/releases/latest",
    ),
    closed("enable_proxy_authentication", BOOLEAN, "false"),
    free("proxy_authentication_url", ""),
    free("proxy_authentication_username_field", "username"),
    free("proxy_authentication_password_field", "password"),
    closed("proxy_authentication_require_https", BOOLEAN, "true"),
    free("http_proxy", ""),
    free("https_proxy", ""),
    free("no_proxy", ""),
    free("network.check_url", "https://www.gstatic.com/generate_204"),
    free("network.check_status", "204"),
    free("network.timeout_seconds", "4"),
    free("network.poll_seconds", "20"),
    free("shortcut.badgeText", "MITO"),
    closed("shortcut.badgeColor", COLORS, "naranja"),
];

/// Busca una propiedad reconocida por su clave exacta.
pub fn lookup(key: &str) -> Option<&'static Property> {
    PROPERTIES.iter().find(|p| p.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hay_veinticuatro_idiomas_sin_repetir() {
        let mut sorted = LANGUAGES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 24);
        assert_eq!(LANGUAGE_CHOICES.len(), 25);
        assert_eq!(LANGUAGE_CHOICES[0], "system");
    }

    #[test]
    fn los_valores_por_defecto_cumplen_su_lista_cerrada() {
        for property in &PROPERTIES {
            if let Some(choices) = property.choices {
                assert!(choices.contains(&property.default), "{}", property.key);
            }
        }
    }

    #[test]
    fn las_claves_posthog_no_se_reconocen() {
        assert!(lookup("posthog.enabled").is_none());
        assert!(lookup("shortcut.badgeColor").is_some());
        assert_eq!(
            lookup("network.check_status").map(|p| p.default),
            Some("204")
        );
    }
}
