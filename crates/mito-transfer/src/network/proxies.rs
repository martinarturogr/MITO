//! Proxies de una petición con la semántica de `urllib` en Windows.
//!
//! Reproduce `getproxies` (entorno y registro), `ProxyHandler.proxy_open`,
//! `proxy_bypass_environment` y `proxy_bypass_registry`. Contrato:
//! `docs/spec/modules/network.md`, secciones 3.7 a 3.9.

use mito_win::registry::{Hive, Key, RawValue};

/// Clave del registro con la configuración de Internet del usuario.
const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// Proxies de una petición (`proxies` de `ProxyHandler`).
///
/// `None` significa «sin entrada para ese esquema» (conexión directa) y `Some("")`
/// «entrada vacía» (directa también, y anula la del sistema).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proxies {
    /// Proxy para `http`.
    pub http: Option<String>,
    /// Proxy para `https`.
    pub https: Option<String>,
    /// Lista `no_proxy` (hosts y sufijos separados por comas).
    pub no_proxy: Option<String>,
    /// `ProxyOverride` del registro, solo si los proxies salieron del registro
    /// (`proxy_bypass_registry`); `None` si salieron del entorno.
    pub registry_override: Option<String>,
    /// `NO_PROXY` del entorno del sistema (`proxy_bypass` de la biblioteca estándar).
    /// Sobrevive a [`Proxies::overridden`]: las exclusiones del sistema se suman al
    /// `no_proxy` de la configuración (F4 del original, se conserva).
    pub system_no_proxy: Option<String>,
}

/// Valores de `HKCU\...\Internet Settings` que usa `urllib`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistrySettings {
    /// `ProxyEnable` distinto de cero.
    pub enable: bool,
    /// `ProxyServer`, si existe.
    pub server: Option<String>,
    /// `ProxyOverride` (vacío si no existe).
    pub proxy_override: String,
}

impl Proxies {
    /// `urllib.request.getproxies()` en Windows: las variables `*_proxy` de `env` o, si no
    /// hay ninguna con valor, el registro de Internet Settings de HKCU (`ProxyEnable`,
    /// `ProxyServer`, `ProxyOverride`).
    pub fn system(env: &[(String, String)]) -> Self {
        Self::system_with(env, &read_registry)
    }

    /// Igual que [`Proxies::system`] con la lectura del registro inyectada (solo se
    /// invoca si `env` no define ningún proxy).
    pub fn system_with(env: &[(String, String)], registry: &dyn Fn() -> RegistrySettings) -> Self {
        let map = environment_map(env);
        if map.is_empty() {
            let settings = registry();
            Self::from_registry_parts(
                settings.enable,
                settings.server.as_deref(),
                &settings.proxy_override,
            )
        } else {
            Self::from_map(&map)
        }
    }

    /// Parte pura de [`Proxies::system`] para el entorno (`getproxies_environment`).
    ///
    /// Como `os.environ` de Windows, los nombres se tratan en mayúsculas (si hay dos que
    /// solo difieren en las mayúsculas gana el último) y un valor vacío no define nada.
    /// Con `REQUEST_METHOD` presente se descarta `http` (CVE-2016-1000110).
    pub fn from_environment(env: &[(String, String)]) -> Self {
        Self::from_map(&environment_map(env))
    }

