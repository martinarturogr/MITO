//! Acciones de la interfaz en un hilo de trabajo, con progreso y confirmaciones.
//!
//! Reproduce el worker `operation` de `ui.py` y su «Tabla de acciones del worker»
//! (`docs/spec/modules/ui.md`). Una acción corre en su propio hilo y habla con la
//! interfaz solo por un canal de [`Report`]: nunca toca widgets. Cuando el motor pide
//! una confirmación (versión nueva, descargas sin hash) el hilo se bloquea hasta que la
//! interfaz responde por el canal de la propia petición.
//!
//! El equivalente de `wait_startup_updates` está en la interfaz: la comprobación del
//! arranque corre en su propio hilo y una acción pedida mientras tanto empieza al
//! terminar esta.
//!
//! `Launch` usa siempre el modo independiente (sin acoplar, F8).

use std::cell::Cell;
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use mito_catalog::pyjson::py_str;
use mito_core::diagnostics::{self, LogLevel};
use mito_core::error_tracking::{self, Options};
use mito_core::{Message, MutationLock, Operation};
use mito_engine::{
    DownloaderFetcher, Engine, Error, InstallOptions, LaunchOptions, LogLine, Records, Result,
    version_key,
};
use mito_i18n::Translator;
use serde_json::{Map, json};

use crate::model::{IntegrityInfo, describe};

/// Lo que la interfaz pide hacer en el hilo (`begin(action, …)` del original).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// «Buscar actualizaciones»: refresca los manifiestos de los instalados forzando la
    /// comprobación diaria (`check-updates`).
    CheckUpdates,
    /// Crea un profile con ese nombre (`create`).
    Create(String),
    /// Elimina el profile seleccionado (`delete-profile`).
    DeleteProfile,
    /// Activa o desactiva un componente en el profile (`toggle`).
    Toggle {
        /// Id de la instalación.
        installation: String,
        /// Nuevo valor de la casilla.
        active: bool,
    },
    /// Instala una clave del catálogo (`install_from_catalog`).
    Install(String),
    /// Elimina un componente (`remove-component`).
    RemoveComponent(String),
    /// Actualiza un componente (`update-component`).
    UpdateComponent(String),
    /// Actualiza varios componentes (`update-all`).
    UpdateAll(Vec<String>),
    /// Quita la retención de versión (`release-version`).
    ReleaseVersion(String),
    /// Abre una aplicación en modo independiente (`launch`).
    Launch {
        /// Id de la instalación.
        installation: String,
        /// Argumentos del usuario.
        arguments: Vec<String>,
        /// Entrada de aplicación elegida, si el manifiesto declara varias.
        shortcut: Option<String>,
    },
    /// «Actualizar catálogos»: reimporta todos los buckets configurados (`refresh`).
    Refresh,
    /// Comprobación de componentes al arrancar (`check_component_updates`): como
    /// `CheckUpdates`, pero respeta la caché diaria y no registra «Búsqueda … finalizada».
    StartupUpdates,
}

impl Action {
    /// Nombre de la acción en el original, para `operation = "ui.<nombre>"` del registro.
    pub fn name(&self) -> &'static str {
        match self {
            Self::CheckUpdates => "check-updates",
            Self::Create(_) => "create",
            Self::DeleteProfile => "delete-profile",
            Self::Toggle { .. } => "toggle",
            Self::Install(_) => "install",
            Self::RemoveComponent(_) => "remove-component",
            Self::UpdateComponent(_) => "update-component",
            Self::UpdateAll(_) => "update-all",
            Self::ReleaseVersion(_) => "release-version",
            Self::Launch { .. } => "launch",
            Self::Refresh => "refresh",
            Self::StartupUpdates => "startup-updates",
        }
    }

    /// Componente al que afecta, para el seguimiento de errores.
    fn component(&self) -> Option<&str> {
        match self {
            Self::Toggle { installation, .. } | Self::Launch { installation, .. } => {
                Some(installation)
            }
            Self::Install(key)
            | Self::RemoveComponent(key)
            | Self::UpdateComponent(key)
            | Self::ReleaseVersion(key) => Some(key),
            _ => None,
        }
    }
}

