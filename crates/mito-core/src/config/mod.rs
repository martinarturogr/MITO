//! `config.properties` y el bloque `env` de los profiles (`configuration.py` del original).
//!
//! Contrato: `docs/spec/modules/configuration.md`. La configuración global es un
//! documento de texto que se conserva al guardar (comentarios, líneas vacías, orden y
//! claves desconocidas, incluidas las `posthog.*` de instancias creadas por el
//! original, que MITO no interpreta). El guardado valida todo antes de escribir, toma
//! el [`MutationLock`] y detecta cambios externos desde la apertura.

mod properties;
mod rules;
mod text;

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::error::{Error, Result, bail};
use crate::layout::Layout;
use crate::lock::MutationLock;
use crate::message::Message;
use crate::storage::{identifier, read_json, write_atomic, write_json};

pub use properties::{
    DEFAULT_SETTING, LANGUAGE_CHOICES, LANGUAGES, PREFIXES, PROPERTIES, Property, lookup,
};
pub use rules::{badge_text, validate_env, validate_source, validate_url, validated};
pub use text::{Values, parse, render};

/// Lee `config.properties` de la instancia. Un archivo ausente da valores vacíos.
///
/// # Errors
///
/// [`Error::Io`] si no se puede leer o no es UTF-8.
pub fn read_global(layout: &Layout) -> Result<Values> {
    let path = layout.config_file();
    let bytes = read_optional(&path)?;
    Ok(parse(decode_text(&path, bytes.as_deref())?))
}

fn read_optional(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e)),
    }
}

fn decode_text<'a>(path: &std::path::Path, bytes: Option<&'a [u8]>) -> Result<&'a str> {
    match bytes {
        None => Ok(""),
        Some(bytes) => text::decode(bytes).ok_or_else(|| {
            Error::io(
                path,
                io::Error::new(io::ErrorKind::InvalidData, "el archivo no es UTF-8"),
            )
        }),
    }
}

/// Edición de la configuración global o de las variables de entorno de un profile.
///
/// Construirla solo lee. Solo [`save`](Self::save) escribe.
#[derive(Debug)]
pub struct Configuration {
    layout: Layout,
    profile: Option<String>,
    path: PathBuf,
    original: Option<Vec<u8>>,
    values: Values,
}

impl Configuration {
    /// Abre `config.properties`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si no se puede leer o no es UTF-8.
    pub fn global(layout: &Layout) -> Result<Self> {
        let path = layout.config_file();
        let original = read_optional(&path)?;
        let values = parse(decode_text(&path, original.as_deref())?);
        Ok(Self {
            layout: layout.clone(),
            profile: None,
            path,
            original,
            values,
        })
    }

    /// Abre el bloque `env` de `profiles/<profile>.json`.
    ///
    /// # Errors
    ///
    /// Los de [`identifier`]; `No existe el profile …` si falta el archivo; el error
    /// de JSON si es inválido; y los de [`validate_env`] si `env` no es válido.
    pub fn profile(layout: &Layout, profile: &str) -> Result<Self> {
        let name = identifier(profile)?;
        let path = layout.profiles().join(format!("{name}.json"));
        let Some(original) = read_optional(&path)? else {
            return bail(Message::new("No existe el profile {p1}").arg("p1", profile));
        };
        let data: Value = read_json(&path)?;
        let values = profile_env(&data)?;
        Ok(Self {
            layout: layout.clone(),
            profile: Some(name),
            path,
            original: Some(original),
            values,
        })
    }

    /// Archivo que se edita.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Bytes leídos al abrir (o tras el último guardado); `None` si el archivo no existía.
    pub fn original(&self) -> Option<&[u8]> {
        self.original.as_deref()
    }

    /// Valores actuales: todas las claves de `config.properties`, o las variables de
    /// `env` del profile.
    pub fn values(&self) -> &Values {
        &self.values
    }

