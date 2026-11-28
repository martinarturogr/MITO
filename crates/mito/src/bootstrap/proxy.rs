//! Proxy del bootstrap (`Initialize-YitoProxy` de `bootstrap.ps1`, sección 5.7 del
//! contrato del bootstrap).
//!
//! Antes de descargar las herramientas se decide por dónde sale el tráfico: manda
//! `config.properties`; sin configuración se usa `HTTPS_PROXY` o el proxy de Windows
//! (manual, PAC o WPAD). Se sondea la ruta, se explica el resultado, se ofrece guardar
//! la configuración detectada y, si hay un portal cautivo configurado, se pide el login
//! en consola. Con `--approved` nunca se pregunta.
//!
//! Diferencias con el original: el proxy no guardado no se exporta a variables de
//! entorno del proceso, sino que se devuelve para las descargas de esta preparación; y
//! el mensaje de «windows» dice que MITO (no Python) no puede usar la sesión de Windows.
//! La contraseña del portal solo vive en [`Credentials`] y en la consola: nunca llega al
//! log ni a `config.properties`.

use std::fs;
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use mito_core::Layout;
use mito_core::config::{Values, read_global, render};
use mito_core::diagnostics::{self, LogLevel};
use mito_engine::Env;
use mito_engine::network::{Credentials, Network, apply, proxies_for_root};
use mito_transfer::network::{
    Probe, ProbeKind, Proxies, SystemProxy, no_proxy_from_override, portal_fields, probe,
    probe_with_windows_credentials, process_environment, system_proxy, urljoin, validate_url,
};
use serde_json::{Value, json};

use super::OPERATION;
use crate::console::{Color, Console};

/// Tiempo máximo de cada sonda (`Timeout` de `Invoke-YitoHttp`).
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// Intentos del login del portal.
const LOGIN_ATTEMPTS: usize = 3;
/// `network.check_url` por defecto.
const DEFAULT_CHECK_URL: &str = "https://www.gstatic.com/generate_204";

/// Pares clave-valor propuestos para `config.properties`.
type Fields = Vec<(String, String)>;

/// Lo que el bootstrap pide al exterior: entorno, proxy de Windows, sondas y PowerShell.
/// Los tests lo sustituyen para no tocar el registro, PowerShell ni Internet.
pub struct Services<'a> {
    /// Entorno del proceso (`HTTPS_PROXY`, `NO_PROXY`).
    pub env: Vec<(String, String)>,
    /// Proxy de Windows para un destino (`Get-YitoSystemProxy`).
    pub system_proxy: &'a dyn Fn(&str) -> Option<SystemProxy>,
    /// Sonda de `url` por `proxy` (`Invoke-YitoHttp`).
    pub probe: &'a dyn Fn(&str, &str, Duration) -> Probe,
    /// Sonda con las credenciales de Windows (`-ProxyUseDefaultCredentials`).
    pub windows_probe: &'a dyn Fn(&str, &str, Duration, u16) -> bool,
    /// Nombres de campo no predeterminados del portal.
    pub portal_fields: &'a dyn Fn(&str, &str, Duration) -> Fields,
    /// Proxies del sistema fijos para la sesión del portal y las descargas (`None`: los
    /// del entorno y el registro).
    pub system_proxies: Option<Proxies>,
}

impl Services<'static> {
    /// Servicios reales.
    pub fn real() -> Self {
        Self {
            env: process_environment(),
            system_proxy: &system_proxy,
            probe: &probe,
            windows_probe: &probe_with_windows_credentials,
            portal_fields: &portal_fields,
            system_proxies: None,
        }
    }
}

/// Resultado de la sonda (`Get-YitoProxyKind` más `windows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ok,
    Windows,
    Auth,
    Portal,
    Blocked,
    Unreachable,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Windows => "windows",
            Self::Auth => "auth",
            Self::Portal => "portal",
            Self::Blocked => "blocked",
            Self::Unreachable => "unreachable",
        }
    }
}

impl From<ProbeKind> for Kind {
    fn from(kind: ProbeKind) -> Self {
        match kind {
            ProbeKind::Ok => Self::Ok,
            ProbeKind::Auth => Self::Auth,
            ProbeKind::Portal => Self::Portal,
            ProbeKind::Unreachable => Self::Unreachable,
            ProbeKind::Blocked => Self::Blocked,
        }
    }
}

