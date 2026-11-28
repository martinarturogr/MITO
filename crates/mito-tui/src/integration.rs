//! Integración con Windows que usan la interfaz y su hilo de trabajo (F8).
//!
//! Agrupa en un solo valor lo que toca el sistema: el escritorio donde se crean los
//! accesos, las carpetas `AppData` del host, la rama del registro de «Abrir con» y de
//! protocolos, la búsqueda de aplicaciones del host, los selectores nativos y el
//! lanzamiento de aplicaciones del host. [`Integration::system`] usa las reales; las
//! pruebas inyectan carpetas temporales, una rama desechable de HKCU y selectores falsos
//! para no tocar nunca el escritorio, el `AppData` ni el registro del usuario.

use std::path::{Path, PathBuf};

use mito_core::Layout;
use mito_desktop::host_apps;
use mito_desktop::host_data::HostRoots;
use mito_desktop::native::NativeError;
use mito_desktop::open_with::Classes;
use mito_desktop::shortcuts::desktop_path;
use mito_engine::network::{Connectivity, Credentials, Diagnosis, Network, diagnose};
use mito_engine::{Engine, Error, Result};

/// Función que da una carpeta del sistema.
type Folder<T> = Box<dyn Fn() -> Result<T> + Send + Sync>;
/// Traducción de las etiquetas de un selector nativo.
pub type Translate<'a> = &'a dyn Fn(&str) -> String;
/// Busca las aplicaciones del host en la raíz de MITO dada.
type Discover = Box<dyn Fn(&Path) -> Result<Vec<(String, PathBuf)>> + Send + Sync>;
/// Selector nativo de archivo.
type ChooseFile = Box<dyn Fn(Translate<'_>) -> Result<Option<PathBuf>> + Send + Sync>;
/// Selector nativo de carpeta, con la carpeta inicial.
type ChooseFolder = Box<dyn Fn(&Path, Translate<'_>) -> Result<Option<PathBuf>> + Send + Sync>;
/// Lanza una aplicación del host: motor, profile, identidad, argumentos y si va acoplada.
type LaunchHost = Box<dyn Fn(&Engine, &str, &str, &[String], bool) -> Result<i32> + Send + Sync>;

/// Comprueba Internet: `(¿hay Internet?, ¿hay red local?)`. Con `true` registra el fallo de
/// red como `network.check`.
type CheckNetwork = Box<dyn Fn(&Layout, bool) -> Result<(bool, bool)> + Send + Sync>;
/// Diagnostica la ruta de red (no guarda nada).
type DiagnoseNetwork = Box<dyn Fn(&Layout) -> Result<Diagnosis> + Send + Sync>;
/// Entra en el portal con las credenciales; `Ok(false)` si no lo acepta.
type Authenticate = Box<dyn Fn(&Layout, &Credentials) -> Result<bool> + Send + Sync>;

/// Operaciones de red que la interfaz inyecta (`Network.check`, `diagnose` y
/// `Network.authenticate` del original, que sus pruebas sustituyen con `patch`).
/// `apply` no está aquí: la interfaz guarda siempre con el real.
pub struct NetworkPort {
    /// Comprobación de Internet y de la red local.
    pub check: CheckNetwork,
    /// Diagnóstico de la ruta.
    pub diagnose: DiagnoseNetwork,
    /// Autenticación en el portal cautivo.
    pub authenticate: Authenticate,
}

impl NetworkPort {
    /// Las operaciones reales: sondas HTTP, PowerShell para el proxy de Windows y el
    /// portal de `config.properties`.
    pub fn system() -> Self {
        Self {
            check: Box::new(|layout, log| {
                let network = Network::open(layout, None)?;
                let online = network.check(log);
                let local = online || network.local_network();
                Ok((online, local))
            }),
            diagnose: Box::new(|layout| {
                let network = Network::open(layout, None)?;
                Ok(diagnose(
                    &network,
                    &network,
                    &mito_transfer::network::system_proxy,
                ))
            }),
            authenticate: Box::new(|layout, credentials| {
                Network::open(layout, None)?.authenticate(credentials)
            }),
        }
    }
}

/// Lo que la interfaz usa del sistema Windows.
pub struct Integration {
    /// Escritorio del usuario (donde se crean los accesos directos).
    pub desktop: Folder<PathBuf>,
    /// Carpetas `AppData` del host.
    pub host: Folder<HostRoots>,
    /// Rama del registro de «Abrir con» y de protocolos.
    pub classes: Classes,
    /// Aplicaciones del host que hay en el menú Inicio y el escritorio.
    pub discover: Discover,
    /// Selector nativo de archivo.
    pub choose_file: ChooseFile,
    /// Selector nativo de carpeta.
    pub choose_folder: ChooseFolder,
    /// Lanzamiento de una aplicación del host.
    pub launch_host: LaunchHost,
    /// Comprobación, diagnóstico y autenticación de red (botón INTERNET).
    pub network: NetworkPort,
}

impl std::fmt::Debug for Integration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Integration").finish_non_exhaustive()
    }
}