    fn from_map(map: &[(String, String)]) -> Self {
        let get = |name: &str| {
            map.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let no_proxy = get("no");
        Self {
            http: get("http"),
            https: get("https"),
            system_no_proxy: no_proxy.clone(),
            no_proxy,
            registry_override: None,
        }
    }

    /// Parte pura de [`Proxies::system`] para el registro (`getproxies_registry`):
    /// `ProxyServer` `host:puerto` o `http=…;https=…`, con `http://` por defecto.
    pub fn from_registry(enable: bool, server: &str, proxy_override: &str) -> Self {
        Self::from_registry_parts(enable, Some(server), proxy_override)
    }

    fn from_registry_parts(enable: bool, server: Option<&str>, proxy_override: &str) -> Self {
        let mut proxies = Self::default();
        if enable {
            if let Some(server) = server {
                let (http, https) = parse_proxy_server(server);
                proxies.http = http;
                proxies.https = https;
            }
            if !proxy_override.is_empty() {
                proxies.registry_override = Some(proxy_override.to_owned());
            }
        }
        proxies
    }

    /// `opener`: las claves `http_proxy`, `https_proxy` y `no_proxy` de `env` (sin
    /// distinguir mayúsculas) sustituyen a las del sistema. Un valor vacío se conserva
    /// (`Some("")`): anula el proxy heredado.
    #[must_use]
    pub fn overridden(mut self, env: &[(String, String)]) -> Self {
        for (key, value) in env {
            match key.to_lowercase().as_str() {
                "http_proxy" => self.http = Some(value.clone()),
                "https_proxy" => self.https = Some(value.clone()),
                "no_proxy" => self.no_proxy = Some(value.clone()),
                _ => {}
            }
        }
        self
    }

    /// Ruta de una sonda (`probe`): `""` directo; otro valor, para `http` y `https`.
    /// No aplica ninguna exclusión del sistema (F7 del original, corregido).
    pub fn single(proxy: &str) -> Self {
        Self {
            http: Some(proxy.to_owned()),
            https: Some(proxy.to_owned()),
            ..Self::default()
        }
    }

    /// Proxy que usa `url`, ya como URL `http://…` (o `socks…://`), o `None` si va
    /// directa: entrada vacía o ausente, o bypass de `no_proxy`
    /// (`proxy_bypass_environment`) o del sistema (`proxy_bypass`).
    pub fn proxy_for(&self, url: &reqwest::Url) -> Option<String> {
        let entry = match url.scheme() {
            "http" => &self.http,
            "https" => &self.https,
            _ => return None,
        };
        let proxy = entry.as_deref().filter(|proxy| !proxy.is_empty())?;
        let host = request_host(url);
        if self
            .no_proxy
            .as_deref()
            .is_some_and(|list| bypass_environment(&host, list))
        {
            return None;
        }
        if self.system_bypass(&host) {
            return None;
        }
        proxy_url(proxy)
    }

    /// `urllib.request.proxy_bypass` con los datos del sistema ya leídos.
    fn system_bypass(&self, host: &str) -> bool {
        if let Some(list) = &self.system_no_proxy {
            return bypass_environment(host, list);
        }
        self.registry_override
            .as_deref()
            .is_some_and(|list| bypass_registry(host, list))
    }
}

/// Variables reales del proceso (las que lee `urllib`).
pub fn process_environment() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

/// `getproxies_environment`: nombre sin sufijo (en minúsculas) → valor no vacío.
fn environment_map(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut upper: Vec<(String, String)> = Vec::new();
    for (name, value) in env {
        let name = name.to_uppercase();
        match upper.iter_mut().find(|(key, _)| *key == name) {
            Some(slot) => slot.1.clone_from(value),
            None => upper.push((name, value.clone())),
        }
    }
    let mut proxies: Vec<(String, String)> = Vec::new();
    for (name, value) in &upper {
        let Some(base) = name.strip_suffix("_PROXY") else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let key = base.to_lowercase();
        match proxies.iter_mut().find(|(known, _)| *known == key) {
            Some(slot) => slot.1.clone_from(value),
            None => proxies.push((key, value.clone())),
        }
    }
    if upper.iter().any(|(name, _)| name == "REQUEST_METHOD") {
        proxies.retain(|(key, _)| key != "http");
    }
    proxies
}

/// `getproxies_registry` para `ProxyServer`: devuelve los proxies `http` y `https`.
fn parse_proxy_server(server: &str) -> (Option<String>, Option<String>) {
    let all;
    let server = if server.contains('=') || server.contains(';') {
        server
    } else {
        all = format!("http={server};https={server};ftp={server}");
        &all
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    for part in server.split(';') {
        // `p.split('=', 1)` falla (`ValueError`) y deja lo ya leído.
        let Some((protocol, address)) = part.split_once('=') else {
            break;
        };
        let mut address = address.to_owned();
        if !has_scheme_prefix(&address) {
            match protocol {
                "http" | "https" | "ftp" => address = format!("http://{address}"),
                "socks" => address = format!("socks://{address}"),
                _ => {}
            }
        }
        match entries.iter_mut().find(|(known, _)| known == protocol) {
            Some(slot) => slot.1 = address,
            None => entries.push((protocol.to_owned(), address)),
        }
    }
    let get = |name: &str| {
        entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let (mut http, mut https) = (get("http"), get("https"));
    if let Some(socks) = get("socks").filter(|socks| !socks.is_empty()) {
        let address = match socks.strip_prefix("socks://") {
            Some(rest) => format!("socks4://{rest}"),
            None => socks,
        };
        if http.as_deref().is_none_or(str::is_empty) {
            http = Some(address.clone());
        }
        if https.as_deref().is_none_or(str::is_empty) {
            https = Some(address);
        }
    }
    (http, https)
}

/// `re.match('(?:[^/:]+)://', address)`.
fn has_scheme_prefix(address: &str) -> bool {
    address
        .find("://")
        .is_some_and(|at| at > 0 && !address[..at].contains(['/', ':']))
}

/// Lee `HKCU\...\Internet Settings`; cualquier fallo equivale a «sin proxy».
fn read_registry() -> RegistrySettings {
    let Ok(Some(key)) = Key::open(Hive::CurrentUser, INTERNET_SETTINGS, false) else {
        return RegistrySettings::default();
    };
    let enable = match key.get_raw("ProxyEnable") {
        // REG_DWORD
        Ok(Some(RawValue { kind: 4, data })) => data.iter().any(|byte| *byte != 0),
        Ok(Some(RawValue { data, .. })) => !data.is_empty(),
        _ => false,
    };
    RegistrySettings {
        enable,
        server: key.get_string("ProxyServer").ok().flatten(),
        proxy_override: key
            .get_string("ProxyOverride")
            .ok()
            .flatten()
            .unwrap_or_default(),
    }
}

/// `req.host` de `urllib`: `host[:puerto]` con el puerto solo si no es el de por defecto.
fn request_host(url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// `urllib.parse._splitport`: separa `host:puerto` solo si lo que sigue al último `:` son
/// dígitos. Devuelve el host sin los dos puntos aunque el puerto esté vacío.
fn split_port(host: &str) -> &str {
    match host.rfind(':') {
        Some(at) if host[at + 1..].bytes().all(|b| b.is_ascii_digit()) => &host[..at],
        _ => host,
    }
}

/// `proxy_bypass_environment`: `*` excluye todo; cada entrada excluye ese host y sus
/// subdominios. F5: `*.dominio` equivale a `.dominio`.
pub(crate) fn bypass_environment(host: &str, no_proxy: &str) -> bool {
    if no_proxy == "*" {
        return true;
    }
    let host = host.to_lowercase();
    let only = split_port(&host);
    for name in no_proxy.split(',') {
        let name = name.trim();
        let name = if name.starts_with("*.") {
            &name[1..]
        } else {
            name
        };
        let name = name.trim_start_matches('.').to_lowercase();
        if name.is_empty() {
            continue;
        }
        if only == name || host == name {
            return true;
        }
        let suffix = format!(".{name}");
        if only.ends_with(&suffix) || host.ends_with(&suffix) {
            return true;
        }
    }
    false
}

/// `_proxy_bypass_winreg_override`: patrones `fnmatch` separados por `;` y `<local>`.
pub(crate) fn bypass_registry(host: &str, proxy_override: &str) -> bool {
    let host = split_port(host).to_lowercase();
    proxy_override.split(';').any(|test| {
        let test = test.trim();
        if test == "<local>" {
            !host.contains('.')
        } else {
            fnmatch(&host, &test.to_lowercase())
        }
    })
}

/// `fnmatch.fnmatch` (sin distinguir mayúsculas, como en Windows): `*`, `?`, `[...]`.
fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    glob(&name, &pattern)
}

fn glob(name: &[char], pattern: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|skip| glob(&name[skip..], &pattern[1..])),
        Some('?') => !name.is_empty() && glob(&name[1..], &pattern[1..]),
        Some('[') => match class(pattern) {
            Some((set, negate, used)) => {
                name.first().is_some_and(|c| in_set(&set, *c) != negate)
                    && glob(&name[1..], &pattern[used..])
            }
            None => name.first() == Some(&'[') && glob(&name[1..], &pattern[1..]),
        },
        Some(literal) => name.first() == Some(literal) && glob(&name[1..], &pattern[1..]),
    }
}

/// Interpreta `[...]` al inicio de `pattern`: contenido, negación y caracteres
/// consumidos; `None` si el corchete no se cierra (entonces es literal).
fn class(pattern: &[char]) -> Option<(Vec<char>, bool, usize)> {
    let mut at = 1;
    let negate = pattern.get(at) == Some(&'!');
    if negate {
        at += 1;
    }
    let first = at;
    if pattern.get(at) == Some(&']') {
        at += 1;
    }
    while pattern.get(at).is_some_and(|c| *c != ']') {
        at += 1;
    }
    pattern.get(at)?;
    Some((pattern[first..at].to_vec(), negate, at + 1))
}

fn in_set(set: &[char], c: char) -> bool {
    let mut index = 0;
    while index < set.len() {
        if index + 2 < set.len() && set[index + 1] == '-' {
            if (set[index]..=set[index + 2]).contains(&c) {
                return true;
            }
            index += 3;
        } else {
            if set[index] == c {
                return true;
            }
            index += 1;
        }
    }
    false
}

/// `_parse_proxy`: dirección que `reqwest` puede usar. `urllib` conecta siempre en claro
/// con el proxy, así que un `https://` se normaliza a `http://`. Una URL sin `//` tras el
/// esquema es inválida (`ValueError` del original): la petición va directa.
fn proxy_url(proxy: &str) -> Option<String> {
    if let Some((scheme, rest)) = split_scheme(proxy)
        && rest.starts_with('/')
    {
        let rest = rest.strip_prefix("//")?;
        let scheme = if scheme.eq_ignore_ascii_case("https") {
            "http".to_owned()
        } else {
            scheme.to_lowercase()
        };
        return Some(format!("{scheme}://{rest}"));
    }
    Some(format!("http://{proxy}"))
}

/// `urllib.parse._splittype`: `([^/:]+):(.*)`.
fn split_scheme(text: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = text.split_once(':')?;
    (!scheme.is_empty() && !scheme.contains('/')).then_some((scheme, rest))
}

#[cfg(test)]
mod tests;