/// Un proxy elegido y de dónde salió.
struct Chosen {
    url: String,
    source: String,
    no_proxy: String,
}

/// Escribe un registro del bootstrap con su fase.
fn note(layout: &Layout, level: LogLevel, phase: &str, text: &str, extra: &[(&str, Value)]) {
    let mut details = vec![("phase", json!(phase))];
    details.extend(extra.iter().map(|(key, value)| (*key, value.clone())));
    diagnostics::write(layout, level, OPERATION, text, &details);
}

/// `Test-YitoHttpUrl`: URL HTTP/HTTPS con host y sin credenciales.
fn check_http_url(url: &str, label: &str) -> anyhow::Result<()> {
    validate_url(url)
        .map(|_| ())
        .map_err(|_| anyhow!("{label}: usa una URL HTTP/HTTPS sin credenciales"))
}

/// Valor no vacío de `key`.
fn setting<'a>(values: &'a Values, key: &str) -> Option<&'a str> {
    values.get(key).filter(|value| !value.is_empty())
}

/// Variable de entorno sin distinguir mayúsculas (Windows).
fn env_var<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Origen que se muestra para un proxy del sistema (`Get-YitoSystemProxy`).
fn system_source(source: &str) -> String {
    match source {
        "manual" => "configuración manual de Windows".to_owned(),
        // El script del sistema no devuelve la URL del PAC.
        "pac" => "script PAC".to_owned(),
        _ => "autodetección WPAD".to_owned(),
    }
}

/// `Read-YitoYes`: respuesta vacía, el valor por defecto; sin entrada, `false`.
fn read_yes(console: &Console, prompt: &str, default: bool) -> bool {
    let Some(answer) = console.read_line(&format!("{prompt}: ")) else {
        return false;
    };
    let answer = answer.trim().to_lowercase();
    if answer.is_empty() {
        return default;
    }
    matches!(answer.as_str(), "s" | "si" | "sí" | "y" | "yes")
}

/// Configuración vigente de la instancia: `config.properties` ilegible cuenta como vacío
/// (el original ignoraba el fallo).
fn settings_of(layout: &Layout) -> Values {
    read_global(layout).unwrap_or_default()
}

/// Proxies de las descargas cuando no se detecta nada nuevo: los de la configuración, el
/// entorno y el sistema.
fn configured_proxies(layout: &Layout, services: &Services<'_>) -> anyhow::Result<Proxies> {
    Ok(proxies_for_root(layout, services.system_proxies.as_ref())?)
}

/// Proxy elegido, o las descargas por la ruta normal (sin proxy nuevo).
enum Selection {
    /// Sin proxy que sondear: los proxies de las descargas ya están decididos.
    Direct(Proxies),
    /// Proxy que se sondea y se informa.
    Proxy(Chosen),
}

/// `network.check_url` y `network.check_status` validados.
fn check_options(settings: &Values) -> anyhow::Result<(String, u16)> {
    let check_url = setting(settings, "network.check_url")
        .unwrap_or(DEFAULT_CHECK_URL)
        .to_owned();
    let expected = match setting(settings, "network.check_status") {
        None => 204,
        Some(text) => text
            .parse::<u16>()
            .ok()
            .filter(|code| (100..=599).contains(code))
            .ok_or_else(|| {
                anyhow!("network.check_status: introduce un código HTTP entre 100 y 599")
            })?,
    };
    check_http_url(&check_url, "network.check_url")?;
    Ok((check_url, expected))
}