/// Un error de un selector nativo, como error del motor.
fn native(error: NativeError) -> Error {
    Error::untranslated(error)
}

impl Integration {
    /// El escritorio, el `AppData`, el registro y los selectores reales de Windows.
    pub fn system() -> Self {
        Self {
            desktop: Box::new(desktop_path),
            host: Box::new(HostRoots::system),
            classes: Classes::system(),
            discover: Box::new(host_apps::discover_host),
            choose_file: Box::new(|translate| {
                mito_desktop::native::choose_file(translate).map_err(native)
            }),
            choose_folder: Box::new(|initial, translate| {
                mito_desktop::native::choose_folder(initial, translate).map_err(native)
            }),
            launch_host: Box::new(|engine, profile, identity, arguments, coupled| {
                host_apps::launch(engine, profile, identity, arguments, coupled, false)
            }),
            network: NetworkPort::system(),
        }
    }
}

impl Default for Integration {
    fn default() -> Self {
        Self::system()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "utilidades de prueba")]
pub(crate) mod testing {
    //! Integración falsa para las pruebas: nada toca el sistema real.

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use mito_win::registry::{Hive, Key};

    use super::*;

    /// Lanzamientos del host registrados: `(profile, identidad, argumentos, acoplado)`.
    pub type Launches = Arc<Mutex<Vec<(String, String, Vec<String>, bool)>>>;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Rama `HKCU\Software\MITO-tests\<único>` que se borra al soltarla.
    pub struct Sandbox {
        name: String,
        pub classes: Classes,
    }

    impl Sandbox {
        pub fn new() -> Self {
            let name = format!(
                "tui-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let classes = Classes::under(&format!(r"Software\MITO-tests\{name}"));
            Self { name, classes }
        }
    }

    impl Sandbox {
        fn path(&self, scheme: &str) -> String {
            format!(r"Software\MITO-tests\{}\Classes\{scheme}", self.name)
        }

        /// Registra el protocolo `scheme` con `command` en la rama de prueba.
        pub fn protocol(&self, scheme: &str, command: &str) {
            let path = self.path(scheme);
            let key = Key::create(Hive::CurrentUser, &path).unwrap();
            key.set_string("URL Protocol", "").unwrap();
            let open =
                Key::create(Hive::CurrentUser, &format!(r"{path}\shell\open\command")).unwrap();
            open.set_string("", command).unwrap();
        }

        /// Comando registrado para `scheme`, si lo hay.
        pub fn command(&self, scheme: &str) -> Option<String> {
            let path = format!(r"{}\shell\open\command", self.path(scheme));
            Key::open(Hive::CurrentUser, &path, false)
                .unwrap()?
                .get_string("")
                .unwrap()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            if let Ok(Some(parent)) = Key::open(Hive::CurrentUser, r"Software\MITO-tests", true) {
                let _ = parent.delete_tree(&self.name);
            }
        }
    }

    /// Lo que la red falsa responde y lo que se le pidió.
    pub struct NetworkScript {
        /// Respuesta de `check`: `(Internet, red local)`.
        pub check: (bool, bool),
        /// Si no es `None`, `check` falla con este mensaje.
        pub check_error: Option<String>,
        /// Respuesta de `diagnose`; `Err` con el mensaje si es `Err`.
        pub diagnosis: std::result::Result<Diagnosis, String>,
        /// Respuesta de `authenticate`.
        pub auth: std::result::Result<bool, String>,
        /// Valor de `log` de cada `check` recibido.
        pub checks: Vec<bool>,
        /// Número de `diagnose` recibidos.
        pub diagnoses: u32,
        /// `(usuario, contraseña)` de cada `authenticate` recibido.
        pub auths: Vec<(String, String)>,
    }

