# API de Rust de la red (F9)

Complementa los contratos de comportamiento [network.md](network.md),
[network_ui.md](network_ui.md) y la sección 5.7 de [bootstrap.md](bootstrap.md). Aquí se fija
**dónde** vive cada pieza y **qué firma** tiene; el comportamiento es siempre el del
original. Referencia de `urllib` para la semántica de proxies: `C:\Python314\Lib\urllib\request.py`
(`getproxies_environment`, `getproxies_registry`, `proxy_bypass_environment`,
`proxy_bypass_registry`, `ProxyHandler.proxy_open`).

## Capas

| Pieza | Crate | Motivo |
|---|---|---|
| Estado de la red local (`InternetGetConnectedState`) | `mito-win::network::local_network` | FFI |
| Entrada de consola sin eco | `mito-win::console::HiddenInput` | FFI |
| Proxies, sondas, cliente HTTP sin redirecciones, formulario del portal, proxy del sistema | `mito-transfer::network` | HTTP sin leer `config.properties` (ADR 0008, punto 10) |
| `config.properties` → red: `proxy_environment`, `Network`, `authenticate`, `diagnose`, `apply`, `intercept`, credenciales | `mito-engine::network` | Lee la configuración de la instancia |
| Diálogo nativo de credenciales (`prompt_credentials`) | `mito-desktop::native` | Selectores nativos de F8 |
| Botón INTERNET, `NetworkSetupDialog`, `ProxyDialog` | `mito-tui` | Interfaz |
| `intercept` antes de `run` (sin `--bin`) y `host-run`; proxy del bootstrap | `mito` | CLI |

## `mito-transfer::network`

```rust
/// Estados de redirección (`REDIRECTS`).
pub const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];

/// Proxies de una petición (`proxies` de `ProxyHandler`). `None`: sin entrada para ese
/// esquema (directo); `Some("")`: entrada vacía (directo, anula al sistema).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proxies {
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Option<String>,
    /// `ProxyOverride` del registro, solo si los proxies salieron del registro
    /// (`proxy_bypass_registry`); `None` si salieron del entorno.
    pub registry_override: Option<String>,
}

impl Proxies {
    /// `urllib.request.getproxies()` en Windows: las variables `*_proxy` de `env` o, si no
    /// hay ninguna, el registro de Internet Settings de HKCU (`ProxyEnable`, `ProxyServer`,
    /// `ProxyOverride`).
    pub fn system(env: &[(String, String)]) -> Self;
    /// Parte pura de `system` para el entorno (`getproxies_environment`).
    pub fn from_environment(env: &[(String, String)]) -> Self;
    /// Parte pura de `system` para el registro (`getproxies_registry`): `ProxyServer`
    /// `host:puerto` o `http=…;https=…`, con `http://` por defecto.
    pub fn from_registry(enable: bool, server: &str, proxy_override: &str) -> Self;
    /// `opener`: las claves `http_proxy`, `https_proxy` y `no_proxy` de `env` (sin
    /// distinguir mayúsculas) sustituyen a las del sistema.
    pub fn overridden(self, env: &[(String, String)]) -> Self;
    /// Ruta de una sonda (`probe`): `""` directo; otro valor, para `http` y `https`.
    pub fn single(proxy: &str) -> Self;
    /// Proxy que usa `url`, o `None` si va directa: entrada vacía o ausente, o bypass de
    /// `no_proxy` (`proxy_bypass_environment`) o del registro (`proxy_bypass`).
    pub fn proxy_for(&self, url: &reqwest::Url) -> Option<String>;
}
```

- `DownloadOptions` cambia `proxy`/`no_proxy` por `proxies: Option<Proxies>`; `None`
  equivale a `Proxies::system(&entorno del proceso)`. El cliente se construye siempre con
  `reqwest::Proxy::custom(move |url| proxies.proxy_for(url))` (y `no_proxy()` para que
  `reqwest` no añada los suyos), así la semántica es exactamente la del original.
- Un proxy `https://…` del registro o del entorno se trata como el original: `urllib`
  conecta siempre en claro con el proxy. Si `reqwest` exige TLS hacia un proxy `https://`,
  se normaliza a `http://` y se documenta.