/// Elige el proxy: manda `config.properties`; sin configuración, el entorno y Windows.
fn select(
    layout: &Layout,
    target: &str,
    console: &Console,
    settings: &Values,
    configured: bool,
    services: &Services<'_>,
) -> anyhow::Result<Selection> {
    if configured {
        let Some(proxy) = settings.get("https_proxy") else {
            return Ok(Selection::Direct(configured_proxies(layout, services)?));
        };
        if proxy.is_empty() {
            // Valor vacío: sin proxy, también frente al heredado de Windows.
            note(
                layout,
                LogLevel::Info,
                "proxy",
                "Proxy disabled by config.properties",
                &[],
            );
            return Ok(Selection::Direct(configured_proxies(layout, services)?));
        }
        check_http_url(proxy, "Proxy")?;
        return Ok(Selection::Proxy(Chosen {
            url: proxy.to_owned(),
            source: "config.properties".to_owned(),
            no_proxy: settings.get("no_proxy").unwrap_or_default().to_owned(),
        }));
    }
    let Some(found) = detect(layout, target, services) else {
        note(
            layout,
            LogLevel::Info,
            "proxy",
            "No system proxy detected; direct connection",
            &[],
        );
        return Ok(Selection::Direct(configured_proxies(layout, services)?));
    };
    console.colored_line(
        format!("MITO: proxy detectado: {} ({})", found.url, found.source),
        Color::Cyan,
    );
    Ok(Selection::Proxy(found))
}

/// `Initialize-YitoProxy`: decide el proxy de las descargas, informa y, si procede,
/// guarda la configuración y pide el login del portal.
///
/// `target` es la URL con la que se consulta el proxy del sistema (la primera descarga).
/// Devuelve los proxies que deben usar las descargas de esta preparación.
///
/// # Errors
///
/// Una URL de proxy o de comprobación inválida en la configuración (`Proxy: usa una URL
/// HTTP/HTTPS sin credenciales`).
pub fn initialize(
    layout: &Layout,
    target: &str,
    interactive: bool,
    console: &Console,
    services: &Services<'_>,
) -> anyhow::Result<Proxies> {
    let mut settings = settings_of(layout);
    let (check_url, expected) = check_options(&settings)?;
    let configured = settings.contains("https_proxy") || settings.contains("http_proxy");
    let chosen = match select(layout, target, console, &settings, configured, services)? {
        Selection::Direct(proxies) => return Ok(proxies),
        Selection::Proxy(chosen) => chosen,
    };
    let (kind, probed) = classify(&chosen.url, &check_url, expected, services);
    let portal_url = if kind == Kind::Portal {
        portal_target(&probed)
    } else {
        None
    };
    let kind = if kind == Kind::Portal && portal_url.is_none() {
        Kind::Blocked
    } else {
        kind
    };
    note(
        layout,
        LogLevel::Info,
        "proxy",
        &format!(
            "Proxy {} from {}: {}",
            chosen.url,
            chosen.source,
            kind.name()
        ),
        &[
            ("status", json!(probed.status)),
            ("authenticate", json!(probed.authenticate)),
            ("network_error", json!(probed.error)),
        ],
    );
    report(
        layout,
        console,
        kind,
        &probed,
        portal_url.as_deref().unwrap_or(""),
    );

    let mut saved = configured;
    if !configured {
        let values = proposal(&chosen, portal_url.as_deref(), &settings, services);
        for (key, value) in &values {
            settings.insert(key.as_str(), value.as_str());
        }
        saved = false;
        if interactive {
            saved = offer_save(layout, console, &values);
        }
        if !saved {
            console.line("MITO: se usará el proxy detectado sólo durante esta preparación.");
        }
    }

    let authentication = settings
        .get("enable_proxy_authentication")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        && setting(&settings, "proxy_authentication_url").is_some();
    if interactive && authentication && !matches!(kind, Kind::Ok | Kind::Windows) {
        portal_login(layout, console, &settings, saved, &chosen, services);
    }

    if configured {
        configured_proxies(layout, services)
    } else {
        let mut proxies = Proxies::single(&chosen.url);
        proxies.no_proxy = Some(chosen.no_proxy);
        Ok(proxies)
    }
}

/// Proxy sin configurar: `HTTPS_PROXY` del entorno (si es válido) o el de Windows.
fn detect(layout: &Layout, target: &str, services: &Services<'_>) -> Option<Chosen> {
    if let Some(proxy) = env_var(&services.env, "HTTPS_PROXY").filter(|v| !v.is_empty()) {
        if check_http_url(proxy, "Proxy").is_ok() {
            return Some(Chosen {
                url: proxy.trim_end_matches('/').to_owned(),
                source: "variable de entorno HTTPS_PROXY".to_owned(),
                no_proxy: no_proxy_from_override(env_var(&services.env, "NO_PROXY").unwrap_or("")),
            });
        }
        note(
            layout,
            LogLevel::Warning,
            "proxy",
            "Ignoring HTTPS_PROXY with credentials or invalid URL",
            &[],
        );
    }
    let found = (services.system_proxy)(target)?;
    Some(Chosen {
        url: found.url,
        source: system_source(&found.source),
        no_proxy: found.no_proxy,
    })
}