/// Pregunta que el hilo hace a la interfaz y espera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `InstallVersionDialog`: la versión del catálogo es posterior a la que vio el
    /// usuario.
    InstallVersion {
        /// Clave del componente.
        key: String,
        /// Versión que mostraba el catálogo.
        previous: String,
        /// Versión que se va a instalar.
        version: String,
    },
    /// `UnverifiedDownloadsDialog`: descargas sin hash, una por elemento.
    Unverified(Vec<String>),
}

/// Mensajes del hilo a la interfaz.
#[derive(Debug)]
pub enum Report {
    /// Línea del log. El mensaje va sin traducir; la interfaz lo traduce.
    Log {
        /// Mensaje con sus argumentos.
        message: Message,
        /// Se pinta en rojo y negrita.
        error: bool,
        /// `Some(true)` abre un paso largo sin progreso (`LongStep`) y `Some(false)` lo
        /// cierra.
        long_step: Option<bool>,
    },
    /// Confirmación pendiente: el hilo espera un `bool` en `answer`. Si el canal se
    /// rompe o se descarta, cuenta como `false`.
    Confirm {
        /// Qué se pregunta.
        request: Request,
        /// Dónde responder.
        answer: Sender<bool>,
    },
    /// Primera fase activa de la operación, para el diálogo de progreso. Su estado se lee
    /// con `Operation::state()` y se cancela con `Operation::cancel()`.
    Started(Operation),
    /// La acción terminó (bien, con error o con un pánico). Siempre es el último
    /// informe.
    Done {
        /// Profile seleccionado al terminar: `Create` y `DeleteProfile` lo cambian.
        profile: Option<String>,
        /// `None` si todo fue bien.
        failure: Option<Failure>,
    },
}

/// Cómo falló una acción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// El usuario canceló la operación (ya se escribió «Instalación cancelada…»).
    pub cancelled: bool,
    /// Datos para el diálogo «DESCARGA ALTERADA POR LA RED», si ese fue el motivo.
    pub integrity: Option<IntegrityInfo>,
}

/// Lanza `action` en un hilo y devuelve su manejador. Los errores se describen con el
/// idioma de `config.properties` en el momento de lanzar la acción.
///
/// Siempre llega un [`Report::Done`] al final, incluso si la acción falla o entra en
/// pánico. `profile` es el profile seleccionado.
pub fn spawn(
    engine: Arc<Engine>,
    action: Action,
    profile: Option<String>,
    reports: Sender<Report>,
) -> JoinHandle<()> {
    let tr = Translator::from_layout(engine.layout()).unwrap_or_default();
    spawn_translated(engine, action, profile, reports, tr)
}

/// Como [`spawn`], con el traductor que usa la interfaz para describir los errores.
pub fn spawn_translated(
    engine: Arc<Engine>,
    action: Action,
    profile: Option<String>,
    reports: Sender<Report>,
    tr: Translator,
) -> JoinHandle<()> {
    thread::spawn(move || run(&engine, &action, profile, &reports, &tr))
}

/// Cuerpo del hilo.
fn run(
    engine: &Engine,
    action: &Action,
    profile: Option<String>,
    reports: &Sender<Report>,
    tr: &Translator,
) {
    // El listener del `Operation` necesita el propio `Operation` para enviar `Started`;
    // la ranura se vacía al terminar para romper el ciclo de referencias.
    let slot: Arc<Mutex<Option<Operation>>> = Arc::default();
    let announced = Arc::new(AtomicBool::new(false));
    let operation = Operation::new({
        let (slot, announced, reports) =
            (Arc::clone(&slot), Arc::clone(&announced), reports.clone());
        move |state| {
            if state.active && !announced.swap(true, Ordering::SeqCst) {
                let operation = slot.lock().unwrap_or_else(PoisonError::into_inner).clone();
                if let Some(operation) = operation {
                    let _ = reports.send(Report::Started(operation));
                }
            }
        }
    });
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(operation.clone());
    let context = Context {
        engine,
        reports,
        tr,
        operation: &operation,
    };
    let mut current = profile;
    let outcome = catch_unwind(AssertUnwindSafe(|| execute(&context, action, &mut current)));
    let failure = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(context.failure(action, &error)),
        Err(payload) => {
            let text = panic_text(payload.as_ref());
            context.line(
                Message::new("Error: {p1}").arg("p1", &text),
                &format!("Error: {text}"),
                true,
                None,
            );
            Some(Failure {
                cancelled: false,
                integrity: None,
            })
        }
    };
    slot.lock().unwrap_or_else(PoisonError::into_inner).take();
    let _ = reports.send(Report::Done {
        profile: current,
        failure,
    });
}