    /// Valida y guarda `pairs` como el conjunto completo de valores.
    ///
    /// En modo global, las claves de `config.properties` que no estén en `pairs` se
    /// borran (también las desconocidas: el llamador debe devolverlas). En modo
    /// profile solo se reemplaza `env` en el JSON y `config.properties` no se toca.
    ///
    /// # Errors
    ///
    /// Los mensajes de validación del original; `La configuración cambió fuera del
    /// diálogo. Ciérralo y vuelve a abrirlo.` si el archivo cambió desde la apertura;
    /// `Otra instancia está modificando MITO. Espera a que termine.` si hay otra
    /// mutación en curso; y los errores de E/S. Si falla, el archivo no cambia.
    pub fn save<K: AsRef<str>, V: AsRef<str>>(
        &mut self,
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()> {
        let values = if self.profile.is_some() {
            validated_profile(pairs)?
        } else {
            validated(pairs)?
        };
        let _lock = MutationLock::mutation(&self.layout, Duration::ZERO)?;
        if read_optional(&self.path)? != self.original {
            return bail(Message::new(
                "La configuración cambió fuera del diálogo. Ciérralo y vuelve a abrirlo.",
            ));
        }
        if self.profile.is_some() {
            let mut data: Value = read_json(&self.path)?;
            let object = data.as_object_mut().ok_or_else(|| {
                Error::domain(Message::new(
                    "env debe ser un objeto de variables de entorno",
                ))
            })?;
            let env: Map<String, Value> = values
                .iter()
                .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
                .collect();
            object.insert("env".to_owned(), Value::Object(env));
            write_json(&self.path, &data)?;
        } else {
            let original = decode_text(&self.path, self.original.as_deref())?;
            write_atomic(&self.path, render(original, &values).as_bytes())?;
        }
        self.original = read_optional(&self.path)?;
        self.values = values;
        Ok(())
    }
}

/// Extrae `env` de un profile (`engine.profile(...)['env']`).
fn profile_env(data: &Value) -> Result<Values> {
    let invalid = || {
        Error::domain(Message::new(
            "env debe ser un objeto de variables de entorno",
        ))
    };
    let mut values = Values::new();
    match data.get("env") {
        None => {}
        Some(Value::Object(env)) => {
            for (key, value) in env {
                let value = value.as_str().ok_or_else(|| {
                    Error::domain(Message::new("Variable de entorno no válida: ").with_suffix(key))
                })?;
                values.insert(key.as_str(), value);
            }
        }
        Some(_) => return Err(invalid()),
    }
    validate_env(values.iter())?;
    Ok(values)
}

/// `validated(..., profile=True)`: los pares son variables de entorno; las claves se
/// recortan y los valores se conservan tal cual.
fn validated_profile<K: AsRef<str>, V: AsRef<str>>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> Result<Values> {
    let mut result = Values::new();
    for (key, value) in pairs {
        let key = text::py_strip(key.as_ref());
        let value = value.as_ref();
        if key.is_empty() || key.contains(['=', '\r', '\n', '\0']) || key.starts_with(['#', '!']) {
            return bail(Message::new("Introduce una clave válida"));
        }
        if value.contains(['\r', '\n', '\0']) {
            return bail(Message::new("{p0}: el valor debe ocupar una línea").arg("p0", key));
        }
        if result.contains(key) {
            return bail(Message::new("Clave repetida: {p1}").arg("p1", key));
        }
        result.insert(key, value);
    }
    validate_env(result.iter())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instance() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        (dir, layout)
    }

    fn save_profile(layout: &Layout, name: &str, data: &Value) {
        write_json(&layout.profiles().join(format!("{name}.json")), data).unwrap();
    }