    impl Default for NetworkScript {
        fn default() -> Self {
            Self {
                check: (true, true),
                check_error: None,
                diagnosis: Err("sin diagnóstico preparado".into()),
                auth: Ok(true),
                checks: Vec::new(),
                diagnoses: 0,
                auths: Vec::new(),
            }
        }
    }

    /// Red falsa: el guion y una compuerta que retiene los `check` mientras esté cerrada.
    #[derive(Clone, Default)]
    pub struct FakeNetwork {
        pub script: Arc<Mutex<NetworkScript>>,
        /// Mientras sea `true`, `check` espera.
        pub hold: Arc<AtomicBool>,
    }

    impl FakeNetwork {
        /// Retiene (o libera) los `check` siguientes.
        pub fn hold(&self, held: bool) {
            self.hold.store(held, Ordering::SeqCst);
        }

        /// Cuántos `check` se pidieron.
        pub fn checks(&self) -> usize {
            self.script.lock().unwrap().checks.len()
        }

        fn port(&self) -> NetworkPort {
            let check = Arc::clone(&self.script);
            let hold = Arc::clone(&self.hold);
            let diagnose = Arc::clone(&self.script);
            let authenticate = Arc::clone(&self.script);
            NetworkPort {
                check: Box::new(move |_, log| {
                    check.lock().unwrap().checks.push(log);
                    while hold.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    let script = check.lock().unwrap();
                    match &script.check_error {
                        Some(text) => Err(Error::untranslated(text)),
                        None => Ok(script.check),
                    }
                }),
                diagnose: Box::new(move |_| {
                    let mut script = diagnose.lock().unwrap();
                    script.diagnoses += 1;
                    script.diagnosis.clone().map_err(Error::untranslated)
                }),
                authenticate: Box::new(move |_, credentials| {
                    let mut script = authenticate.lock().unwrap();
                    script.auths.push((
                        credentials.username.clone(),
                        credentials.password.to_string(),
                    ));
                    script.auth.clone().map_err(Error::untranslated)
                }),
            }
        }
    }

    /// Lo que las pruebas pueden preparar y comprobar de la integración falsa.
    #[derive(Clone, Default)]
    pub struct Fakes {
        /// Comprobación, diagnóstico y autenticación de red.
        pub network: FakeNetwork,
        /// Aplicaciones que «encuentra» la búsqueda del host.
        pub found: Arc<Mutex<Vec<(String, PathBuf)>>>,
        /// Respuestas del selector de archivo (vacío o `None` = cancelado).
        pub files: Arc<Mutex<VecDeque<Option<PathBuf>>>>,
        /// Respuestas del selector de carpeta.
        pub folders: Arc<Mutex<VecDeque<Option<PathBuf>>>>,
        /// Lanzamientos del host: `(profile, identidad, argumentos, acoplado)`.
        pub launched: Launches,
    }

    /// Integración de prueba: escritorio y `AppData` temporales, rama de registro dada y
    /// los selectores falsos de `fakes`.
    pub fn integration(
        desktop: &Path,
        host: &HostRoots,
        classes: &Classes,
        fakes: &Fakes,
    ) -> Integration {
        let desktop = desktop.to_path_buf();
        let host = host.clone();
        let found = Arc::clone(&fakes.found);
        let files = Arc::clone(&fakes.files);
        let folders = Arc::clone(&fakes.folders);
        let launched = Arc::clone(&fakes.launched);
        Integration {
            desktop: Box::new(move || Ok(desktop.clone())),
            host: Box::new(move || Ok(host.clone())),
            classes: classes.clone(),
            discover: Box::new(move |_| Ok(found.lock().unwrap().clone())),
            choose_file: Box::new(move |_| Ok(files.lock().unwrap().pop_front().flatten())),
            choose_folder: Box::new(move |_, _| Ok(folders.lock().unwrap().pop_front().flatten())),
            launch_host: Box::new(move |_, profile, identity, arguments, coupled| {
                launched.lock().unwrap().push((
                    profile.to_owned(),
                    identity.to_owned(),
                    arguments.to_vec(),
                    coupled,
                ));
                Ok(0)
            }),
            network: fakes.network.port(),
        }
    }
}