/// Texto de la carga de un pánico.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "pánico sin mensaje".to_owned())
}

/// Lo que comparten las funciones de una acción.
struct Context<'a> {
    engine: &'a Engine,
    reports: &'a Sender<Report>,
    tr: &'a Translator,
    operation: &'a Operation,
}

impl Context<'_> {
    /// `write_log`: escribe en `logs/mito.log` (`local`, en español) y avisa a la
    /// interfaz con el mensaje sin traducir.
    fn line(&self, message: Message, local: &str, error: bool, long_step: Option<bool>) {
        let level = if error {
            LogLevel::Error
        } else {
            LogLevel::Info
        };
        diagnostics::write(self.engine.layout(), level, "ui", local, &[]);
        let _ = self.reports.send(Report::Log {
            message,
            error,
            long_step,
        });
    }

    /// Línea normal del motor o de la propia acción.
    fn log(&self, line: LogLine) {
        let local = line.message.to_string();
        self.line(line.message, &local, false, line.long_step);
    }

    /// Línea normal con un mensaje.
    fn say(&self, message: Message) {
        self.log(message.into());
    }

    /// Línea de error con un mensaje.
    fn error_line(&self, message: Message) {
        let local = message.to_string();
        self.line(message, &local, true, None);
    }

    /// Pide una confirmación y bloquea hasta la respuesta. Si la interfaz ya no está,
    /// es un «no».
    fn ask(&self, request: Request) -> bool {
        let (answer, response) = channel();
        if self
            .reports
            .send(Report::Confirm { request, answer })
            .is_err()
        {
            return false;
        }
        response.recv().unwrap_or(false)
    }

    /// `confirm(downloads)`: descargas sin hash.
    fn confirm_unverified(&self, downloads: &[String]) -> bool {
        self.ask(Request::Unverified(downloads.to_vec()))
    }

    /// Registra un error de la acción y devuelve lo que la interfaz necesita saber.
    fn failure(&self, action: &Action, error: &Error) -> Failure {
        if error.is_cancelled() {
            self.error_line(Message::new(
                "Instalación cancelada. No se ha publicado nada.",
            ));
            return Failure {
                cancelled: true,
                integrity: None,
            };
        }
        let operation = format!("ui.{}", action.name());
        let options = Options {
            component: action.component(),
            ..Options::operation(&operation)
        };
        error_tracking::capture(self.engine.layout(), error, &options);
        self.line(
            Message::new("Error: {p1}").arg("p1", describe(error, self.tr)),
            &format!("Error: {error}"),
            true,
            None,
        );
        Failure {
            cancelled: false,
            integrity: IntegrityInfo::from_error(error),
        }
    }
}

/// El profile seleccionado, o `Selecciona un profile`.
fn required(profile: Option<&str>) -> Result<&str> {
    profile
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::domain(Message::new("Selecciona un profile")))
}

/// Ejecuta la acción (tabla del worker de ui.md).
fn execute(context: &Context<'_>, action: &Action, profile: &mut Option<String>) -> Result<()> {
    let engine = context.engine;
    let log = |line: LogLine| context.log(line);
    let confirm = |downloads: &[String]| context.confirm_unverified(downloads);
    match action {
        Action::CheckUpdates => {
            refresh_component_updates(context, true);
            context.say(Message::new("Búsqueda de actualizaciones finalizada."));
        }
        Action::StartupUpdates => refresh_component_updates(context, false),
        Action::Create(name) => {
            let created = engine.create_profile(name, None)?;
            context.say(Message::new("Profile {p1} creado").arg("p1", &created));
            *profile = Some(created);
        }
        Action::DeleteProfile => {
            let name = required(profile.as_deref())?.to_owned();
            let next = engine.delete_profile(&name)?;
            context.say(
                Message::new("Profile {p1} eliminado. Se conservan componentes, home y workspace.")
                    .arg("p1", &name),
            );
            *profile = Some(next);
        }
        Action::Toggle {
            installation,
            active,
        } => engine.set_active(required(profile.as_deref())?, installation, *active)?,
        Action::Install(target) => install(context, target, profile.as_deref())?,
        Action::RemoveComponent(key) => engine.remove(key, &log)?,
        Action::UpdateComponent(key) => {
            engine.update(key, Some(&confirm), &log, context.operation)?;
        }
        Action::UpdateAll(keys) => {
            engine.update_all(keys, Some(&confirm), &log, context.operation)?;
        }
        Action::ReleaseVersion(key) => {
            engine.release_version(key, &log)?;
        }
        Action::Launch {
            installation,
            arguments,
            shortcut,
        } => {
            let options = LaunchOptions {
                app: Some(installation),
                arguments: arguments.clone(),
                shortcut: shortcut.as_deref(),
                ..LaunchOptions::default()
            };
            engine.launch(required(profile.as_deref())?, &options)?;
        }
        Action::Refresh => refresh(context)?,
    }
    Ok(())
}