    #[test]
    fn guardar_en_global_conserva_comentarios_claves_desconocidas_e_iguales() {
        let (_dir, layout) = instance();
        let path = layout.config_file();
        fs::write(
            &path,
            "# My configuration\n\nbucket.main=C:/catalog\nenv.OLD=old\nunknown.setting=keep\n",
        )
        .unwrap();
        let mut settings = Configuration::global(&layout).unwrap();
        assert_eq!(fs::read(&path).unwrap(), settings.original().unwrap());
        let mut values = settings.values().clone();
        values.remove("env.OLD");
        values.insert("env.TOKEN", "a=b");
        values.insert("enable_proxy_authentication", "true");
        settings.save(values.iter()).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(parse(&written), values);
        assert!(written.starts_with("# My configuration\n\n"));
        assert_eq!(
            read_global(&layout).unwrap().env(),
            [("TOKEN".into(), "a=b".into())]
        );
        assert_eq!(settings.values(), &values);
        assert_eq!(settings.original(), Some(written.as_bytes()));
    }

    #[test]
    fn guardar_sin_archivo_previo_lo_crea() {
        let (_dir, layout) = instance();
        let mut settings = Configuration::global(&layout).unwrap();
        assert!(settings.original().is_none());
        settings.save([("terminal.defaultShell", "mito")]).unwrap();
        assert_eq!(
            fs::read_to_string(layout.config_file()).unwrap(),
            "terminal.defaultShell=mito\n"
        );
    }

    #[test]
    fn el_bom_se_pierde_al_reescribir() {
        let (_dir, layout) = instance();
        fs::write(layout.config_file(), b"\xEF\xBB\xBFmito.language=english\n").unwrap();
        let mut settings = Configuration::global(&layout).unwrap();
        assert_eq!(settings.values().get("mito.language"), Some("english"));
        let values = settings.values().clone();
        settings.save(values.iter()).unwrap();
        assert_eq!(
            fs::read(layout.config_file()).unwrap(),
            b"mito.language=english\n"
        );
    }

    #[test]
    fn las_claves_posthog_se_conservan_sin_efecto() {
        let (_dir, layout) = instance();
        fs::write(
            layout.config_file(),
            "posthog.enabled=true\nposthog.host=https://eu.i.posthog.com\nposthog.api_key=x\n",
        )
        .unwrap();
        let mut settings = Configuration::global(&layout).unwrap();
        let mut values = settings.values().clone();
        values.insert("terminal.defaultShell", "mito");
        settings.save(values.iter()).unwrap();
        let written = fs::read_to_string(layout.config_file()).unwrap();
        assert!(written.starts_with(
            "posthog.enabled=true\nposthog.host=https://eu.i.posthog.com\nposthog.api_key=x\n"
        ));
        assert!(lookup("posthog.enabled").is_none());
    }

    #[test]
    fn en_profile_solo_cambia_env_y_prevalece_sobre_el_global() {
        let (_dir, layout) = instance();
        save_profile(
            &layout,
            "test",
            &json!({"workspace": "work", "apps": [], "env": {"OLD": "old"}}),
        );
        fs::write(layout.config_file(), "env.CUSTOM=global\n").unwrap();
        let mut settings = Configuration::profile(&layout, "test").unwrap();
        assert_eq!(settings.values().get("OLD"), Some("old"));
        settings.save([("CUSTOM", "profile")]).unwrap();
        let after: Value = read_json(&layout.profiles().join("test.json")).unwrap();
        assert_eq!(
            after,
            json!({"workspace": "work", "apps": [], "env": {"CUSTOM": "profile"}})
        );
        assert_eq!(
            fs::read_to_string(layout.config_file()).unwrap(),
            "env.CUSTOM=global\n"
        );
        assert_eq!(settings.values().get("CUSTOM"), Some("profile"));
    }

    #[test]
    fn el_profile_conserva_sus_otros_campos_y_el_orden() {
        let (_dir, layout) = instance();
        save_profile(
            &layout,
            "p",
            &json!({"workspace": "w", "env": {}, "apps": ["a"], "extra": 1}),
        );
        let mut settings = Configuration::profile(&layout, "p").unwrap();
        settings.save([("X", "  con espacios  ")]).unwrap();
        let text = fs::read_to_string(layout.profiles().join("p.json")).unwrap();
        assert!(text.find("workspace").unwrap() < text.find("env").unwrap());
        assert!(text.find("env").unwrap() < text.find("apps").unwrap());
        assert!(text.contains("\"X\": \"  con espacios  \""));
    }

