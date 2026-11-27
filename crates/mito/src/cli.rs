//! Definición de la línea de comandos (`argparse` de `__main__.py`).
//!
//! Los nombres de subcomandos y opciones son los del original. Los de fases posteriores
//! se analizan igual, pero el programa responde que todavía no están disponibles.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Tu entorno de desarrollo en una carpeta.
#[derive(Debug, Parser)]
#[command(
    name = "mito",
    bin_name = "mito",
    version,
    about = "Tu entorno de desarrollo en una carpeta"
)]
pub struct Cli {
    /// Raíz de la instancia (solo para pruebas; por defecto, la deducida del ejecutable).
    #[arg(long, global = true, hide = true, value_name = "RUTA")]
    pub root: Option<PathBuf>,

    /// Subcomando; sin él se abre la interfaz (`ui`).
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Subcomandos de `mito`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Abrir la interfaz Textual
    Ui {
        /// Profile con el que abrir
        #[arg(long)]
        profile: Option<String>,
    },
    /// Abrir MITO en Windows Terminal portable
    Start {
        /// Profile con el que abrir
        #[arg(long)]
        profile: Option<String>,
    },
    /// Preparar herramientas privadas
    Bootstrap,
    /// Listar todos los componentes instalados
    Installed,
    /// Mostrar las carpetas de diagnóstico local
    Logs,
    /// Ejecutar un comando Pocketool en una pestaña de MITO
    PocketoolRun(PocketoolRun),
    /// Abrir un ejecutable externo con un profile de MITO
    HostRun(HostRun),
    /// Comprobar runtime y estado
    Doctor,
    /// Servidor MCP (uso interno)
    #[command(hide = true)]
    McpServer(McpServer),
    /// Convertir instalaciones antiguas a rutas estables, con las aplicaciones cerradas
    MigrateStorage,
    /// Gestionar buckets
    #[command(subcommand)]
    Bucket(BucketCommand),
    /// Buscar componentes en el catálogo
    Search {
        /// Texto a buscar
        #[arg(default_value = "")]
        query: String,
        /// Mostrar solo los componentes compatibles
        #[arg(long)]
        compatible: bool,
    },
    /// Gestionar profiles
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Instalar un componente y sus dependencias
    Install {
        /// Componente (`bucket/app` o `app`)
        app: String,
        /// Profile en el que activarlo
        #[arg(long)]
        profile: Option<String>,
    },
    /// Activar un componente instalado en un profile
    Activate(Toggle),
    /// Desactivar un componente en un profile
    Deactivate(Toggle),
    /// Abrir una shell (o ejecutar un comando) en el entorno de un profile
    Shell {
        /// Profile
        #[arg(long)]
        profile: String,
        /// Intérprete de comandos
        #[arg(long = "type", value_name = "TYPE", value_enum, default_value_t = ShellType::Cmd)]
        shell_type: ShellType,
        /// Comando y argumentos (tras `--`)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Abrir la terminal de un profile
    Terminal {
        /// Profile
        #[arg(long)]
        profile: String,
        /// Intérprete de comandos
        #[arg(long = "type", value_name = "TYPE", value_enum, default_value_t = ShellType::Cmd)]
        shell_type: ShellType,
        /// Abrir en una ventana nueva
        #[arg(long)]
        new: bool,
    },
    /// Ejecutar una aplicación o un comando de un componente
    Run(Run),
    /// Crear un acceso de aplicación en el escritorio
    AppShortcut(AppShortcut),
    /// Acceso a MITO en el escritorio
    Shortcut {
        /// Profile
        #[arg(long)]
        profile: String,
        /// Texto de la insignia
        #[arg(long)]
        badge: Option<String>,
        /// Color de la insignia
        #[arg(long)]
        color: Option<String>,
    },
    /// Incrementar versión y publicar en GitHub
    Release(Release),
}

impl Command {
    /// Nombre del subcomando tal como se escribe (`cli.<nombre>` en el log).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ui { .. } => "ui",
            Self::Start { .. } => "start",
            Self::Bootstrap => "bootstrap",
            Self::Installed => "installed",
            Self::Logs => "logs",
            Self::PocketoolRun(_) => "pocketool-run",
            Self::HostRun(_) => "host-run",
            Self::Doctor => "doctor",
            Self::McpServer(_) => "mcp-server",
            Self::MigrateStorage => "migrate-storage",
            Self::Bucket(_) => "bucket",
            Self::Search { .. } => "search",
            Self::Profile(_) => "profile",
            Self::Install { .. } => "install",
            Self::Activate(_) => "activate",
            Self::Deactivate(_) => "deactivate",
            Self::Shell { .. } => "shell",
            Self::Terminal { .. } => "terminal",
            Self::Run(_) => "run",
            Self::AppShortcut(_) => "app-shortcut",
            Self::Shortcut { .. } => "shortcut",
            Self::Release(_) => "release",
        }
    }
}

