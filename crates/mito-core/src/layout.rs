//! Estructura de carpetas de una instancia MITO (compatible con el original, ADR 0003).

use std::path::{Path, PathBuf};

/// Rutas de una instancia a partir de su raíz.
///
/// El ejecutable vive en `<raíz>\core\bin\mito.exe` (ADR 0008) y la raíz se deduce de
/// su ruta. Las rutas se calculan; ninguna función de este tipo toca el disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    /// Instancia con raíz en `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Deduce la instancia desde la ruta de `core\bin\mito.exe`. Devuelve `None` si la
    /// ruta no tiene tres niveles de carpetas por encima.
    pub fn from_executable(executable: &Path) -> Option<Self> {
        executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(Self::new)
    }

    /// Raíz de la instancia.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `config.properties`.
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.properties")
    }

    /// `profiles/` (un `<name>.json` por profile).
    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// `workspaces/`.
    pub fn workspaces(&self) -> PathBuf {
        self.root.join("workspaces")
    }

    /// `home/`, raíz única de HOME, USERPROFILE y APPDATA.
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// `logs/`.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `pocketools/`.
    pub fn pocketools(&self) -> PathBuf {
        self.root.join("pocketools")
    }

    /// `update/` (releases offline).
    pub fn update(&self) -> PathBuf {
        self.root.join("update")
    }

    /// `core/`.
    pub fn core(&self) -> PathBuf {
        self.root.join("core")
    }

    /// `core/bin/` (ejecutable de MITO).
    pub fn bin(&self) -> PathBuf {
        self.core().join("bin")
    }

    /// `core/state/`.
    pub fn state(&self) -> PathBuf {
        self.core().join("state")
    }

    /// `core/apps/` (`<bucket>/<app>`).
    pub fn apps(&self) -> PathBuf {
        self.core().join("apps")
    }

    /// `core/profile-apps/` (`<profile>/<bucket>/<app>`).
    pub fn profile_apps(&self) -> PathBuf {
        self.core().join("profile-apps")
    }

    /// `core/staging/`.
    pub fn staging(&self) -> PathBuf {
        self.core().join("staging")
    }

    /// `core/buckets/` (snapshots `<bucket>.json`).
    pub fn buckets(&self) -> PathBuf {
        self.core().join("buckets")
    }

    /// `core/tools/` (7-Zip, innounp y Windows Terminal privados).
    pub fn tools(&self) -> PathBuf {
        self.core().join("tools")
    }

    /// `core/commands/`.
    pub fn commands(&self) -> PathBuf {
        self.core().join("commands")
    }

    /// `core/bootstrap.lock.json`: herramientas privadas fijadas por SHA-256 (ADR 0010).
    pub fn bootstrap_lock(&self) -> PathBuf {
        self.core().join("bootstrap.lock.json")
    }

    /// `core/assets/branding/mito.ico`: icono de MITO.
    pub fn branding_icon(&self) -> PathBuf {
        self.core().join("assets").join("branding").join("mito.ico")
    }

    /// `core/tools/7zip/7z.exe`: extractor privado de 7z, RAR y CAB.
    pub fn sevenzip_exe(&self) -> PathBuf {
        self.tools().join("7zip").join("7z.exe")
    }

    /// `core/tools/innounp/innounp.exe`: extractor privado de Inno Setup.
    pub fn innounp_exe(&self) -> PathBuf {
        self.tools().join("innounp").join("innounp.exe")
    }

    /// `core/tools/windows-terminal/`: Windows Terminal portable privado.
    pub fn windows_terminal(&self) -> PathBuf {
        self.tools().join("windows-terminal")
    }

    /// `core/tools/windows-terminal/WindowsTerminal.exe`: el ejecutable verificado.
    pub fn windows_terminal_exe(&self) -> PathBuf {
        self.windows_terminal().join("WindowsTerminal.exe")
    }

    /// `core/tools/windows-terminal/YitoTerminal.exe`: copia con el icono de MITO.
    pub fn branded_terminal_exe(&self) -> PathBuf {
        self.windows_terminal().join("YitoTerminal.exe")
    }

    /// `core/bin/mito.exe`: el ejecutable de MITO de esta instancia.
    pub fn mito_exe(&self) -> PathBuf {
        self.bin().join("mito.exe")
    }

    /// `core/bin/mitow.exe`: el mismo MITO sin consola, destino de los accesos
    /// directos, «Abrir con» y protocolos (ADR 0013).
    pub fn mitow_exe(&self) -> PathBuf {
        self.bin().join("mitow.exe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduce_la_raiz_desde_el_ejecutable() {
        let exe = Path::new(r"C:\mito\core\bin\mito.exe");
        let layout = Layout::from_executable(exe).unwrap();
        assert_eq!(layout.root(), Path::new(r"C:\mito"));
        assert_eq!(layout.buckets(), Path::new(r"C:\mito\core\buckets"));
        assert_eq!(
            layout.config_file(),
            Path::new(r"C:\mito\config.properties")
        );
        assert!(Layout::from_executable(Path::new("mito.exe")).is_none());
    }
}
