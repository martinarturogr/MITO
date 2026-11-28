//! Interceptor de arranque (`intercept` del original).

use mito_core::Layout;
use mito_core::diagnostics::{self, LogLevel, write};
use mito_transfer::network::origin;
use serde_json::json;

use super::{Connectivity, Credentials, Network};
use crate::environment::Env;

/// `intercept`: antes de lanzar una aplicación, si no hay Internet y el portal está
/// configurado, pide credenciales con `prompt(failed)` hasta que entran o se cancela.
///
/// Nunca falla ni impide lanzar: cancelar o no tener red continúa sin conexión. Respecto
/// al original (F1), los errores de configuración de [`Network::authenticate`] (HTTPS
/// obligatorio, otro origen) y de abrir la configuración se registran en `logs/mito.log`
/// en lugar de callarse.
pub fn intercept(
    layout: &Layout,
    env: Option<&Env>,
    prompt: &mut dyn FnMut(bool) -> Option<Credentials>,
) {
    match Network::open(layout, env) {
        Ok(network) => intercept_with(layout, &network, &network, prompt),
        Err(error) => {
            write(
                layout,
                LogLevel::Warning,
                "network.check",
                &diagnostics::message(layout, &error),
                &[],
            );
        }
    }
}

/// [`intercept`] con la conectividad inyectada: `connectivity` decide si hay Internet y
/// red local; `network` autentica.
pub fn intercept_with(
    layout: &Layout,
    network: &Network,
    connectivity: &dyn Connectivity,
    prompt: &mut dyn FnMut(bool) -> Option<Credentials>,
) {
    if connectivity.check(true) || !network.enabled() || !connectivity.local_network() {
        return;
    }
    let mut failed = false;
    loop {
        let Some(credentials) = prompt(failed) else {
            return;
        };
        match network.authenticate(&credentials) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                let (scheme, netloc) = origin(&network.config().auth_url);
                write(
                    layout,
                    LogLevel::Warning,
                    "network.authenticate",
                    &diagnostics::message(layout, &error),
                    &[("portal", json!([scheme, netloc]))],
                );
                return;
            }
        }
        failed = true;
    }
}