    #[test]
    fn profile_inexistente_o_invalido() {
        let (_dir, layout) = instance();
        assert_eq!(
            Configuration::profile(&layout, "nada")
                .unwrap_err()
                .to_string(),
            "No existe el profile nada"
        );
        assert!(Configuration::profile(&layout, "../x").is_err());
        save_profile(&layout, "mal", &json!({"env": []}));
        assert!(Configuration::profile(&layout, "mal").is_err());
        save_profile(&layout, "mal2", &json!({"env": {"PATH": "x"}}));
        assert_eq!(
            Configuration::profile(&layout, "mal2")
                .unwrap_err()
                .to_string(),
            "Variable reservada por MITO: PATH"
        );
        save_profile(&layout, "mal3", &json!({"env": {"A": 1}}));
        assert!(Configuration::profile(&layout, "mal3").is_err());
    }

    #[test]
    fn los_valores_invalidos_dejan_el_archivo_sin_cambios() {
        let (_dir, layout) = instance();
        fs::write(layout.config_file(), "# original\n").unwrap();
        let cases: Vec<Vec<(&str, &str)>> = vec![
            vec![("enable_proxy_authentication", "maybe")],
            vec![("env.PATH", "bad")],
            vec![("env.X", "one"), ("env.x", "two")],
            vec![("env.X", "a\nb")],
            vec![("network.timeout_seconds", "nan")],
            vec![("network.check_status", "999")],
            vec![("http_proxy", "http://user:password@example.org")],
            vec![("shortcut.badgeColor", "blanco")],
            vec![("terminal.defaultShell", "bash")],
        ];
        for pairs in cases {
            let result = Configuration::global(&layout).unwrap().save(pairs.clone());
            assert!(result.is_err(), "{pairs:?}");
            assert_eq!(
                fs::read_to_string(layout.config_file()).unwrap(),
                "# original\n"
            );
        }
    }

    #[test]
    fn una_edicion_concurrente_no_se_sobrescribe() {
        let (_dir, layout) = instance();
        fs::write(layout.config_file(), "env.X=one\n").unwrap();
        let mut settings = Configuration::global(&layout).unwrap();
        fs::write(layout.config_file(), "env.X=external\n").unwrap();
        let error = settings.save([("env.X", "mine")]).unwrap_err();
        assert!(error.to_string().contains("cambió fuera"));
        assert_eq!(
            fs::read_to_string(layout.config_file()).unwrap(),
            "env.X=external\n"
        );
    }

    #[test]
    fn crear_el_archivo_externamente_tambien_es_un_cambio() {
        let (_dir, layout) = instance();
        let mut settings = Configuration::global(&layout).unwrap();
        fs::write(layout.config_file(), "env.X=external\n").unwrap();
        assert!(settings.save([("env.X", "mine")]).is_err());
    }

    #[test]
    fn guardar_espera_a_que_nadie_mute_la_instancia() {
        let (_dir, layout) = instance();
        let _held = MutationLock::mutation(&layout, Duration::ZERO).unwrap();
        let mut settings = Configuration::global(&layout).unwrap();
        let error = settings.save([("env.X", "1")]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Otra instancia está modificando MITO. Espera a que termine."
        );
        assert!(!layout.config_file().exists());
    }

    #[test]
    fn un_archivo_que_no_es_utf8_da_error_de_lectura() {
        let (_dir, layout) = instance();
        fs::write(layout.config_file(), b"\xFF\xFE").unwrap();
        assert!(matches!(
            Configuration::global(&layout),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn valores_efectivos_con_defecto() {
        let values = parse("terminal.defaultShell=mito\n");
        assert_eq!(values.effective("terminal.defaultShell"), Some("mito"));
        assert_eq!(values.effective("network.timeout_seconds"), Some("4"));
        assert_eq!(values.effective("mito.language"), Some("system"));
        assert_eq!(values.effective("desconocida"), None);
    }
}
