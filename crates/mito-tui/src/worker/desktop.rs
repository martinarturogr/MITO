//! Acciones de F8 del hilo de trabajo: accesos directos, aplicaciones del host, enlaces
//! de datos y de caché, exposición de `AppData` y «Abrir con».
//!
//! Siguen la «Tabla de acciones del worker» de `ui.md` y llaman a `mito-desktop`. Todo lo
//! que toca el sistema pasa por [`Integration`](crate::integration::Integration).

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use mito_core::{Message, MutationLock};
use mito_desktop::host_data::{self, HostRoots, Kind};
use mito_desktop::shortcuts::{self, LinkOptions};
use mito_desktop::{host_apps, host_exposure, open_with};
use mito_engine::{Error, LogLine, Result};

use super::{Action, Context, Report, Request, required};

/// Respuesta del diálogo «AGREGAR APLICACIÓN DEL HOST».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Se cerró sin elegir.
    Cancel,
    /// «Examinar…»: abrir el selector nativo de archivo.
    Browse,
    /// La aplicación elegida.
    Path(PathBuf),
}

impl Context<'_> {
    /// `confirm_host_data`: pregunta con `HostDataConfirm` y espera la respuesta.
    fn confirm_host(&self, message: Message, label: &'static str) -> bool {
        self.ask(Request::HostConfirm { message, label })
    }

    /// `choose_host_application`: bloquea hasta que la interfaz elige.
    fn pick(&self, found: Vec<(String, PathBuf)>) -> Pick {
        let (answer, response) = channel();
        if self.reports.send(Report::Pick { found, answer }).is_err() {
            return Pick::Cancel;
        }
        response.recv().unwrap_or(Pick::Cancel)
    }

    /// Traducción de las etiquetas de los selectores nativos.
    fn translate(&self, key: &str) -> String {
        self.tr.text(key).to_owned()
    }

    fn roots(&self) -> Result<HostRoots> {
        (self.integration.host)()
    }

    /// Candado de mutación como el de la CLI.
    fn mutation(&self) -> Result<MutationLock> {
        Ok(MutationLock::mutation(
            self.engine.layout(),
            Duration::ZERO,
        )?)
    }

    fn desktop(&self) -> Result<PathBuf> {
        (self.integration.desktop)()
    }
}

/// Ejecuta una acción de F8.
#[allow(clippy::too_many_lines, reason = "una rama por acción de F8")]
pub(super) fn execute(context: &Context<'_>, action: &Action, profile: Option<&str>) -> Result<()> {
    match action {
        Action::HostAdd => host_add(context),
        Action::HostLink { identity, kind } => host_link(context, identity, *kind),
        Action::OpenWith {
            installation,
            wanted,
        } => {
            let log = |line: LogLine| context.log(line);
            open_with::apply(
                &context.integration.classes,
                context.engine,
                required(profile)?,
                installation,
                wanted,
                &log,
            )
        }
        Action::HostExpose(installation) => host_expose(context, installation),
        Action::HostOpen {
            identity,
            arguments,
            coupled,
        } => {
            (context.integration.launch_host)(
                context.engine,
                required(profile)?,
                identity,
                arguments,
                *coupled,
            )?;
            Ok(())
        }
        Action::HostRemove(identity) => {
            host_apps::remove(context.engine.root(), &context.roots()?, identity)?;
            context.say(Message::new(
                "Referencia eliminada. El ejecutable y los datos del host se conservan.",
            ));
            Ok(())
        }
        Action::HostShortcut {
            identity,
            badge,
            color,
            coupled,
            arguments,
        } => host_shortcut(
            context,
            required(profile)?,
            identity,
            &LinkOptions {
                badge: Some(badge),
                color: Some(color),
                coupled: *coupled,
                arguments,
                shortcut: None,
            },
        ),
        Action::MitoShortcut { badge, color } => {
            let _lock = context.mutation()?;
            let path = shortcuts::mito_link(
                context.engine,
                &|| context.desktop(),
                required(profile)?,
                Some(badge),
                Some(color),
            )?;
            context.say(
                Message::new("Acceso a MITO creado en el escritorio: {p1}")
                    .arg("p1", path.display()),
            );
            Ok(())
        }
        Action::ApplicationShortcut {
            installation,
            badge,
            color,
            coupled,
            arguments,
            shortcut,
        } => {
            let _lock = context.mutation()?;
            let path = shortcuts::application_link(
                context.engine,
                &|| context.desktop(),
                required(profile)?,
                installation,
                &LinkOptions {
                    badge: Some(badge),
                    color: Some(color),
                    coupled: *coupled,
                    arguments,
                    shortcut: shortcut.as_deref(),
                },
            )?;
            context.say(
                Message::new("Acceso directo creado en el escritorio: {p1}")
                    .arg("p1", path.display()),
            );
            Ok(())
        }
        // Las demás acciones las resuelve el núcleo del worker.
        _ => Ok(()),
    }
}

