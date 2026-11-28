//! Red antes de lanzar (`intercept` en `run` sin `--bin` y en `host-run`).
//!
//! Si no hay Internet y la autenticación del portal está configurada, pide usuario y
//! contraseña con el diálogo nativo hasta que entran o el usuario cancela. Nunca impide
//! el lanzamiento: cancelar, no tener red o un fallo de configuración continúan sin
//! conexión. Funciona igual desde `mito.exe` y `mitow.exe`: el diálogo es una ventana
//! de Windows Forms, no usa la consola.

use mito_core::Layout;
use mito_engine::network::{Connectivity, Credentials, Network, intercept_with};
use mito_engine::{Engine, Env};
use mito_i18n::Translator;
use mito_transfer::network::Proxies;

/// Qué se sustituye por pruebas: la conectividad, los proxies del sistema y el diálogo.
#[derive(Default)]
pub struct Overrides<'a> {
    /// Conectividad que decide si hay Internet y red local (por defecto, la real).
    pub connectivity: Option<&'a dyn Connectivity>,
    /// Proxies del sistema fijos (por defecto, el entorno y el registro).
    pub system: Option<Proxies>,
}

/// `intercept` de producción: el entorno del profile, la red real y el diálogo nativo
/// con el idioma de la instancia.
pub fn intercept_for_launch(engine: &Engine, profile: &str) {
    let layout = engine.layout();
    let translator = Translator::from_layout(layout).unwrap_or_else(|_| Translator::new("system"));
    let translate = |text: &str| translator.text(text).to_owned();
    let mut prompt =
        |failed: bool| mito_desktop::native::prompt_credentials_in(layout, failed, &translate);
    intercept_with_prompt(engine, profile, &Overrides::default(), &mut prompt);
}

/// Intercepta con el diálogo `prompt(failed)` y las sustituciones indicadas.
///
/// Si el entorno del profile no se puede construir no hace nada: el lanzamiento que
/// sigue informará del mismo error. Sin autenticación de portal configurada no se
/// consulta la red (el original comprobaba Internet antes, solo para no preguntar).
pub fn intercept_with_prompt(
    engine: &Engine,
    profile: &str,
    overrides: &Overrides<'_>,
    prompt: &mut dyn FnMut(bool) -> Option<Credentials>,
) {
    let layout = engine.layout();
    let Ok(env) = engine.environment(profile) else {
        return;
    };
    match open(layout, &env, overrides.system.as_ref()) {
        Ok(network) => {
            if !network.enabled() {
                return;
            }
            let connectivity: &dyn Connectivity = overrides.connectivity.unwrap_or(&network);
            intercept_with(layout, &network, connectivity, prompt);
        }
        Err(_) => {
            // `intercept` registra el fallo de apertura como `network.check`.
            mito_engine::network::intercept(layout, Some(&env), prompt);
        }
    }
}

fn open(layout: &Layout, env: &Env, system: Option<&Proxies>) -> mito_engine::Result<Network> {
    match system {
        Some(system) => Network::with_system(layout, Some(env), system.clone()),
        None => Network::open(layout, Some(env)),
    }
}