/// `install_from_catalog`: comprueba la clave, refresca su manifiesto y, si la versión
/// subió desde la que vio el usuario, pide confirmación antes de instalar.
fn install(context: &Context<'_>, target: &str, profile: Option<&str>) -> Result<()> {
    let engine = context.engine;
    let catalog = engine.catalog()?;
    let (key, manifest) = catalog.resolve(target)?;
    let previous = manifest
        .raw()
        .get("version")
        .map_or_else(|| "?".to_owned(), py_str);
    context.say(Message::new("Comprobando {p1}…").arg("p1", &key));
    let fetcher = DownloaderFetcher(engine.extensions().downloader.as_ref());
    let refreshed = catalog.refresh_keys(
        [key.as_str()],
        &fetcher,
        context.operation,
        &mut |message| context.say(message),
    );
    if refreshed != 1 {
        // `refresh_keys` ya dejó en el log por qué no pudo comprobarla: no es un error.
        return Ok(());
    }
    let rejected = Cell::new(false);
    let review = |records: &Records| {
        let Some(record) = records.get(&key) else {
            return true;
        };
        let version = record.version_text();
        match (version_key(&previous), version_key(&version)) {
            (Some(old), Some(new)) if new > old => {
                let accepted = context.ask(Request::InstallVersion {
                    key: key.clone(),
                    previous: previous.clone(),
                    version,
                });
                rejected.set(!accepted);
                accepted
            }
            _ => true,
        }
    };
    let confirm = |downloads: &[String]| context.confirm_unverified(downloads);
    let options = InstallOptions {
        profile,
        confirm_plan: Some(&review),
        confirm_unverified: Some(&confirm),
        ..InstallOptions::default()
    };
    let result = engine.install(
        &[key.as_str()],
        &options,
        &|line| context.log(line),
        context.operation,
    );
    match result {
        // El motor responde «Importación cancelada»; aquí el rechazo es de la instalación.
        Err(_) if rejected.get() => Err(Error::domain(Message::new("Instalación cancelada"))),
        other => other.map(|_| ()),
    }
}

/// `refresh`: reimporta cada bucket configurado bajo el lock de mutación.
fn refresh(context: &Context<'_>) -> Result<()> {
    let engine = context.engine;
    let _lock = MutationLock::mutation(engine.layout(), Duration::ZERO)?;
    let catalog = engine.catalog()?;
    let fetcher = DownloaderFetcher(engine.extensions().downloader.as_ref());
    for (bucket, source) in catalog.sources().iter() {
        context.operation.check()?;
        context.say(Message::new("Actualizando {p1}…").arg("p1", bucket));
        let count = catalog.import_bucket(bucket, source, &fetcher, context.operation)?;
        context.say(
            Message::new("{p0}: {p2} manifiestos")
                .arg("p0", bucket)
                .arg("p2", count),
        );
    }
    Ok(())
}

/// `refresh_component_updates`: refresca el manifiesto de cada instalado, como máximo una
/// vez al día salvo `force`. Un fallo se registra y no es un error de la acción.
fn refresh_component_updates(context: &Context<'_>, force: bool) {
    if let Err(error) = try_refresh_component_updates(context, force) {
        if error.is_cancelled() {
            return;
        }
        context.line(
            Message::new("Error al comprobar actualizaciones: {p1}")
                .arg("p1", describe(&error, context.tr)),
            &format!("Error al comprobar actualizaciones: {error}"),
            true,
            None,
        );
    }
}

