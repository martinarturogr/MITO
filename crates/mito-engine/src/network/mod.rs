//! Red de la instancia (`network.py` del original): comprobación de Internet, autenticación
//! en el portal cautivo, diagnóstico de la ruta y guardado de los cambios propuestos.
//!
//! Contratos: `docs/spec/modules/network.md` y `network_api.md`. Este módulo lee
//! `config.properties`; la capa HTTP sin configuración es [`mito_transfer::network`].
//!
//! Usuario y contraseña del portal viven solo en [`Credentials`] y en el cuerpo del POST:
//! nunca se escriben en disco, en el log ni en un `Debug`. Los registros de
//! `network.authenticate` llevan el origen del portal y estados, jamás una URL completa
//! (los portales meten un token de sesión en ella).

mod config;
mod diagnose;
mod intercept;

use mito_core::config::{Values, read_global};
use mito_core::diagnostics::{LogLevel, write};
use mito_core::{Layout, Message};
use mito_transfer::network::{
    Failure, Proxies, Session, SessionOptions, origin, portal_forms, process_environment,
    redirect_location, same_origin, urljoin, validate_url,
};
use serde_json::json;
use zeroize::{Zeroize, Zeroizing};

use crate::environment::Env;
use crate::error::{Error, Result};
use crate::extensions::Network as NetworkExtension;

pub use config::{NetworkConfig, poll_interval, proxy_environment};
pub use diagnose::{Diagnosis, DiagnosisKind, apply, diagnose};
pub use intercept::{intercept, intercept_with};

/// Intentos de seguir las redirecciones del portal.
const HOPS: usize = 5;

/// Usuario y contraseña del portal. La contraseña se borra de memoria al soltarse y nunca
/// se muestra: ni [`Debug`] ni ningún mensaje la incluyen.
pub struct Credentials {
    /// Nombre de usuario.
    pub username: String,
    /// Contraseña.
    pub password: Zeroizing<String>,
}

impl Credentials {
    /// Credenciales con la contraseña protegida.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: Zeroizing::new(password.into()),
        }
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").finish_non_exhaustive()
    }
}

/// Extensión [`NetworkExtension`] del motor: lee `config.properties` en cada llamada y
/// aplica sus claves de proxy al entorno de los hooks y de las aplicaciones.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigNetwork;

impl NetworkExtension for ConfigNetwork {
    fn proxy_environment(&self, layout: &Layout, env: &mut Env) -> Result<()> {
        proxy_environment(&read_global(layout)?, env)
    }
}

/// Lo que [`diagnose`] y la interfaz consultan de la conectividad (inyectable en tests).
pub trait Connectivity {
    /// ¿Responde `network.check_url` con el estado esperado? Con `log` se registra el
    /// fallo de red como `network.check`.
    fn check(&self, log: bool) -> bool;
    /// ¿Hay alguna conexión de red local (no implica Internet)?
    fn local_network(&self) -> bool;
}

/// Proxies del proceso para `layout`: el sistema (`system`, o el entorno y el registro si
/// es `None`) sustituido por las claves de proxy de `config.properties` (`opener` con
/// `environment=None`). Los usan las descargas.
///
/// # Errors
///
/// Los de leer `config.properties` y [`proxy_environment`].
pub fn proxies_for_root(layout: &Layout, system: Option<&Proxies>) -> Result<Proxies> {
    let process = process_environment();
    let mut env: Env = process.iter().cloned().collect();
    proxy_environment(&read_global(layout)?, &mut env)?;
    let system = system.cloned().unwrap_or_else(|| Proxies::system(&process));
    Ok(system.overridden(&env_pairs(&env)))
}