/// Sonda y clasificación: reintento por HTTP si la URL es HTTPS y no responde bien (un
/// portal solo redirige HTTP) y reintento con las credenciales de Windows ante un 407
/// NTLM/Negotiate. Devuelve el tipo, la sonda que lo decidió y si las credenciales de
/// Windows fueron necesarias.
fn classify(proxy: &str, check_url: &str, expected: u16, services: &Services<'_>) -> (Kind, Probe) {
    let mut result = (services.probe)(check_url, proxy, PROBE_TIMEOUT);
    let mut kind = Kind::from(result.kind(expected));
    if kind != Kind::Ok
        && let Ok(mut plain) = validate_url(check_url)
        && plain.scheme() == "https"
        && plain.set_scheme("http").is_ok()
        && plain.set_port(None).is_ok()
    {
        let second = (services.probe)(plain.as_str(), proxy, PROBE_TIMEOUT);
        let second_kind = Kind::from(second.kind(expected));
        if matches!(second_kind, Kind::Auth | Kind::Portal) {
            result = second;
            kind = second_kind;
        }
    }
    if kind == Kind::Auth {
        let schemes = result.authenticate.to_lowercase();
        if (schemes.contains("negotiate") || schemes.contains("ntlm"))
            && (services.windows_probe)(check_url, proxy, PROBE_TIMEOUT, expected)
        {
            kind = Kind::Windows;
        }
    }
    (kind, result)
}

/// URL del portal: la redirección de la sonda, resuelta y validada.
fn portal_target(probed: &Probe) -> Option<String> {
    let joined = urljoin(&probed.url, &probed.location);
    validate_url(&joined).ok().map(|url| url.to_string())
}

/// Mensaje de consola por tipo.
fn report(layout: &Layout, console: &Console, kind: Kind, probed: &Probe, portal: &str) {
    match kind {
        Kind::Ok => console.line("MITO: el proxy permite el acceso a Internet."),
        Kind::Windows => console.colored_line(
            "MITO: el proxy acepta tu sesión de Windows (NTLM/Kerberos). MITO no puede usarla: algunas descargas podrían fallar con HTTP 407.",
            Color::Yellow,
        ),
        Kind::Auth => console.colored_line(
            format!(
                "MITO: el proxy exige autenticación HTTP 407 ({}), que MITO no implementa.",
                probed.authenticate
            ),
            Color::Yellow,
        ),
        Kind::Portal => console.colored_line(
            format!(
                "MITO: el proxy redirige a un portal de autenticación: {}",
                diagnostics::scrub(layout, portal)
            ),
            Color::Yellow,
        ),
        Kind::Blocked => console.colored_line(
            format!(
                "MITO: la comprobación de Internet a través del proxy respondió HTTP {}.",
                probed.status
            ),
            Color::Yellow,
        ),
        Kind::Unreachable => console.colored_line(
            format!(
                "MITO: no se pudo comprobar Internet a través del proxy ({}).",
                probed.error
            ),
            Color::Yellow,
        ),
    }
}

/// Claves que se proponen guardar, en orden.
fn proposal(
    chosen: &Chosen,
    portal: Option<&str>,
    settings: &Values,
    services: &Services<'_>,
) -> Vec<(String, String)> {
    let mut values = vec![
        ("http_proxy".to_owned(), chosen.url.clone()),
        ("https_proxy".to_owned(), chosen.url.clone()),
        ("no_proxy".to_owned(), chosen.no_proxy.clone()),
    ];
    if let Some(portal) = portal
        && setting(settings, "proxy_authentication_url").is_none()
    {
        values.push(("enable_proxy_authentication".to_owned(), "true".to_owned()));
        values.push(("proxy_authentication_url".to_owned(), portal.to_owned()));
        // Son nombres de campos HTML, no credenciales.
        values.extend((services.portal_fields)(portal, &chosen.url, PROBE_TIMEOUT));
    }
    values
}