/// `shortcut` de `host_apps.py`: valida el archivo antes de tocar el escritorio.
fn host_shortcut(
    context: &Context<'_>,
    profile: &str,
    identity: &str,
    options: &LinkOptions<'_>,
) -> Result<()> {
    let root = context.engine.root();
    let executable = host_apps::target(root, identity)?;
    if !executable.is_file() {
        return Err(Error::domain(Message::new(
            "El archivo de la aplicación del host ya no está disponible",
        )));
    }
    let _lock = context.mutation()?;
    let path = shortcuts::host_link(
        context.engine,
        &|| context.desktop(),
        profile,
        identity,
        &executable,
        options,
    )?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    context.say(Message::new("Acceso directo al host creado: {p1}").arg("p1", name));
    Ok(())
}

/// `host-add`: busca aplicaciones, deja elegir una, la registra y ofrece enlazar sus
/// carpetas de datos y de caché.
fn host_add(context: &Context<'_>) -> Result<()> {
    let root = context.engine.root();
    context.say(Message::new(
        "Buscando aplicaciones en el menú Inicio y el escritorio del host…",
    ));
    let found = match (context.integration.discover)(root) {
        Ok(found) => found,
        Err(error) => {
            context.say(
                Message::new("No se pudieron revisar los accesos del host: {p1}")
                    .arg("p1", crate::model::describe(&error, context.tr)),
            );
            Vec::new()
        }
    };
    let pick = if found.is_empty() {
        Pick::Browse
    } else {
        context.pick(found)
    };
    let file = match pick {
        Pick::Cancel => None,
        Pick::Path(path) => Some(path),
        Pick::Browse => (context.integration.choose_file)(&|key| context.translate(key))?,
    };
    let Some(file) = file else {
        return Ok(());
    };
    let identity = host_apps::add(root, &file)?;
    context.say(Message::new(
        "Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado.",
    ));
    let roots = context.roots()?;
    for kind in Kind::ALL {
        if host_data::entries(root, &roots, &identity, kind)?.is_empty() {
            context.say(Message::new("Buscando carpetas del host: {p1}…").arg("p1", kind.key()));
            for folder in host_data::candidates(root, &roots, &identity, kind)? {
                offer_host_data(context, &roots, &identity, kind, &folder)?;
            }
        }
    }
    Ok(())
}

/// `offer_host_data`: pregunta si enlazar `folder` y, si la carpeta de MITO existe y no
/// es ya ese enlace, si se puede borrar.
fn offer_host_data(
    context: &Context<'_>,
    roots: &HostRoots,
    identity: &str,
    kind: Kind,
    folder: &Path,
) -> Result<()> {
    let root = context.engine.root();
    let (path, folder, _) = host_data::plan(root, roots, kind, folder)?;
    let label = match kind {
        Kind::Data => "Host data",
        Kind::Cache => "Host cache",
    };
    let question = Message::new(
        "{p0}: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nCompartido por todos los profiles. La aplicación podrá modificar los datos del host. Cierra la aplicación antes de enlazar.",
    )
    .arg("p0", context.tr.text(label))
    .arg("p2", folder.display())
    .arg("p4", path.display());
    if !context.confirm_host(question, "Enlazar") {
        return Ok(());
    }
    let replace = path.exists() && !host_exposure::points_to(&path, &folder);
    if replace {
        let warning = Message::new(
            "La carpeta {p1} ya existe y ha de ser borrada para generar el enlace.\nSe perderá su contenido local en MITO.",
        )
        .arg("p1", path.display());
        if !context.confirm_host(warning, "Borrar y enlazar") {
            return Ok(());
        }
    }
    host_data::link(root, roots, identity, kind, &folder, replace)?;
    Ok(())
}