fn try_refresh_component_updates(context: &Context<'_>, force: bool) -> Result<()> {
    let engine = context.engine;
    let catalog = engine.catalog()?;
    let installed = engine.installed()?;
    let inventory: Map<String, _> = installed
        .iter()
        .map(|(id, record)| (id.to_owned(), record.to_value()))
        .collect();
    let sources: Map<String, _> = catalog
        .sources()
        .iter()
        .map(|(name, source)| (name.to_owned(), json!(source)))
        .collect();
    let identity = json!({"inventory": inventory, "sources": sources});
    let fetcher = DownloaderFetcher(engine.extensions().downloader.as_ref());
    let ran = Cell::new(false);
    let checked = mito_catalog::daily(
        engine.layout(),
        "component-updates",
        &identity,
        force,
        || {
            ran.set(true);
            context.say(Message::new(
                "Comprobando actualizaciones de componentes instalados…",
            ));
            let keys: BTreeSet<&str> = installed.keys().collect();
            Ok::<usize, mito_catalog::Error>(catalog.refresh_installed(
                keys,
                &fetcher,
                context.operation,
                &mut |message| context.say(message),
            ))
        },
    )?;
    if ran.get()
        && let Some(count) = checked
    {
        context.say(
            Message::new("Manifiestos de componentes instalados comprobados: {p1}.")
                .arg("p1", count),
        );
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::assert_is_empty, clippy::needless_pass_by_value)]
mod tests {
    use std::fs;
    use std::sync::mpsc;

    use serde_json::{Value, json};

    use super::*;
    use crate::model::fixture::{Fx, fx, spanish};

    /// Todo lo que la interfaz recibe de una acción.
    #[derive(Default)]
    struct Outcome {
        logs: Vec<(String, bool)>,
        requests: Vec<Request>,
        started: usize,
        profile: Option<String>,
        failure: Option<Failure>,
    }

    impl Outcome {
        fn said(&self, text: &str) -> bool {
            self.logs.iter().any(|(line, _)| line == text)
        }

        fn errors(&self) -> Vec<&str> {
            self.logs
                .iter()
                .filter(|(_, error)| *error)
                .map(|(line, _)| line.as_str())
                .collect()
        }
    }

    /// Ejecuta una acción y contesta a las confirmaciones con `answers` (por defecto sí).
    fn run_action(f: &Fx, action: Action, profile: Option<&str>, answers: &[bool]) -> Outcome {
        let (reports, inbox) = mpsc::channel();
        let handle = spawn_translated(
            Arc::clone(&f.engine),
            action,
            profile.map(str::to_owned),
            reports,
            spanish(),
        );
        let tr = spanish();
        let mut answers = answers.iter().copied();
        let mut outcome = Outcome::default();
        loop {
            match inbox.recv().unwrap() {
                Report::Log { message, error, .. } => outcome.logs.push((tr.tr(&message), error)),
                Report::Confirm { request, answer } => {
                    outcome.requests.push(request);
                    answer.send(answers.next().unwrap_or(true)).unwrap();
                }
                Report::Started(_) => outcome.started += 1,
                Report::Done { profile, failure } => {
                    outcome.profile = profile;
                    outcome.failure = failure;
                    break;
                }
            }
        }
        handle.join().unwrap();
        assert!(inbox.try_recv().is_err(), "Done es el último informe");
        outcome
    }

    fn without_hash(f: &Fx, version: &str) -> Value {
        let mut manifest = f.manifest(version);
        manifest.as_object_mut().unwrap().remove("hash");
        manifest
    }

    /// Catálogo con `main/tool` 1.0 y el origen local ya publicando `version`.
    fn catalog_with_newer(f: &Fx, version: &str) {
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.publish("tool", &f.manifest(version));
    }

    fn installed_version(f: &Fx) -> Option<String> {
        f.engine
            .installed()
            .unwrap()
            .get("main/tool")
            .map(mito_engine::Record::version_text)
    }

