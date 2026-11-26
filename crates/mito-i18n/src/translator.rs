//! Selección de idioma y traducción de mensajes.

use std::borrow::Cow;

use mito_core::config::read_global;
use mito_core::{Layout, Message, Result};

use crate::bundle::{Bundle, bundle};
use crate::{DEFAULT_LANGUAGE, DEFAULT_SETTING, LANGUAGES};

/// Idioma que corresponde a un `LANGID` de Windows.
///
/// Solo cuenta el idioma primario (`LANGID & 0x3ff`), así que las variantes regionales
/// (`0x0c0a`, `0x080a`, `0x0809`…) usan el catálogo de su idioma. Cualquier idioma fuera
/// de los 24 admitidos da `english`.
pub fn language_for_langid(langid: u16) -> &'static str {
    match langid & 0x3ff {
        0x02 => "български",
        0x05 => "čeština",
        0x06 => "dansk",
        0x07 => "deutsch",
        0x08 => "ελληνικά",
        0x0a => "español",
        0x0b => "suomi",
        0x0c => "français",
        0x0e => "magyar",
        0x10 => "italiano",
        0x13 => "nederlands",
        0x15 => "polski",
        0x16 => "português",
        0x18 => "română",
        // Incluye el serbio latino y cirílico, que comparten idioma primario.
        0x1a => "hrvatski",
        0x1b => "slovenčina",
        0x1d => "svenska",
        0x24 => "slovenščina",
        0x25 => "eesti",
        0x26 => "latviešu",
        0x27 => "lietuvių",
        0x3a => "malti",
        0x3c => "gaeilge",
        _ => DEFAULT_LANGUAGE,
    }
}

/// Idioma de la interfaz de Windows del usuario (`GetUserDefaultUILanguage`), no el
/// formato regional. `english` si es un idioma no admitido.
pub fn system_language() -> &'static str {
    language_for_langid(mito_win::system::ui_language_id())
}

/// Traductor de mensajes de un idioma.
///
/// Busca la plantilla española en el catálogo del idioma, luego en el inglés y, por
/// último, usa la propia plantilla. Un catálogo cuyos parámetros no encajan con los del
/// mensaje cuenta como si no tuviera la entrada. Los argumentos se interpolan **después**
/// de traducir y nunca se traducen ni se vuelven a interpolar.
#[derive(Debug, Clone)]
pub struct Translator {
    language: &'static str,
    messages: Cow<'static, Bundle>,
    english: Cow<'static, Bundle>,
}

impl Translator {
    /// Traductor para `setting`: `system` (idioma de Windows), uno de los 24 idiomas o,
    /// para cualquier otro valor, `english`.
    pub fn new(setting: &str) -> Self {
        Self::with_detector(setting, system_language)
    }

    /// Como [`new`](Self::new) con la detección del idioma del sistema inyectada.
    pub fn with_detector(setting: &str, detect: impl FnOnce() -> &'static str) -> Self {
        let wanted = if setting == DEFAULT_SETTING {
            detect()
        } else {
            setting
        };
        let language = LANGUAGES
            .iter()
            .find(|l| **l == wanted)
            .copied()
            .unwrap_or(DEFAULT_LANGUAGE);
        Self {
            language,
            messages: Cow::Borrowed(bundle(language)),
            english: Cow::Borrowed(bundle(DEFAULT_LANGUAGE)),
        }
    }

    /// Traductor con catálogos propios (para herramientas y pruebas). `language` debe
    /// ser uno de [`LANGUAGES`]; si no, se usa `english`.
    pub fn with_bundles(language: &str, messages: Bundle, english: Bundle) -> Self {
        let language = LANGUAGES
            .iter()
            .find(|l| **l == language)
            .copied()
            .unwrap_or(DEFAULT_LANGUAGE);
        Self {
            language,
            messages: Cow::Owned(messages),
            english: Cow::Owned(english),
        }
    }

    /// Traductor según `mito.language` de `config.properties` de la instancia; sin
    /// archivo o sin propiedad, `system`.
    ///
    /// # Errors
    ///
    /// El error de lectura de `config.properties`.
    pub fn from_layout(layout: &Layout) -> Result<Self> {
        Self::from_layout_with_detector(layout, system_language)
    }

    /// Como [`from_layout`](Self::from_layout) con la detección inyectada. La detección
    /// solo se invoca si el valor es `system`.
    ///
    /// # Errors
    ///
    /// El error de lectura de `config.properties`.
    pub fn from_layout_with_detector(
        layout: &Layout,
        detect: impl FnOnce() -> &'static str,
    ) -> Result<Self> {
        let values = read_global(layout)?;
        let setting = values.get("mito.language").unwrap_or(DEFAULT_SETTING);
        Ok(Self::with_detector(setting, detect))
    }

    /// Idioma activo (uno de [`LANGUAGES`]).
    pub fn language(&self) -> &'static str {
        self.language
    }

    /// Traduce un mensaje e interpola sus argumentos.
    pub fn tr(&self, message: &Message) -> String {
        [&*self.messages, &*self.english]
            .into_iter()
            .filter_map(|bundle| bundle.get(message.template()))
            .find_map(|template| message.render_with(template))
            .unwrap_or_else(|| message.to_string())
    }

    /// Traduce un texto fijo sin argumentos (etiquetas, atajos). Una clave desconocida
    /// se devuelve tal cual.
    pub fn text<'a>(&'a self, template: &'a str) -> &'a str {
        self.messages
            .get(template)
            .or_else(|| self.english.get(template))
            .unwrap_or(template)
    }
}

impl Default for Translator {
    /// Traductor del idioma de Windows.
    fn default() -> Self {
        Self::new(DEFAULT_SETTING)
    }
}