```rust
/// Valida una URL HTTP/HTTPS con host y sin credenciales (`validate_url`).
pub fn validate_url(url: &str) -> mito_core::Result<reqwest::Url>;
/// `(esquema, netloc)` en minúsculas, con la `netloc` tal cual (`urlsplit`).
pub fn origin(url: &str) -> (String, String);
/// Destino absoluto de una redirección, o `""` (`redirect_location`; `urljoin`).
pub fn redirect_location(url: &str, status: u16, location: Option<&str>) -> String;

/// Cliente sin redirecciones (`NoRedirect`) con cookies opcionales.
pub struct Session { /* … */ }
pub struct SessionOptions { pub proxies: Proxies, pub timeout: Duration, pub cookies: bool }
pub struct Reply { pub status: u16, pub location: Option<String>, pub proxy_authenticate: Vec<String>, pub body: Vec<u8> /* ≤ 1 MiB */ }
/// Fallo de transporte: nombre del tipo de error y, si fue el CONNECT del túnel, su estado.
pub struct Failure { pub error: String, pub tunnel_status: Option<u16> }
impl Session {
    pub fn new(options: &SessionOptions) -> Result<Self, Failure>;
    pub fn get(&self, url: &str) -> Result<Reply, Failure>;
    pub fn post_form(&self, url: &str, fields: &[(String, String)]) -> Result<Reply, Failure>;
}

/// Formulario con contraseña (`Form`): campos en orden de inserción, sin duplicados.
pub struct PortalForm { pub action: String, pub fields: Vec<(String, String)>, pub user_field: String, pub password_field: String }
/// Formularios con contraseña de `html` (`Form(password_field).feed`).
pub fn portal_forms(html: &str, password_field: &str) -> Vec<PortalForm>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind { Ok, Auth, Portal, Unreachable, Blocked }
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe { pub status: u16, pub location: String, pub authenticate: String, pub error: String, pub url: String }
impl Probe { pub fn kind(&self, expected: u16) -> ProbeKind; }
/// Una petición por `proxy` (`""` directo) sin seguir redirecciones (`probe`).
pub fn probe(url: &str, proxy: &str, timeout: Duration) -> Probe;
/// Nombres de campo del portal distintos de los predeterminados (`portal_fields`).
pub fn portal_fields(url: &str, proxy: &str, timeout: Duration) -> Vec<(String, String)>;

/// `ProxyOverride` de Windows a `no_proxy` (`no_proxy_from_override`).
pub fn no_proxy_from_override(proxy_override: &str) -> String;
/// Script literal del original (`SYSTEM_PROXY_SCRIPT`).
pub const SYSTEM_PROXY_SCRIPT: &str;
pub struct SystemProxy { pub url: String, pub source: String, pub no_proxy: String }
/// Proxy que usaría Windows para `target` (`system_proxy`), con PowerShell.
pub fn system_proxy(target: &str) -> Option<SystemProxy>;
/// Igual, con el ejecutor inyectado: recibe el script y `MITO_PROXY_TARGET`, y devuelve
/// `(código, stdout)` o `None` si no arrancó o superó 30 s.
pub fn system_proxy_with(target: &str, run: &dyn Fn(&str, &str) -> Option<(i32, String)>) -> Option<SystemProxy>;
/// Solo bootstrap: ¿responde `expected` la sonda con las credenciales de Windows
/// (NTLM/Kerberos)? Se hace con PowerShell (`-ProxyUseDefaultCredentials`), porque
/// `reqwest` no las admite.
pub fn probe_with_windows_credentials(url: &str, proxy: &str, timeout: Duration, expected: u16) -> bool;
```

## `mito-engine::network`