    #[test]
    fn instala_con_la_confirmacion_de_una_version_nueva_aceptada() {
        let f = fx();
        catalog_with_newer(&f, "2.0");
        f.engine.create_profile("work", None).unwrap();
        let out = run_action(
            &f,
            Action::Install("main/tool".into()),
            Some("work"),
            &[true],
        );
        assert_eq!(out.failure, None);
        assert_eq!(out.profile.as_deref(), Some("work"));
        assert_eq!(
            out.requests,
            [Request::InstallVersion {
                key: "main/tool".into(),
                previous: "1.0".into(),
                version: "2.0".into()
            }]
        );
        assert!(out.said("Comprobando main/tool…"));
        assert!(out.said("Profile work preparado"));
        assert_eq!(out.errors(), Vec::<&str>::new());
        assert_eq!(installed_version(&f).as_deref(), Some("2.0"));
        assert_eq!(f.engine.profile("work").unwrap().apps, ["main/tool"]);
        let log = fs::read_to_string(f.engine.layout().logs().join("mito.log")).unwrap();
        // El registro es JSON con escapes ASCII (`…` se guarda como `…`).
        assert!(log.contains("\"operation\": \"ui\""), "{log}");
        assert!(log.contains("Comprobando main/tool"), "{log}");
    }

    #[test]
    fn rechazar_la_version_nueva_cancela_sin_instalar() {
        let f = fx();
        catalog_with_newer(&f, "2.0");
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[false]);
        assert_eq!(out.requests.len(), 1);
        assert_eq!(
            out.failure,
            Some(Failure {
                cancelled: false,
                integrity: None
            })
        );
        assert_eq!(out.errors(), ["Error: Instalación cancelada"]);
        assert_eq!(installed_version(&f), None);
    }

    #[test]
    fn la_misma_version_no_pregunta() {
        let f = fx();
        catalog_with_newer(&f, "1.0");
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[]);
        assert_eq!(out.requests, []);
        assert_eq!(out.failure, None);
        assert!(out.said("Componente instalado. Puedes activarlo en Inicio."));
        assert_eq!(installed_version(&f).as_deref(), Some("1.0"));
    }

    #[test]
    fn si_no_se_refresca_una_clave_termina_sin_error() {
        let f = fx();
        catalog_with_newer(&f, "1.0");
        fs::remove_file(f.source.path().join("tool.json")).unwrap();
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[]);
        assert_eq!(out.failure, None);
        assert_eq!(out.requests, []);
        assert_eq!(out.logs.len(), 2, "{:?}", out.logs);
        assert_eq!(out.logs[0].0, "Comprobando main/tool…");
        assert!(
            out.logs[1]
                .0
                .starts_with("No se pudo comprobar main/tool: ")
        );
        assert_eq!(installed_version(&f), None);
    }

    #[test]
    fn las_descargas_sin_hash_piden_confirmacion() {
        let f = fx();
        f.catalog(json!({"tool": without_hash(&f, "1.0")}));
        f.publish("tool", &without_hash(&f, "1.0"));
        let accepted = run_action(&f, Action::Install("main/tool".into()), None, &[true]);
        assert_eq!(accepted.failure, None);
        match accepted.requests.as_slice() {
            [Request::Unverified(downloads)] => {
                assert!(downloads.join("\n").contains("main/tool"), "{downloads:?}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(installed_version(&f).as_deref(), Some("1.0"));

        let g = fx();
        g.catalog(json!({"tool": without_hash(&g, "1.0")}));
        g.publish("tool", &without_hash(&g, "1.0"));
        let rejected = run_action(&g, Action::Install("main/tool".into()), None, &[false]);
        assert!(rejected.failure.is_some());
        assert_eq!(
            rejected.errors(),
            ["Error: Instalación cancelada: descargas sin hash no confirmadas"]
        );
        assert_eq!(installed_version(&g), None);
    }

    #[test]
    fn cancelar_durante_la_descarga_no_publica_nada() {
        let f = fx();
        catalog_with_newer(&f, "1.0");
        f.fake.cancel.lock().unwrap().push("tool.exe".into());
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[]);
        assert_eq!(
            out.failure,
            Some(Failure {
                cancelled: true,
                integrity: None
            })
        );
        assert_eq!(
            out.errors(),
            ["Instalación cancelada. No se ha publicado nada."]
        );
        assert_eq!(out.started, 1);
        assert_eq!(installed_version(&f), None);
    }

    #[test]
    fn una_descarga_alterada_informa_de_la_integridad() {
        let f = fx();
        catalog_with_newer(&f, "1.0");
        f.fake.tamper.lock().unwrap().push("tool.exe".into());
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[]);
        assert_eq!(
            out.failure,
            Some(Failure {
                cancelled: false,
                integrity: Some(IntegrityInfo {
                    url: "https://example.org/tool.exe".into(),
                    size: Some(12),
                    host: "proxy.local".into(),
                    portal: false
                })
            })
        );
        assert_eq!(out.errors(), ["Error: Descarga alterada"]);
    }

    #[test]
    fn un_error_se_registra_y_done_llega_siempre() {
        let f = fx();
        let out = run_action(&f, Action::UpdateComponent("main/nada".into()), None, &[]);
        assert_eq!(
            out.failure,
            Some(Failure {
                cancelled: false,
                integrity: None
            })
        );
        assert_eq!(out.errors(), ["Error: El componente ya no está instalado"]);
        let log = fs::read_to_string(f.engine.layout().logs().join("mito.log")).unwrap();
        assert!(log.contains("ui.update-component"), "{log}");
    }

    #[test]
    fn un_panico_tambien_termina_con_done() {
        let f = fx();
        catalog_with_newer(&f, "1.0");
        f.fake.explode.lock().unwrap().push("tool.exe".into());
        let out = run_action(&f, Action::Install("main/tool".into()), None, &[]);
        assert!(out.failure.is_some());
        assert_eq!(out.errors(), ["Error: descarga con pánico"]);
    }

    #[test]
    fn sin_interfaz_las_confirmaciones_son_un_no() {
        let f = fx();
        let operation = Operation::detached();
        let tr = spanish();
        let (reports, inbox) = mpsc::channel();
        drop(inbox);
        let context = Context {
            engine: &f.engine,
            reports: &reports,
            tr: &tr,
            operation: &operation,
        };
        assert!(!context.ask(Request::Unverified(vec![])));

        let (reports, inbox) = mpsc::channel();
        let listener = thread::spawn(move || {
            if let Ok(Report::Confirm { answer, .. }) = inbox.recv() {
                drop(answer);
            }
        });
        let context = Context {
            engine: &f.engine,
            reports: &reports,
            tr: &tr,
            operation: &operation,
        };
        assert!(!context.ask(Request::Unverified(vec![])));
        listener.join().unwrap();
    }

    #[test]
    fn activar_y_desactivar_solo_cambia_el_profile() {
        let f = fx();
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.engine.create_profile("work", None).unwrap();
        f.install("main/tool", Some("work"));
        let toggle = |active| Action::Toggle {
            installation: "main/tool".into(),
            active,
        };
        let off = run_action(&f, toggle(false), Some("work"), &[]);
        assert_eq!(off.failure, None);
        assert_eq!(f.engine.profile("work").unwrap().apps, Vec::<String>::new());
        assert_eq!(installed_version(&f).as_deref(), Some("1.0"));
        let on = run_action(&f, toggle(true), Some("work"), &[]);
        assert_eq!(on.failure, None);
        assert_eq!(f.engine.profile("work").unwrap().apps, ["main/tool"]);
        let nobody = run_action(&f, toggle(true), None, &[]);
        assert_eq!(nobody.errors(), ["Error: Selecciona un profile"]);
    }

    #[test]
    fn crear_y_borrar_profiles_cambian_el_seleccionado() {
        let f = fx();
        let created = run_action(&f, Action::Create("uno".into()), None, &[]);
        assert_eq!(created.profile.as_deref(), Some("uno"));
        assert!(created.said("Profile uno creado"));
        run_action(&f, Action::Create("dos".into()), Some("uno"), &[]);
        let deleted = run_action(&f, Action::DeleteProfile, Some("dos"), &[]);
        assert_eq!(deleted.failure, None);
        assert_eq!(deleted.profile.as_deref(), Some("uno"));
        assert!(deleted.said("Profile dos eliminado. Se conservan componentes, home y workspace."));
        assert_eq!(f.engine.profiles(), ["uno"]);
        let last = run_action(&f, Action::DeleteProfile, Some("uno"), &[]);
        assert_eq!(
            last.errors(),
            ["Error: No se puede eliminar el último profile"]
        );
        assert_eq!(last.profile.as_deref(), Some("uno"));
    }

    #[test]
    fn quitar_actualizar_y_liberar() {
        let f = fx();
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.install("main/tool", None);
        f.catalog(json!({"tool": f.manifest("2.0")}));
        let updated = run_action(&f, Action::UpdateComponent("main/tool".into()), None, &[]);
        assert_eq!(updated.failure, None);
        assert!(updated.said(
            "Actualizado main/tool: 1.0 → 2.0. Ruta, persistencia y activación conservadas."
        ));
        assert_eq!(installed_version(&f).as_deref(), Some("2.0"));

        f.catalog(json!({"tool": f.manifest("3.0")}));
        let all = run_action(&f, Action::UpdateAll(vec!["main/tool".into()]), None, &[]);
        assert_eq!(all.failure, None);
        assert!(all.said("Componentes actualizados: 1"));
        assert_eq!(installed_version(&f).as_deref(), Some("3.0"));

        let release = run_action(&f, Action::ReleaseVersion("main/tool".into()), None, &[]);
        assert_eq!(release.failure, None);

        let removed = run_action(&f, Action::RemoveComponent("main/tool".into()), None, &[]);
        assert_eq!(removed.failure, None);
        assert_eq!(installed_version(&f), None);
    }

    #[test]
    fn abrir_exige_un_profile() {
        let f = fx();
        let action = Action::Launch {
            installation: "main/tool".into(),
            arguments: vec!["--x".into()],
            shortcut: None,
        };
        let out = run_action(&f, action, None, &[]);
        assert_eq!(out.errors(), ["Error: Selecciona un profile"]);
    }

    #[test]
    fn actualizar_catalogos_reimporta_cada_bucket_configurado() {
        let f = fx();
        f.publish("tool", &f.manifest("1.5"));
        fs::write(
            f.engine.layout().config_file(),
            format!("bucket.main={}\n", f.source.path().display()),
        )
        .unwrap();
        let out = run_action(&f, Action::Refresh, None, &[]);
        assert_eq!(out.failure, None);
        let lines: Vec<&str> = out.logs.iter().map(|(line, _)| line.as_str()).collect();
        assert_eq!(lines, ["Actualizando main…", "main: 1 manifiestos"]);
        let keys: Vec<String> = f
            .engine
            .catalog()
            .unwrap()
            .all()
            .unwrap()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(keys, ["main/tool"]);
    }

    #[test]
    fn la_comprobacion_de_actualizaciones_respeta_la_cache_diaria() {
        let f = fx();
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.install("main/tool", None);
        f.publish("tool", &f.manifest("2.0"));
        let checking = "Comprobando actualizaciones de componentes instalados…";
        let checked = "Manifiestos de componentes instalados comprobados: 1.";

        let startup = run_action(&f, Action::StartupUpdates, None, &[]);
        assert_eq!(startup.failure, None);
        assert!(
            startup.said(checking) && startup.said(checked),
            "{:?}",
            startup.logs
        );
        assert!(!startup.said("Búsqueda de actualizaciones finalizada."));
        assert_eq!(f.engine.pending_updates().unwrap(), ["main/tool"]);

        let cached = run_action(&f, Action::StartupUpdates, None, &[]);
        assert_eq!(cached.logs, []);

        let forced = run_action(&f, Action::CheckUpdates, None, &[]);
        assert!(forced.said(checking) && forced.said(checked));
        assert_eq!(
            forced.logs.last().map(|(line, _)| line.as_str()),
            Some("Búsqueda de actualizaciones finalizada.")
        );
    }

    #[test]
    fn un_fallo_al_comprobar_se_registra_sin_fallar_la_accion() {
        let f = fx();
        f.catalog(json!({"tool": f.manifest("1.0")}));
        f.install("main/tool", None);
        // Un marcador ilegible hace fallar la lectura del inventario.
        let marker = f.engine.layout().apps().join("main").join("tool");
        fs::write(marker.join(".mito.json"), "no es json").unwrap();
        let out = run_action(&f, Action::CheckUpdates, None, &[]);
        assert_eq!(out.failure, None);
        assert_eq!(out.errors().len(), 1, "{:?}", out.logs);
        assert!(out.errors()[0].starts_with("Error al comprobar actualizaciones: "));
        assert!(out.said("Búsqueda de actualizaciones finalizada."));
    }
}
