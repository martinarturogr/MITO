//! Fachada del motor sobre una instancia.

use std::fs;
use std::path::Path;
use std::time::Duration;

use mito_catalog::{BucketSources, Catalog};
use mito_core::storage::inside;
use mito_core::{Layout, MutationLock};

use crate::error::{Error, Result};
use crate::extensions::Extensions;

/// Carpetas que crea [`Engine::initialize`], en orden.
const FOLDERS: [&str; 11] = [
    "home",
    "home/AppData/Roaming",
    "home/AppData/Local",
    "home/tmp",
    "home/custom-commands",
    "profiles",
    "workspaces",
    "core/cache",
    "core/staging",
    "core/state",
    "core/apps",
];

/// Carpetas conocidas de Windows dentro de `home/` (`USER_FOLDERS`). Sin ellas
/// `SHGetKnownFolderPath` falla y, por ejemplo, Electron aborta con
/// `Failed to get 'downloads' path`.
pub const USER_FOLDERS: [&str; 12] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Music",
    "Pictures",
    "Videos",
    "Favorites",
    "Links",
    "Contacts",
    "Searches",
    "Saved Games",
    "AppData/LocalLow",
];

/// Motor de una instancia MITO: profiles, inventario, entorno, instalación y lanzamiento.
///
/// Los métodos tienen los mismos nombres que los del `Engine` del original. Las
/// mutaciones toman el lock global de `core/state/mutation.lock` y no son reentrantes:
/// no se debe llamar a un método público que muta desde dentro de otro.
#[derive(Debug)]
pub struct Engine {
    layout: Layout,
    extensions: Extensions,
    lock_wait: Duration,
}

impl Engine {
    /// Motor de la instancia con raíz en `root` (se resuelve a su ruta canónica si
    /// existe) y los puntos de extensión por defecto (ADR 0009).
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self::with_extensions(root, Extensions::default())
    }

    /// Motor con puntos de extensión propios.
    pub fn with_extensions(root: impl AsRef<Path>, extensions: Extensions) -> Self {
        let root = root.as_ref();
        let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Self {
            layout: Layout::new(root),
            extensions,
            lock_wait: Duration::ZERO,
        }
    }

    /// Espera máxima por un lock de mutación ocupado (por defecto 0: falla en el acto).
    #[must_use]
    pub fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    /// Carpetas de la instancia.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Raíz de la instancia.
    pub fn root(&self) -> &Path {
        self.layout.root()
    }

    /// Puntos de extensión.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    /// Puntos de extensión modificables (para sustituir uno tras construir el motor).
    pub fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    /// Catálogo de la instancia con los orígenes `bucket.<nombre>` de
    /// `config.properties` (ADR 0008, punto 10).
    ///
    /// # Errors
    ///
    /// Los de lectura de la configuración y de nombres de bucket no válidos.
    pub fn catalog(&self) -> Result<Catalog> {
        let path = self.layout.config_file();
        let text = match fs::read(&path) {
            Ok(bytes) => {
                let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
                String::from_utf8_lossy(bytes).into_owned()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::io(&path, e)),
        };
        let sources = BucketSources::from_properties(&text)?;
        Ok(Catalog::new(self.layout.clone(), sources))
    }

    /// Adquiere el lock global de mutaciones esperando `lock_wait`.
    pub(crate) fn mutation(&self) -> Result<MutationLock> {
        Ok(MutationLock::mutation(&self.layout, self.lock_wait)?)
    }

    /// Punto de fallo de pruebas (ver [`crate::FaultHook`]).
    pub(crate) fn fault(&self, point: &str, subject: &str) -> Result<()> {
        self.extensions.fault(point, subject)
    }

    /// Crea las carpetas de trabajo de la instancia y las carpetas conocidas de
    /// Windows dentro de `home/`.
    ///
    /// # Errors
    ///
    /// Errores de E/S o rutas fuera de la raíz.
    pub fn initialize(&self) -> Result<()> {
        for relative in FOLDERS {
            let path = inside(self.root(), relative)?;
            fs::create_dir_all(&path).map_err(|e| Error::io(&path, e))?;
        }
        let home = self.layout.home();
        for relative in USER_FOLDERS {
            let path = inside(&home, relative)?;
            fs::create_dir_all(&path).map_err(|e| Error::io(&path, e))?;
        }
        Ok(())
    }
}