/// Argumentos de `pocketool-run`.
#[derive(Debug, Args)]
pub struct PocketoolRun {
    /// Profile
    #[arg(long)]
    pub profile: String,
    /// Paquete de Pocketool
    #[arg(long)]
    pub package: String,
    /// Punto de entrada
    #[arg(long)]
    pub entry: String,
    /// Esperar al proceso y pausar al terminar
    #[arg(long, hide = true)]
    pub owned: bool,
    /// Argumentos codificados en base64
    #[arg(long, hide = true)]
    pub arguments64: Option<String>,
    /// Argumentos del comando (tras `--`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

/// Argumentos de `host-run`.
#[derive(Debug, Args)]
pub struct HostRun {
    /// Identidad del ejecutable externo
    pub identity: String,
    /// Profile
    #[arg(long)]
    pub profile: String,
    /// Acoplar a una pestaña de la terminal
    #[arg(long, conflicts_with = "owned")]
    pub coupled: bool,
    /// Esperar al proceso y pausar al terminar
    #[arg(long, hide = true)]
    pub owned: bool,
    /// Argumentos del ejecutable (tras `--`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

/// Argumentos de `mcp-server`.
#[derive(Debug, Args)]
pub struct McpServer {
    /// Canal de comunicación
    #[arg(long)]
    pub pipe: String,
    /// Proceso propietario
    #[arg(long)]
    pub owner: i64,
    /// Sesión
    #[arg(long)]
    pub session: String,
}

/// Acciones de `bucket`.
#[derive(Debug, Subcommand)]
pub enum BucketCommand {
    /// Importar un bucket desde un repositorio GitHub o una carpeta local
    Import {
        /// Nombre del bucket
        name: String,
        /// Repositorio GitHub HTTPS o carpeta local
        source: String,
    },
    /// Reimportar los buckets configurados
    Refresh {
        /// Solo este bucket
        name: Option<String>,
    },
}

/// Acciones de `profile`.
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Crear un profile
    Create {
        /// Nombre
        name: String,
        /// Workspace (por defecto, el nombre del profile)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Eliminar un profile
    Delete {
        /// Nombre
        name: String,
    },
    /// Exportar un profile
    Export {
        /// Nombre
        name: String,
        /// Archivo de salida
        output: String,
    },
    /// Importar un profile
    Import {
        /// Archivo de origen
        source: String,
        /// Nombre del nuevo profile
        #[arg(long)]
        name: String,
    },
    /// Listar los profiles
    List,
    /// Mostrar un profile en JSON
    Show {
        /// Nombre
        name: String,
    },
}

/// Argumentos de `activate` y `deactivate`.
#[derive(Debug, Args)]
pub struct Toggle {
    /// ID que muestra mito installed
    pub installation: String,
    /// Profile
    #[arg(long)]
    pub profile: String,
}

/// Intérprete de `shell` y `terminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellType {
    // `cmd.exe`
    Cmd,
    // Windows PowerShell
    Powershell,
}

/// Argumentos de `run`.
#[derive(Debug, Args)]
pub struct Run {
    /// Profile
    #[arg(long)]
    pub profile: String,
    /// Componente (`bucket/app` o `app`)
    #[arg(long)]
    pub app: String,
    /// Alias de un `bin` en lugar de la aplicación
    #[arg(long)]
    pub bin: Option<String>,
    /// Etiqueta del acceso declarado
    #[arg(long)]
    pub shortcut: Option<String>,
    /// Acoplar a una pestaña de la terminal
    #[arg(long, conflicts_with = "owned")]
    pub coupled: bool,
    /// Esperar al proceso y pausar al terminar
    #[arg(long, hide = true)]
    pub owned: bool,
    /// Argumentos de la aplicación (tras `--`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

/// Argumentos de `app-shortcut`.
#[derive(Debug, Args)]
pub struct AppShortcut {
    /// ID de instalación
    pub installation: String,
    /// Profile
    #[arg(long)]
    pub profile: String,
    /// Texto de la insignia
    #[arg(long)]
    pub badge: Option<String>,
    /// Color de la insignia
    #[arg(long)]
    pub color: Option<String>,
    /// Etiqueta del acceso declarado
    #[arg(long)]
    pub shortcut: Option<String>,
    /// Acoplar a una pestaña de la terminal
    #[arg(long)]
    pub coupled: bool,
    /// Argumentos de la aplicación (tras `--`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

/// Tipo de publicación de `release`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReleaseKind {
    // X.0.1
    Mayor,
    // M.X.1
    Minor,
    // M.m.X
    Hotfix,
}

/// Argumentos de `release`.
#[derive(Debug, Args)]
pub struct Release {
    /// mayor: X.0.1; minor: M.X.1; hotfix: M.m.X (defecto)
    #[arg(value_enum)]
    pub kind: Option<ReleaseKind>,
    /// Sólo generar ZIP local, sin incrementar ni publicar
    #[arg(long)]
    pub local: bool,
    /// Directorio de salida (defecto: releases)
    #[arg(long)]
    pub output: Option<String>,
}

impl Cli {
    /// Validaciones que `argparse` hace con `parser.error` tras analizar: `release
    /// --local` no admite `kind`. Devuelve el texto del error.
    pub fn validate(&self) -> Result<(), &'static str> {
        match &self.command {
            Some(Command::Release(release)) if release.local && release.kind.is_some() => {
                Err("--local no admite mayor, minor ni hotfix: empaqueta la versión actual")
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("mito").chain(args.iter().copied()))
    }

