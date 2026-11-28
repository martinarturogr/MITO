//! Integración con Windows que usan la interfaz y su hilo de trabajo (F8).
//!
//! Agrupa en un solo valor lo que toca el sistema: el escritorio donde se crean los
//! accesos, las carpetas `AppData` del host, la rama del registro de «Abrir con» y de
//! protocolos, la búsqueda de aplicaciones del host, los selectores nativos y el
//! lanzamiento de aplicaciones del host. [`Integration::system`] usa las reales; las
//! pruebas inyectan carpetas temporales, una rama desechable de HKCU y selectores falsos
//! para no tocar nunca el escritorio, el `AppData` ni el registro del usuario.

use std::path::{Path, PathBuf};

use mito_desktop::host_apps;
use mito_desktop::host_data::HostRoots;
use mito_desktop::native::NativeError;
use mito_desktop::open_with::Classes;
use mito_desktop::shortcuts::desktop_path;
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
    use std::sync::atomic::{AtomicU32, Ordering};
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

    /// Lo que las pruebas pueden preparar y comprobar de la integración falsa.
    #[derive(Clone, Default)]
    pub struct Fakes {
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
        }
    }
}