/// Muestra las claves y pregunta si se guardan. `true` si quedaron en `config.properties`.
fn offer_save(layout: &Layout, console: &Console, values: &[(String, String)]) -> bool {
    console.line("");
    console.line("Se añadirán estas claves a config.properties:");
    for (key, value) in values {
        console.line(format!("  {key}={}", diagnostics::scrub(layout, value)));
    }
    if values
        .iter()
        .any(|(key, _)| key == "proxy_authentication_url")
    {
        console.line("Tu usuario y contraseña nunca se guardan: MITO los pedirá cuando el portal lo requiera.");
    }
    if !read_yes(console, "¿Guardar esta configuración de red? [S/n]", true) {
        return false;
    }
    match apply(layout, values) {
        Ok(()) => {
            console.line("MITO: configuración de red guardada en config.properties.");
            note(
                layout,
                LogLevel::Info,
                "proxy",
                "Detected proxy saved to config.properties",
                &[],
            );
            true
        }
        Err(error) => {
            let text = error.to_string();
            console.colored_line(
                format!("MITO: no se pudo guardar config.properties: {text}"),
                Color::Yellow,
            );
            note(layout, LogLevel::Warning, "proxy", &text, &[]);
            false
        }
    }
}

/// Login del portal en consola: hasta [`LOGIN_ATTEMPTS`] intentos. Un usuario vacío
/// (o el fin de la entrada) lo omite.
fn portal_login(
    layout: &Layout,
    console: &Console,
    settings: &Values,
    on_disk: bool,
    chosen: &Chosen,
    services: &Services<'_>,
) {
    console.line("");
    console.line(
        "Inicia sesión en el portal de red para continuar. Deja el usuario vacío para omitirlo.",
    );
    for _ in 0..LOGIN_ATTEMPTS {
        let Some(user) = console.read_line("Usuario: ") else {
            break;
        };
        let user = user.trim().to_owned();
        if user.is_empty() {
            break;
        }
        let Some(password) = console.read_secret("Contraseña: ") else {
            break;
        };
        let credentials = Credentials::new(user, password.as_str());
        drop(password);
        let outcome = authenticate(layout, settings, on_disk, chosen, services, &credentials);
        drop(credentials);
        match outcome {
            Err(error) => {
                let text = crate::app::describe(&error);
                console.colored_line(format!("MITO: {text}"), Color::Red);
                note(
                    layout,
                    LogLevel::Warning,
                    "proxy-authentication",
                    &text,
                    &[],
                );
                break;
            }
            Ok(true) => {
                console.colored_line("MITO: autenticación correcta.", Color::Green);
                note(
                    layout,
                    LogLevel::Info,
                    "proxy-authentication",
                    "Portal authentication verified",
                    &[],
                );
                break;
            }
            Ok(false) => {
                console.colored_line(
                    "MITO: no se pudo verificar Internet. Revisa usuario y contraseña.",
                    Color::Yellow,
                );
                note(
                    layout,
                    LogLevel::Warning,
                    "proxy-authentication",
                    "Portal authentication not verified",
                    &[],
                );
            }
        }
    }
}

/// `Network::authenticate` con la configuración y el proxy de esta preparación. Si la
/// configuración no está en disco (el usuario no la guardó), se usa una copia temporal.
fn authenticate(
    layout: &Layout,
    settings: &Values,
    on_disk: bool,
    chosen: &Chosen,
    services: &Services<'_>,
    credentials: &Credentials,
) -> anyhow::Result<bool> {
    let env: Env = [
        ("HTTP_PROXY", chosen.url.as_str()),
        ("HTTPS_PROXY", chosen.url.as_str()),
        ("NO_PROXY", chosen.no_proxy.as_str()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    let scratch;
    let scoped = if on_disk {
        layout.clone()
    } else {
        scratch = tempfile::tempdir().context("no se pudo crear la carpeta temporal")?;
        let path = Layout::new(scratch.path());
        fs::write(path.config_file(), render("", settings))
            .map_err(|error| mito_core::Error::io(path.config_file(), error))?;
        path
    };
    let network = match &services.system_proxies {
        Some(system) => Network::with_system(&scoped, Some(&env), system.clone())?,
        None => Network::open(&scoped, Some(&env))?,
    };
    Ok(network.authenticate(credentials)?)
}

#[cfg(test)]
mod tests;