    #[test]
    fn la_definicion_de_la_cli_es_valida() {
        Cli::command().debug_assert();
    }

    #[test]
    fn sin_subcomando_es_ui() {
        let cli = parse(&[]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn run_reenvia_los_argumentos_tras_el_separador() {
        let cli = parse(&[
            "run",
            "--profile",
            "one",
            "--app",
            "extras/tool",
            "--shortcut",
            "Tool Help",
            "--",
            "--name",
            "two words",
        ])
        .unwrap();
        let Some(Command::Run(run)) = cli.command else {
            panic!("se esperaba run");
        };
        assert_eq!(run.arguments, ["--name", "two words"]);
        assert_eq!(run.shortcut.as_deref(), Some("Tool Help"));
        assert!(!run.coupled && !run.owned);
    }

    #[test]
    fn host_run_consume_el_separador() {
        let cli = parse(&[
            "host-run",
            "host-id",
            "--profile",
            "one",
            "--owned",
            "--",
            "--port",
            "8080",
        ])
        .unwrap();
        let Some(Command::HostRun(host)) = cli.command else {
            panic!("se esperaba host-run");
        };
        assert_eq!(host.arguments, ["--port", "8080"]);
        assert!(host.owned && !host.coupled);
    }

    #[test]
    fn app_shortcut_acepta_modo_y_argumentos() {
        let cli = parse(&[
            "app-shortcut",
            "installation",
            "--profile",
            "one",
            "--badge",
            "DEV",
            "--coupled",
            "--",
            "--name",
            "two words",
        ])
        .unwrap();
        let Some(Command::AppShortcut(link)) = cli.command else {
            panic!("se esperaba app-shortcut");
        };
        assert_eq!(link.badge.as_deref(), Some("DEV"));
        assert!(link.coupled);
        assert_eq!(link.arguments, ["--name", "two words"]);
    }

    #[test]
    fn coupled_y_owned_son_excluyentes() {
        let error = parse(&[
            "run",
            "--profile",
            "p",
            "--app",
            "a",
            "--coupled",
            "--owned",
        ])
        .unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn el_tipo_de_shell_solo_admite_cmd_y_powershell() {
        assert!(parse(&["shell", "--profile", "p", "--type", "bash"]).is_err());
        let cli = parse(&["shell", "--profile", "p", "--type", "powershell"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Shell {
                shell_type: ShellType::Powershell,
                ..
            })
        ));
    }

    #[test]
    fn release_valida_antes_de_construir_el_motor() {
        for arguments in [["invalid"].as_slice(), &["mayor", "1.0.1"]] {
            let mut full = vec!["release"];
            full.extend_from_slice(arguments);
            assert_eq!(parse(&full).unwrap_err().exit_code(), 2);
        }
        for arguments in [
            ["--local", "mayor"],
            ["minor", "--local"],
            ["hotfix", "--local"],
        ] {
            let mut full = vec!["release"];
            full.extend_from_slice(&arguments);
            assert!(parse(&full).unwrap().validate().is_err());
        }
        assert!(parse(&["release", "--local"]).unwrap().validate().is_ok());
        assert!(parse(&["release", "minor"]).unwrap().validate().is_ok());
    }

    #[test]
    fn bucket_y_profile_exigen_accion() {
        assert!(parse(&["bucket"]).is_err());
        assert!(parse(&["profile"]).is_err());
        assert!(parse(&["posthog-check"]).is_err());
    }

    #[test]
    fn mcp_server_esta_oculto_pero_se_analiza() {
        let cli = parse(&[
            "mcp-server",
            "--pipe",
            "p",
            "--owner",
            "7",
            "--session",
            "s",
        ])
        .unwrap();
        assert_eq!(cli.command.map(|c| c.name()), Some("mcp-server"));
        let help = Cli::command().render_help().to_string();
        assert!(!help.contains("mcp-server"));
    }
}