/// `host-cache` y `host-data`: desenlaza si hay enlaces; si no, busca carpetas, las
/// ofrece y, si no queda ninguna, deja elegir una a mano.
fn host_link(context: &Context<'_>, identity: &str, kind: Kind) -> Result<()> {
    let root = context.engine.root();
    let roots = context.roots()?;
    let current = host_data::entries(root, &roots, identity, kind)?;
    if !current.is_empty() {
        let paths: Vec<&str> = current.iter().map(|item| item.target.as_str()).collect();
        let question = Message::new(
            "¿Eliminar los enlaces de Host {p1}?\n{p3}\nLos datos del host se conservan.",
        )
        .arg("p1", kind.key())
        .arg("p3", paths.join("\n"));
        if context.confirm_host(question, "Desenlazar") {
            host_data::unlink(root, &roots, identity, kind)?;
        }
        return Ok(());
    }
    context.say(Message::new("Buscando carpetas del host: {p1}…").arg("p1", kind.key()));
    let found = host_data::candidates(root, &roots, identity, kind)?;
    for folder in &found {
        offer_host_data(context, &roots, identity, kind, folder)?;
    }
    if host_data::entries(root, &roots, identity, kind)?.is_empty() {
        let manual = found.is_empty()
            || context.confirm_host(
                Message::new("¿Seleccionar otra carpeta del host manualmente?"),
                "Seleccionar",
            );
        if manual
            && let Some(folder) = (context.integration.choose_folder)(roots.folder(kind), &|key| {
                context.translate(key)
            })?
        {
            offer_host_data(context, &roots, identity, kind, &folder)?;
        }
    }
    Ok(())
}

/// `host-expose`: retira la exposición si existe; si no, ofrece cada carpeta del
/// `AppData` de MITO que coincide con el ejecutable.
fn host_expose(context: &Context<'_>, installation: &str) -> Result<()> {
    let engine = context.engine;
    let root = engine.root();
    let roots = context.roots()?;
    let current = host_exposure::entries(root, installation)?;
    if !current.is_empty() {
        let paths: Vec<String> = current
            .iter()
            .map(|(kind, item)| {
                roots
                    .folder(*kind)
                    .join(item.relative.replace('/', "\\"))
                    .display()
                    .to_string()
            })
            .collect();
        let question =
            Message::new("¿Eliminar los enlaces del host?\n{p1}\nLos datos de MITO se conservan.")
                .arg("p1", paths.join("\n"));
        if context.confirm_host(question, "Desenlazar") {
            host_exposure::unlink(root, &roots, installation)?;
            context.say(Message::new(
                "Enlaces del host retirados. Los datos de MITO se conservan.",
            ));
        }
        return Ok(());
    }
    let record = engine
        .installed()?
        .get(installation)
        .cloned()
        .ok_or_else(|| Error::domain(Message::new("El componente ya no está instalado")))?;
    let found = host_exposure::candidates(engine, &record)?;
    if found.is_empty() {
        context.say(Message::new(
            "No hay carpetas en el AppData de MITO que coincidan con el ejecutable. Abre antes la aplicación desde MITO.",
        ));
    }
    for (kind, relative) in found {
        let (source, host) = host_exposure::plan(root, &roots, kind, &relative)?;
        let question = Message::new(
            "{p0} en el host: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nSi la aplicación arranca fuera de MITO (inicio automático, asociaciones) usará estos datos, pero no el entorno del profile. Cierra la aplicación antes de enlazar.",
        )
        .arg("p0", kind.folder())
        .arg("p2", host.display())
        .arg("p4", source.display());
        if !context.confirm_host(question, "Enlazar") {
            continue;
        }
        let aside = (host.exists() || mito_win::links::is_junction(&host))
            && !host_exposure::points_to(&host, &source);
        if aside {
            let backup = host_exposure::backup_path(&host);
            let name = backup
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let warning = Message::new(
                "La carpeta del host {p1} ya existe. Se renombrará a {p3} y se restaurará al desenlazar.",
            )
            .arg("p1", host.display())
            .arg("p3", name);
            if !context.confirm_host(warning, "Apartar y enlazar") {
                continue;
            }
        }
        host_exposure::link(engine, &roots, installation, kind, &relative, aside)?;
        context.say(Message::new("Enlace creado en el host: {p1}").arg("p1", host.display()));
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    reason = "pruebas"
)]
mod tests;