```rust
/// Contraseña que se borra de memoria al soltarse (`zeroize`); nunca se registra.
pub struct Credentials { pub username: String, pub password: zeroize::Zeroizing<String> }

/// Claves de red de `config.properties` ya validadas (`Network.__init__`).
pub struct NetworkConfig {
    pub timeout: Duration, pub check_url: String, pub expected: u16,
    pub auth_url: String, pub enabled: bool,
    pub username_field: String, pub password_field: String,
    pub require_https: bool, pub form_fields: Vec<(String, String)>,
}
impl NetworkConfig { pub fn from_values(values: &Values) -> Result<Self>; }
/// `network.poll_seconds` (`start_network_poll`): `None` si está desactivado.
pub fn poll_interval(values: &Values) -> Option<Duration>;

/// `proxy_environment`: aplica a `env` las claves de proxy de `values`.
pub fn proxy_environment(values: &Values, env: &mut Env) -> Result<()>;
/// Extensión [`crate::extensions::Network`] que lee `config.properties` en cada llamada.
/// Pasa a ser la predeterminada de `Extensions` (sustituye a `NoNetwork`).
pub struct ConfigNetwork;

/// Lo que `diagnose` y la interfaz consultan de la conectividad (inyectable en tests).
pub trait Connectivity { fn check(&self, log: bool) -> bool; fn local_network(&self) -> bool; }

pub struct Network { /* layout, config, values, env */ }
impl Network {
    /// `Network(root, environment)`: `env` `None` usa el entorno del proceso con
    /// `proxy_environment` aplicado.
    pub fn open(layout: &Layout, env: Option<&Env>) -> Result<Self>;
    pub fn config(&self) -> &NetworkConfig;
    pub fn values(&self) -> &Values;
    pub fn enabled(&self) -> bool;
    /// `opener`: proxies del sistema sustituidos por el entorno.
    pub fn proxies(&self) -> Proxies;
    /// `authenticate`: `Ok(false)` si no verifica; `Err` solo por los errores de
    /// configuración del original (otro origen, HTTPS obligatorio).
    pub fn authenticate(&self, credentials: &Credentials) -> Result<bool>;
}
impl Connectivity for Network { /* check (log al diagnóstico si log), local_network */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosisKind { Ok, Offline, Portal, Auth, Blocked, Unreachable }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis { pub kind: DiagnosisKind, pub proxy: String, pub source: String, pub status: u16, pub authenticate: String, pub portal: String, pub changes: Vec<(String, String)> }
/// `diagnose`: con el resolvedor de proxy del sistema inyectado.
pub fn diagnose(network: &Network, connectivity: &dyn Connectivity, resolve: &dyn Fn(&str) -> Option<SystemProxy>) -> Diagnosis;
/// `apply`: guarda `changes` con el editor de configuración (conserva comentarios y claves).
pub fn apply(layout: &Layout, changes: &[(String, String)]) -> Result<()>;
/// `intercept`: nunca falla ni impide lanzar; `prompt(failed)` pide credenciales.
pub fn intercept(layout: &Layout, env: Option<&Env>, prompt: &mut dyn FnMut(bool) -> Option<Credentials>);
```

- `HttpDownloader::for_root(layout)`: en cada descarga resuelve `Network::proxies()` con
  la configuración vigente (como `_build_opener` en cada intento); un proxy inválido en
  `config.properties` es el error de la descarga. `HttpDownloader::default()` sigue sin
  raíz (proxies del sistema), como `transfer` con `root=None`.

## `mito-desktop::native`

```rust
/// Diálogo de credenciales de Windows Forms (`prompt_credentials`), script literal y
/// etiquetas localizadas. `None` si se cancela o PowerShell falla.
pub fn prompt_credentials(failed: bool, translate: &dyn Fn(&str) -> String) -> Option<Credentials>;
pub fn credentials_script(failed: bool, translate: &dyn Fn(&str) -> String) -> String;
```

## `mito-tui`

- `Integration` gana `network: NetworkPort`, con las operaciones inyectables: `check`
  (`(online, local)`), `diagnose`, `authenticate`; `apply` es la real. Las pruebas usan
  fakes como `patch('mito.network.Network.check')` del original.
- El botón INTERNET deja de estar oculto. Diálogos `NetworkSetup` y `Proxy` (campo de
  contraseña enmascarado; la contraseña se borra del campo al enviar o cancelar).

## Criterios de aceptación

1. Todos los tests de `test_network.py`, `test_network_setup.py` y `test_bootstrap_proxy.py`
   tienen su equivalente en Rust contra servidores y proxies HTTP locales (`127.0.0.1`).
2. Ninguna credencial aparece en `logs/`, `config.properties` ni en `Debug` de ningún
   tipo (test que lo recorre, como el original).
3. Ningún test usa la red real, el proxy real del equipo ni el registro real: el registro
   y el ejecutor de PowerShell se inyectan.
4. `cargo fmt`, `clippy -D warnings` y `cargo test --workspace` en verde.