fn env_pairs(env: &Env) -> Vec<(String, String)> {
    env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Comprobador de Internet y autenticador del portal de una instancia (`Network`).
#[derive(Debug)]
pub struct Network {
    layout: Layout,
    config: NetworkConfig,
    values: Values,
    env: Env,
    /// Proxies del sistema; `None` los lee en cada uso (`getproxies`).
    system: Option<Proxies>,
}

impl Network {
    /// `Network(root, environment)`: lee `config.properties`. Con `env` `None` usa el
    /// entorno del proceso con [`proxy_environment`] aplicado.
    ///
    /// # Errors
    ///
    /// Los de leer `config.properties` (no es UTF-8, no se puede leer), de
    /// [`NetworkConfig::from_values`] y de [`proxy_environment`].
    pub fn open(layout: &Layout, env: Option<&Env>) -> Result<Self> {
        Self::build(layout, env, None)
    }

    /// Igual que [`Network::open`], con los proxies del sistema fijados en lugar de leídos
    /// del entorno y del registro en cada uso: para pruebas y llamadores que ya los
    /// resolvieron.
    ///
    /// # Errors
    ///
    /// Los de [`Network::open`].
    pub fn with_system(layout: &Layout, env: Option<&Env>, system: Proxies) -> Result<Self> {
        Self::build(layout, env, Some(system))
    }

    fn build(layout: &Layout, env: Option<&Env>, system: Option<Proxies>) -> Result<Self> {
        let values = read_global(layout)?;
        let config = NetworkConfig::from_values(&values)?;
        let env = if let Some(env) = env {
            env.clone()
        } else {
            let mut env: Env = process_environment().into_iter().collect();
            proxy_environment(&values, &mut env)?;
            env
        };
        Ok(Self {
            layout: layout.clone(),
            config,
            values,
            env,
            system,
        })
    }

    /// Claves de red validadas.
    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    /// Contenido de `config.properties` al abrir.
    pub fn values(&self) -> &Values {
        &self.values
    }

    /// ¿Está activada la autenticación del portal (y hay URL)?
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// `opener`: proxies del sistema sustituidos por el entorno.
    pub fn proxies(&self) -> Proxies {
        let system = self
            .system
            .clone()
            .unwrap_or_else(|| Proxies::system(&process_environment()));
        system.overridden(&env_pairs(&self.env))
    }

    fn session(&self, cookies: bool) -> std::result::Result<Session, Failure> {
        Session::new(&SessionOptions {
            proxies: self.proxies(),
            timeout: self.config.timeout,
            cookies,
        })
    }

    /// Registro de `network.authenticate`: solo el origen del portal y estados.
    fn log(&self, text: &str, status: Option<u16>) {
        let (scheme, netloc) = origin(&self.config.auth_url);
        let mut details = vec![("portal", json!([scheme, netloc]))];
        if let Some(status) = status {
            details.push(("status", json!(status)));
        }
        write(
            &self.layout,
            LogLevel::Warning,
            "network.authenticate",
            text,
            &details,
        );
    }

    /// `redirect`: destino de la redirección de `url`, o `""`.
    fn redirect(session: &Session, url: &str) -> String {
        match session.get(url) {
            Ok(reply) => redirect_location(url, reply.status, reply.location.as_deref()),
            Err(_) => String::new(),
        }
    }

    /// `portal_page`: URL **actual** del portal. Los portales emiten un token por sesión,
    /// así que la guardada puede haber caducado: se repite la primera petición del
    /// navegador (la URL de comprobación por HTTP) y se usa una redirección al origen del
    /// portal; si no la hay, la URL guardada.
    fn portal_page(&self, session: &Session) -> String {
        let mut url = plain_http(&self.config.check_url);
        for _ in 0..HOPS {
            url = Self::redirect(session, &url);
            if url.is_empty() {
                break;
            }
            if same_origin(&url, &self.config.auth_url) {
                return url;
            }
        }
        self.config.auth_url.clone()
    }

    /// `authenticate`: entra en el portal con `credentials` y comprueba que haya Internet.
    ///
    /// `Ok(false)` si no está activada, faltan datos o el portal no acepta el acceso (el
    /// motivo queda en el log, sin credenciales ni URL completa). Cookies y credenciales
    /// viven solo durante esta secuencia.
    ///
    /// # Errors
    ///
    /// Solo los errores de configuración del original: el formulario apunta a otro origen,
    /// el POST requiere HTTPS o la URL no es válida.
    pub fn authenticate(&self, credentials: &Credentials) -> Result<bool> {
        if !self.config.enabled
            || credentials.username.is_empty()
            || credentials.password.is_empty()
        {
            return Ok(false);
        }
        let session = match self.session(true) {
            Ok(session) => session,
            Err(failure) => {
                self.log(&failure.message, None);
                return Ok(false);
            }
        };
        let mut url = self.portal_page(&session);
        let mut page = None;
        for _ in 0..HOPS {
            match session.get(&url) {
                Ok(reply) if reply.is_success() => {
                    page = Some(reply.body);
                    break;
                }
                Ok(reply) => {
                    let target = redirect_location(&url, reply.status, reply.location.as_deref());
                    if target.is_empty() || !same_origin(&target, &self.config.auth_url) {
                        self.log("Portal page unavailable", Some(reply.status));
                        return Ok(false);
                    }
                    url = target;
                }
                Err(failure) => {
                    self.log(&failure.message, None);
                    return Ok(false);
                }
            }
        }
        let Some(body) = page else {
            self.log("Portal page redirects too many times", None);
            return Ok(false);
        };
        let html = String::from_utf8_lossy(&body);
        let forms = portal_forms(&html, &self.config.password_field);
        let (action, mut fields) = if let Some(form) = forms.into_iter().next() {
            (urljoin(&url, &form.action), form.fields)
        } else {
            self.log(
                "Portal page has no password form; posting to the page itself",
                None,
            );
            (url, Vec::new())
        };
        let parsed = validate_url(&action)?;
        validate_url(&self.config.auth_url)?;
        // F6: el origen se compara como en el resto del módulo (esquema y host en
        // minúsculas, puerto por defecto implícito).
        if !same_origin(&action, &self.config.auth_url) {
            return Err(Error::domain(Message::new(
                "El formulario apunta a otro origen; configura su URL explícitamente",
            )));
        }
        if parsed.scheme() != "https" && self.config.require_https {
            return Err(Error::domain(Message::new(
                "El POST requiere HTTPS; configura proxy_authentication_require_https=false para un portal HTTP",
            )));
        }
        for (name, value) in &self.config.form_fields {
            set_field(&mut fields, name, value);
        }
        set_field(
            &mut fields,
            &self.config.username_field,
            &credentials.username,
        );
        set_field(
            &mut fields,
            &self.config.password_field,
            &credentials.password,
        );
        let reply = session.post_form(&action, &fields);
        // La copia de la contraseña en los campos no sobrevive al POST.
        for (_, value) in &mut fields {
            if value.as_str() == credentials.password.as_str() {
                value.zeroize();
            }
        }
        let reply = match reply {
            Ok(reply) => reply,
            Err(failure) => {
                self.log(&failure.message, None);
                return Ok(false);
            }
        };
        let status = reply.status;
        if !(reply.is_success() || matches!(status, 301..=303)) {
            self.log("Portal rejected the login POST", Some(status));
            return Ok(false);
        }
        // Comprobación con un cliente nuevo: los hijos no heredan las cookies del portal.
        if self.check(true) {
            return Ok(true);
        }
        self.log(
            "Internet still unavailable after the login POST",
            Some(status),
        );
        Ok(false)
    }
}

impl Connectivity for Network {
    /// `check`: sin cookies y sin seguir redirecciones. Un estado distinto del esperado
    /// (un 302 del portal, un 407, un 500) es `false` sin registro; un fallo de red se
    /// registra como `network.check` solo con `log`.
    fn check(&self, log: bool) -> bool {
        let outcome = self
            .session(false)
            .and_then(|session| session.get(&self.config.check_url));
        match outcome {
            Ok(reply) => reply.is_success() && reply.status == self.config.expected,
            Err(failure) => {
                if log {
                    write(
                        &self.layout,
                        LogLevel::Warning,
                        "network.check",
                        &failure.message,
                        &[],
                    );
                }
                false
            }
        }
    }

    fn local_network(&self) -> bool {
        mito_win::network::local_network()
    }
}

/// `dict.update` de un campo del POST: conserva la posición del existente.
fn set_field(fields: &mut Vec<(String, String)>, name: &str, value: &str) {
    match fields.iter_mut().find(|(known, _)| known == name) {
        Some(slot) => value.clone_into(&mut slot.1),
        None => fields.push((name.to_owned(), value.to_owned())),
    }
}

/// La URL con esquema `http` y sin fragmento (`urlunsplit(('http', netloc, path, query, ''))`).
pub(crate) fn plain_http(url: &str) -> String {
    let url = url.trim_start();
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split('#').next().unwrap_or(rest);
    format!("http://{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_cambia_el_esquema_y_quita_el_fragmento() {
        let table = [
            (
                "https://www.gstatic.com/generate_204",
                "http://www.gstatic.com/generate_204",
            ),
            ("http://h:81/a?b=1#frag", "http://h:81/a?b=1"),
            ("HTTPS://H/p?q", "http://H/p?q"),
            ("  https://h", "http://h"),
        ];
        for (input, expected) in table {
            assert_eq!(plain_http(input), expected, "{input}");
        }
    }

    #[test]
    fn credentials_nunca_muestra_nada_en_debug() {
        let credentials = Credentials::new("usuario-secreto", "clave-secreta");
        let text = format!("{credentials:?} {credentials:#?}");
        assert!(!text.contains("clave-secreta"), "{text}");
        assert!(!text.contains("usuario-secreto"), "{text}");
    }

    #[test]
    fn set_field_conserva_la_posicion() {
        let mut fields = vec![
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
        ];
        set_field(&mut fields, "a", "9");
        set_field(&mut fields, "c", "3");
        let expected: Vec<(String, String)> = [("a", "9"), ("b", "2"), ("c", "3")]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        assert_eq!(fields, expected);
    }
}
