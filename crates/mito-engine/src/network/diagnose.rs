//! Diagnóstico de la ruta de red (`diagnose`, `classify` y `apply` del original).

use mito_core::Layout;
use mito_core::config::Configuration;
use mito_transfer::network::{
    Probe, ProbeKind, SystemProxy, portal_fields, probe, urljoin, validate_url,
};

use super::{Connectivity, Network, plain_http};
use crate::error::Result;

/// Resultado de [`diagnose`] (`Diagnosis.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosisKind {
    /// Internet responde.
    Ok,
    /// No hay red local.
    Offline,
    /// Un portal cautivo redirige la comprobación.
    Portal,
    /// El proxy exige autenticación (407).
    Auth,
    /// La ruta responde con otro estado.
    Blocked,
    /// Ninguna ruta responde.
    Unreachable,
}

/// Qué ruta funciona y qué cambios de `config.properties` se proponen (`Diagnosis`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// Resultado.
    pub kind: DiagnosisKind,
    /// Proxy probado (`""` es directo).
    pub proxy: String,
    /// `config.properties`, `manual`, `pac`, `wpad`, `direct` o `""`.
    pub source: String,
    /// Código HTTP o 0.
    pub status: u16,
    /// Esquemas de `Proxy-Authenticate` (solo con [`DiagnosisKind::Auth`]).
    pub authenticate: String,
    /// URL absoluta del portal (solo con [`DiagnosisKind::Portal`]).
    pub portal: String,
    /// Claves propuestas, en el orden en que se muestran.
    pub changes: Vec<(String, String)>,
}

impl Diagnosis {
    fn bare(kind: DiagnosisKind) -> Self {
        Self {
            kind,
            proxy: String::new(),
            source: String::new(),
            status: 0,
            authenticate: String::new(),
            portal: String::new(),
            changes: Vec::new(),
        }
    }
}

fn put(changes: &mut Vec<(String, String)>, key: &str, value: &str) {
    match changes.iter_mut().find(|(known, _)| known == key) {
        Some(slot) => value.clone_into(&mut slot.1),
        None => changes.push((key.to_owned(), value.to_owned())),
    }
}

const fn rank(kind: DiagnosisKind) -> u8 {
    match kind {
        DiagnosisKind::Ok => 0,
        DiagnosisKind::Portal => 1,
        DiagnosisKind::Auth => 2,
        DiagnosisKind::Blocked => 3,
        DiagnosisKind::Unreachable | DiagnosisKind::Offline => 4,
    }
}

const fn from_probe(kind: ProbeKind) -> DiagnosisKind {
    match kind {
        ProbeKind::Ok => DiagnosisKind::Ok,
        ProbeKind::Auth => DiagnosisKind::Auth,
        ProbeKind::Portal => DiagnosisKind::Portal,
        ProbeKind::Unreachable => DiagnosisKind::Unreachable,
        ProbeKind::Blocked => DiagnosisKind::Blocked,
    }
}

/// `classify`: sonda de la URL de comprobación; una comprobación HTTPS que falla se repite
/// por HTTP para ver el 407 y las redirecciones de portal, que por HTTPS no se ven.
fn classify(network: &Network, proxy: &str) -> (Probe, ProbeKind) {
    let config = network.config();
    let result = probe(&config.check_url, proxy, config.timeout);
    let kind = result.kind(config.expected);
    if kind != ProbeKind::Ok
        && config
            .check_url
            .trim_start()
            .to_lowercase()
            .starts_with("https:")
    {
        let plain = probe(&plain_http(&config.check_url), proxy, config.timeout);
        let plain_kind = plain.kind(config.expected);
        if matches!(plain_kind, ProbeKind::Auth | ProbeKind::Portal) {
            return (plain, plain_kind);
        }
    }
    (result, kind)
}

/// Candidato a ruta: proxy, origen y `no_proxy` que se propondría.
struct Candidate {
    proxy: String,
    source: String,
    no_proxy: Option<String>,
}

