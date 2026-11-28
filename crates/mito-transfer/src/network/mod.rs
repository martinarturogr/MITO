//! Red del original (`network.py`) sin leer `config.properties` (ADR 0008, punto 10):
//! proxies con la semántica de `urllib`, cliente HTTP sin redirecciones, sondas,
//! formulario del portal y proxy del sistema.
//!
//! Contratos: `docs/spec/modules/network.md` y `network_api.md`.

mod address;
mod entities;
mod html;
mod probe;
mod proxies;
mod session;
mod system;

pub use address::{REDIRECTS, origin, redirect_location, same_origin, urljoin, validate_url};
pub use html::{PortalForm, portal_forms};
pub use probe::{Probe, ProbeKind, portal_fields, probe};
pub use proxies::{Proxies, RegistrySettings, process_environment};
pub use session::{Failure, MAX_BODY, Reply, Session, SessionOptions};
pub use system::{
    PowerShell, PowerShellEnv, SYSTEM_PROXY_SCRIPT, SystemProxy, no_proxy_from_override,
    probe_with_windows_credentials, probe_with_windows_credentials_with, system_proxy,
    system_proxy_with,
};