/// `diagnose`: busca una ruta que funcione y propone cambios en `config.properties`.
///
/// Orden: la ruta configurada, la que usa Windows (`resolve`: manual, PAC o WPAD) y, si
/// un proxy configurado ya no existe, la conexión directa. **No guarda nada**: eso lo
/// hace [`apply`] tras la confirmación del usuario.
pub fn diagnose(
    network: &Network,
    connectivity: &dyn Connectivity,
    resolve: &dyn Fn(&str) -> Option<SystemProxy>,
) -> Diagnosis {
    if connectivity.check(true) {
        return Diagnosis::bare(DiagnosisKind::Ok);
    }
    if !connectivity.local_network() {
        return Diagnosis::bare(DiagnosisKind::Offline);
    }
    let config = network.values();
    let configured = config.contains("https_proxy") || config.contains("http_proxy");
    let mut candidates: Vec<Candidate> = Vec::new();
    if configured {
        let proxy = config
            .get("https_proxy")
            .or_else(|| config.get("http_proxy"))
            .unwrap_or_default();
        candidates.push(Candidate {
            proxy: proxy.to_owned(),
            source: "config.properties".to_owned(),
            no_proxy: None,
        });
    }
    let found = resolve(&network.config().check_url);
    if let Some(found) = &found
        && !candidates.iter().any(|c| c.proxy == found.url)
    {
        candidates.push(Candidate {
            proxy: found.url.clone(),
            source: found.source.clone(),
            no_proxy: Some(found.no_proxy.clone()),
        });
    }
    if !candidates.iter().any(|c| c.proxy.is_empty()) && (configured || found.is_none()) {
        // Sin claves de proxy la ruta directa ya es la predeterminada: nada que guardar.
        candidates.push(Candidate {
            proxy: String::new(),
            source: "direct".to_owned(),
            no_proxy: configured.then(String::new),
        });
    }
    let mut results: Vec<(DiagnosisKind, &Candidate, Probe)> = Vec::new();
    for candidate in &candidates {
        let (result, kind) = classify(network, &candidate.proxy);
        let kind = from_probe(kind);
        results.push((kind, candidate, result));
        if kind == DiagnosisKind::Ok {
            break;
        }
    }
    // `min_by_key` se queda con el primero en un empate.
    let Some((kind, candidate, result)) = results.into_iter().min_by_key(|(kind, ..)| rank(*kind))
    else {
        return Diagnosis::bare(DiagnosisKind::Unreachable);
    };
    let mut diagnosis = Diagnosis {
        kind,
        proxy: candidate.proxy.clone(),
        source: candidate.source.clone(),
        status: result.status,
        authenticate: result.authenticate.clone(),
        portal: String::new(),
        changes: Vec::new(),
    };
    if let Some(no_proxy) = &candidate.no_proxy
        && matches!(kind, DiagnosisKind::Ok | DiagnosisKind::Portal)
    {
        put(&mut diagnosis.changes, "http_proxy", &candidate.proxy);
        put(&mut diagnosis.changes, "https_proxy", &candidate.proxy);
        put(&mut diagnosis.changes, "no_proxy", no_proxy);
    }
    if kind == DiagnosisKind::Portal {
        let target = urljoin(&result.url, &result.location);
        let Ok(portal) = validate_url(&target) else {
            diagnosis.kind = DiagnosisKind::Blocked;
            diagnosis.changes.clear();
            return diagnosis;
        };
        diagnosis.portal = portal.to_string();
        if config
            .get("proxy_authentication_url")
            .is_none_or(str::is_empty)
        {
            put(
                &mut diagnosis.changes,
                "proxy_authentication_url",
                &diagnosis.portal.clone(),
            );
            for (key, value) in portal_fields(
                &diagnosis.portal,
                &candidate.proxy,
                network.config().timeout,
            ) {
                put(&mut diagnosis.changes, &key, &value);
            }
        }
        if config
            .get("enable_proxy_authentication")
            .unwrap_or("false")
            .to_lowercase()
            != "true"
        {
            put(
                &mut diagnosis.changes,
                "enable_proxy_authentication",
                "true",
            );
        }
    }
    diagnosis
}

/// `apply`: guarda `changes` con el editor de configuración, que conserva comentarios,
/// orden y claves desconocidas.
///
/// # Errors
///
/// Los de [`Configuration::global`] y [`Configuration::save`]: validación, bloqueo,
/// cambios externos y E/S.
pub fn apply(layout: &Layout, changes: &[(String, String)]) -> Result<()> {
    let mut configuration = Configuration::global(layout)?;
    let mut values = configuration.values().clone();
    for (key, value) in changes {
        values.insert(key.as_str(), value.as_str());
    }
    configuration.save(values.iter())?;
    Ok(())
}
