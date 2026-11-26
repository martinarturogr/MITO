# Matriz de paridad: MITO (Python) → MITO (Rust)

Este documento sigue qué funcionalidad del original Python está portada a Rust. Cada fila enlaza un módulo, test o comando del original con su crate Rust, su fase y su estado. Se actualiza al cerrar cada fase.

**Leyenda de estados**

- ⬜ pendiente: no iniciado.
- 🟨 en curso: portado parcialmente o en desarrollo.
- ✅ hecho: portado y verificado con sus tests equivalentes.
- ➖ no aplica: no se porta; la justificación figura en la sección «Fuera de alcance / diferencias deliberadas».

Fases: F1 base (storage, configuración, i18n) · F2 transferencias · F3 catálogo · F4 motor · F5 terminal y arranque · F6 interfaz · F7 instaladores (MSI, Inno, Burn) · F8 integración con Windows · F9 red · F10 Pocketools · F11 perfiles, actualizaciones y release · F12 MCP · F13 cierre (doctor, docs, v1.0).

## Módulos

Fuente: `core/app/mito/*.py` (sin `__init__.py`). Líneas = saltos de línea de cada archivo.

| Módulo Python | Líneas | Responsabilidad | Crate Rust | Fase | Estado |
|---|---:|---|---|---|---|
| `__main__.py` | 354 | Punto de entrada de la CLI y despacho de subcomandos | mito | F4 | ⬜ |
| `archives.py` | 104 | Extrae archivos comprimidos sin ejecutar instaladores descargados | mito-transfer | F2 | ⬜ |
| `arguments.py` | 26 | Divide cadenas de argumentos de línea de comandos en Windows | mito | F4 | ⬜ |
| `bootstrap_tools.py` | 56 | Asegura herramientas privadas del runtime, verificadas por hash | mito | F5 | ⬜ |
| `bucket_resources.py` | 80 | Snapshots privados por instalación para referencias `$bucketsdir` en hooks | mito-catalog | F7 | ⬜ |
| `burn.py` | 201 | Lee CABs de bundles WiX 3 Burn como datos, sin ejecutarlos | mito-win | F7 | ⬜ |
| `catalog.py` | 262 | Modelo del catálogo: manifiestos, instaladores y huellas | mito-catalog | F3 | ⬜ |
| `components.py` | 331 | Ciclo de vida genérico de componentes instalados, sin nombres de app | mito-engine | F4 | ⬜ |
| `configuration.py` | 146 | Almacenamiento del editor de configuración; sólo guardados explícitos escriben | mito-core (`config`) | F1 | ✅ |
| `configuration_ui.py` | 129 | Diálogo Textual de configuración global | mito-tui | F6 | ⬜ |
| `diagnostics.py` | 150 | Diagnósticos locales acotados, usables por el Python de bootstrap | mito-core (`diagnostics`) | F1 | ✅ |
| `dialog_header.py` | 22 | Cabecera común de diálogos Textual | mito-tui | F6 | ⬜ |
| `engine.py` | 703 | Motor central: clase Engine, entornos y movimiento de instancia | mito-engine | F4 | ⬜ |
| `error_tracking.py` | 272 | Trazas de error locales; PostHog es opcional y no se porta | mito-core (`error_tracking`) | F1 | ✅ parte local; PostHog ➖ |
| `hooks.py` | 270 | Ejecuta scripts de instalación del catálogo (código de confianza, no sandbox) | mito-engine | F7 | ⬜ |
| `host_apps.py` | 194 | Ejecutables externos lanzados con el entorno del profile MITO | mito-win | F8 | ⬜ |
| `host_apps_ui.py` | 190 | Diálogos Textual de aplicaciones del host | mito-tui | F8 | ⬜ |
| `host_data.py` | 222 | Junctions explícitos de AppData portable hacia carpetas del host | mito-win | F8 | ⬜ |
| `host_exposure.py` | 242 | Junctions explícitos de AppData del host hacia AppData portable | mito-win | F8 | ⬜ |
| `i18n.py` | 109 | Traducción de mensajes MITO; catálogos y salida externa no se traducen | mito-i18n | F1 | ✅ |
| `icons.py` | 442 | Genera iconos de acceso directo con insignia MITO | mito-win | F8 | ⬜ |
| `inno.py` | 139 | Extrae archivos de instaladores Inno como datos, con innounp fijado | mito-win | F7 | ⬜ |
| `isolation.py` | 61 | Aislamiento de profile según campos declarativos del manifiesto | mito-engine | F4 | ⬜ |
| `jobs.py` | 295 | Jobs acotados de comandos y operaciones por sesión MCP | mito-engine | F4 | ⬜ |
| `localized_ui.py` | 17 | Traduce enlaces por instancia sin cambiar clases Textual | mito-tui | F6 | ⬜ |
| `mcp_server.py` | 125 | Proceso HTTP MCP alojado en la pestaña de Windows Terminal | mito-mcp | F12 | ⬜ |
| `mcp_service.py` | 143 | Ciclo de vida del servidor MCP controlado por el gestor | mito-mcp | F12 | ⬜ |
| `mcp_tools.py` | 285 | Esquemas MCP y adaptadores sobre los servicios genéricos | mito-mcp | F12 | ⬜ |
| `mcp_ui.py` | 138 | Control y configuración MCP opt-in en Textual | mito-tui | F12 | ⬜ |
| `msi.py` | 234 | Lee bases MSI y cabinets como datos; nunca ejecuta instalaciones | mito-win | F7 | ⬜ |
| `native_ui.py` | 15 | Localiza etiquetas fijas de los diálogos nativos de Windows | mito-tui | F6 | ⬜ |
| `network.py` | 502 | Enrutado de proxy y autenticación de portal, sin secretos persistidos | mito-transfer | F9 | ⬜ |
| `network_ui.py` | 122 | Diálogos Textual de red y proxy | mito-tui | F9 | ⬜ |
| `open_with.py` | 278 | Entradas «Abrir con» de Windows por componente (ProgID en HKCU) | mito-win | F8 | ⬜ |
| `open_with_ui.py` | 130 | Diálogo para gestionar extensiones «Abrir con» de un componente | mito-tui | F8 | ⬜ |
| `operation.py` | 110 | Progreso y cancelación cooperativa de la operación en curso | mito-core (`operation`) | F1 | ✅ |
| `path_limit.py` | 44 | Protección MAX_PATH para la raíz portable en Windows | mito-core | F5 | ⬜ |
| `persistence.py` | 101 | Persiste directorios y archivos bajo un único home y reconstruye enlaces | mito-core (`persistence`) + mito-win (`links`) | F1 | ✅ |
| `pocketool_authoring.py` | 269 | Fuentes Pocketool de usuario; guardar nunca ejecuta comandos | mito-engine | F10 | ⬜ |
| `pocketool_editor_ui.py` | 453 | Diálogos de autoría de Pocketools | mito-tui | F10 | ⬜ |
| `pocketool_execution.py` | 184 | Resuelve comandos instalados al lanzar y activa el profile | mito-engine | F10 | ⬜ |
| `pocketool_git.py` | 199 | Operaciones Git explícitas en el repositorio de Pocketools | mito-engine | F10 | ⬜ |
| `pocketools.py` | 194 | Paquetes de scripts compartidos: snapshots validados, sin hooks | mito-engine | F10 | ⬜ |
| `pocketools_ui.py` | 113 | Diálogo Textual de Pocketools | mito-tui | F10 | ⬜ |
| `processes.py` | 147 | Tiempo de vida de procesos Windows; los comandos no heredan handles | mito-win | F4 | ⬜ |
| `profile_files.py` | 399 | Archivos de configuración elegidos a mano en la tabla files del profile | mito-engine | F11 | ⬜ |
| `profile_files_ui.py` | 188 | Diálogo Textual para elegir archivos de configuración del profile | mito-tui | F11 | ⬜ |
| `profile_management_ui.py` | 82 | Diálogos Textual para crear y borrar profiles | mito-tui | F4 | ⬜ |
| `profile_transfer.py` | 123 | Comparte selecciones del catálogo y archivos elegidos; nunca serializa profiles | mito-engine | F11 | ⬜ |
| `profile_transfer_ui.py` | 174 | Diálogos Textual de exportación e importación de profiles | mito-tui | F11 | ⬜ |
| `protocols.py` | 356 | Reclamo de esquemas URL (claude, vscode) para componentes MITO | mito-win | F8 | ⬜ |
| `protocols_ui.py` | 171 | Diálogo que reclama esquemas URL mientras permanece abierto | mito-tui | F8 | ⬜ |
| `publish.py` | 225 | Publica releases numeradas con la credencial Git de GitHub | mito-engine | F11 | ⬜ |
| `release.py` | 71 | Construye distribuible desde lista blanca explícita, nunca datos de usuario | mito-engine | F11 | ⬜ |
| `relocation.py` | 268 | Repara rutas absolutas de instancias movidas o copiadas | mito-engine | F11 | ⬜ |
| `search.py` | 40 | Ordena coincidencias del catálogo: identificador primero, luego descripción | mito-catalog | F3 | ⬜ |
| `self_update.py` | 508 | Actualizaciones explícitas de release, aplicadas por runtime desechable | mito-engine | F11 | ⬜ |
| `self_update_ui.py` | 49 | Revisión humana antes de descargar una release de MITO | mito-tui | F11 | ⬜ |
| `shortcuts.py` | 233 | Accesos directos de aplicaciones en el escritorio de Windows | mito-win | F8 | ⬜ |
| `startup.py` | 28 | Guardia de arranque sólo stdlib: diagnostica fallos de importación | mito | F4 | ⬜ |
| `storage.py` | 114 | Rutas y JSON seguros, YitoError, identificadores y bloqueo de mutaciones | mito-core (`storage`, `lock`, `error`) | F1 | ✅ |
| `storage_migration.py` | 76 | Conversión única de directorios de versión a rutas estables | mito-core | F11 | ⬜ |
| `terminal.py` | 173 | Una ventana privada de Windows Terminal con activaciones de MITO | mito-terminal | F5 | ⬜ |
| `terminal_branding.py` | 139 | Copia con marca MITO del ejecutable privado de Windows Terminal | mito-terminal | F5 | ⬜ |
| `terminal_identity.py` | 142 | Identidad explícita en barra de tareas para ventanas de Terminal | mito-terminal | F5 | ⬜ |
| `transfer.py` | 402 | Descargas, hashes y validación de transferencias de componentes | mito-transfer | F2 | ⬜ |
| `ui.py` | 1952 | Interfaz Textual principal y diálogos de componentes | mito-tui | F6 | ⬜ |
| `update_checks.py` | 31 | Comprobaciones diarias persistentes de actualización, incluidas las fallidas | mito-engine | F11 | ⬜ |
| `versions.py` | 324 | Versiones previas de componentes desde histórico Git y autoupdate | mito-core | F11 | ⬜ |

## Tests del original

Nº de tests = líneas que contienen `def test_` (incluye `async def`), en `core/tests/test_*.py`.

| Test Python | Nº de tests | Fase | Tests Rust equivalentes | Estado |
|---|---:|---|---|---|
| `test_activation.py` | 11 | F4 |  | ⬜ |
| `test_application_entries.py` | 2 | F8 |  | ⬜ |
| `test_bootstrap_proxy.py` | 2 | F9 |  | ⬜ |
| `test_bucket_resources.py` | 6 | F7 |  | ⬜ |
| `test_burn.py` | 8 | F7 |  | ⬜ |
| `test_catalog_performance.py` | 4 | F3 |  | ⬜ |
| `test_cli.py` | 12 | F4 |  | ⬜ |
| `test_component_environment.py` | 2 | F4 |  | ⬜ |
| `test_components.py` | 17 | F4 |  | ⬜ |
| `test_configuration.py` | 4 | F1 | `mito-core::config` (4) | ✅ |
| `test_diagnostics.py` | 10 | F1 | `mito-core::diagnostics` (5); 5 dependen de `bootstrap.ps1` ➖ | ✅ |
| `test_engine.py` | 47 | F4 |  | ⬜ |
| `test_error_tracking.py` | 15 | F1 | `mito-core::error_tracking` (parte local de 5); 10 dependen de PostHog ➖ | ✅ |
| `test_host_apps.py` | 9 | F8 |  | ⬜ |
| `test_host_data.py` | 10 | F8 |  | ⬜ |
| `test_host_data_ui.py` | 2 | F8 |  | ⬜ |
| `test_host_exposure.py` | 6 | F8 |  | ⬜ |
| `test_i18n.py` | 9 | F1 | `mito-i18n` (6); 3 de interfaz pasan a F6 | 🟨 |
| `test_inno.py` | 10 | F7 |  | ⬜ |
| `test_installer_files.py` | 6 | F7 |  | ⬜ |
| `test_isolation.py` | 6 | F4 |  | ⬜ |
| `test_mcp.py` | 15 | F12 |  | ⬜ |
| `test_mcp_ui.py` | 3 | F12 |  | ⬜ |
| `test_move_whole.py` | 5 | F11 |  | ⬜ |
| `test_msi.py` | 5 | F7 |  | ⬜ |
| `test_network.py` | 5 | F9 |  | ⬜ |
| `test_network_setup.py` | 8 | F9 |  | ⬜ |
| `test_open_with.py` | 9 | F8 |  | ⬜ |
| `test_path_limit.py` | 4 | F5 |  | ⬜ |
| `test_persistence_locations.py` | 2 | F1 | `mito-core::persistence` (2) | ✅ |
| `test_pocketool_authoring.py` | 25 | F10 |  | ⬜ |
| `test_pocketool_ui.py` | 7 | F10 |  | ⬜ |
| `test_pocketools.py` | 4 | F10 |  | ⬜ |
| `test_processes.py` | 1 | F4 |  | ⬜ |
| `test_profile_transfer.py` | 16 | F11 |  | ⬜ |
| `test_protocols.py` | 12 | F8 |  | ⬜ |
| `test_publish.py` | 9 | F11 |  | ⬜ |
| `test_release.py` | 2 | F11 |  | ⬜ |
| `test_relocation.py` | 5 | F11 |  | ⬜ |
| `test_search.py` | 4 | F3 |  | ⬜ |
| `test_self_update.py` | 28 | F11 |  | ⬜ |
| `test_shortcuts.py` | 16 | F8 |  | ⬜ |
| `test_stable_storage.py` | 7 | F4 | Son tests de motor y migración, no de `storage.py`; `storage` tiene tests propios | ⬜ |
| `test_terminal.py` | 10 | F5 |  | ⬜ |
| `test_terminal_identity.py` | 4 | F5 |  | ⬜ |
| `test_transfer.py` | 10 | F2 |  | ⬜ |
| `test_ui.py` | 26 | F6 |  | ⬜ |
| `test_update_checks.py` | 10 | F11 |  | ⬜ |
| `test_versions.py` | 7 | F11 |  | ⬜ |

## Comandos CLI

Fuente: subparsers de `core/app/mito/__main__.py`. Incluye el comando oculto `mcp-server`.

| Comando | Fase | Estado |
|---|---|---|
| `mito` / `mito ui [--profile]` | F6 | ⬜ |
| `mito start [--profile]` | F5 | ⬜ |
| `mito bootstrap` | F5 | ⬜ |
| `mito installed` | F4 | ⬜ |
| `mito logs` | F4 (API lista en F1: `diagnostics::locations`) | ⬜ |
| `mito doctor` | F4 | ⬜ |
| `mito posthog-check` | F1 (PostHog) | ➖ no aplica (ver fuera de alcance) |
| `mito migrate-storage` | F11 | ⬜ |
| `mito bucket import <name> <source>` | F3 | ⬜ |
| `mito bucket refresh [name]` | F3 | ⬜ |
| `mito search [query] [--compatible]` | F3 | ⬜ |
| `mito profile create <name> [--workspace]` | F4 | ⬜ |
| `mito profile delete <name>` | F4 | ⬜ |
| `mito profile export <name> <output>` | F11 | ⬜ |
| `mito profile import <source> --name` | F11 | ⬜ |
| `mito profile list` | F4 | ⬜ |
| `mito profile show <name>` | F4 | ⬜ |
| `mito install <app> [--profile]` | F4 | ⬜ |
| `mito activate <installation> --profile` | F4 | ⬜ |
| `mito deactivate <installation> --profile` | F4 | ⬜ |
| `mito shell --profile [--type cmd\|powershell]` | F4 | ⬜ |
| `mito terminal --profile [--type] [--new]` | F5 | ⬜ |
| `mito run --profile --app [--bin] [--shortcut] [--coupled]` | F4 (`--coupled` en F8) | ⬜ |
| `mito app-shortcut <installation> --profile [--badge] [--color] [--shortcut] [--coupled]` | F8 | ⬜ |
| `mito shortcut --profile [--badge] [--color]` | F8 | ⬜ |
| `mito host-run --profile …` | F8 | ⬜ |
| `mito pocketool-run --profile --package --entry` | F10 | ⬜ |
| `mito mcp-server --pipe --owner --session` (oculto) | F12 | ⬜ |
| `mito release [mayor\|minor\|hotfix] [--local] [--output]` | F11 | ⬜ |

## Contratos de disco

Formatos que la versión Rust debe leer y escribir de forma compatible.

- `config.properties` (raíz de la instancia; las claves `posthog.*` no se portan).
- `profiles/<name>.json` (p. ej. `profiles/default.json` con `workspace`, `apps`, `env`).
- `.mito.json` (17 referencias en el código; ver `isolation.py`, `engine.py`, `release.py`).
- `core/state/*.json` según `spec/ARCHITECTURE.md`:
  - `core/state/instance.json` (identidad de la instancia; detecta traslados).
  - `core/state/relocation-notice.json`.
  - `core/state/terminal-profile.json`; `core/state/terminal-settings-before-branding.json`.
  - `core/state/icons/` (derivados de iconos).
  - `core/state/host-applications.json`; `core/state/host-data-links.json`; `core/state/host-exposed-links.json`.
  - `core/state/open-with.json`; `core/state/protocols.json`; `core/state/application-shortcuts`; `core/state/pocketool-shortcuts`.
  - `core/state/catalog-filters.json`.
  - `core/state/mito-update-check.json`; `core/state/component-updates-check.json`.
  - `core/state/mcp/settings.json`.
- `core/apps/<bucket>/<app>` (componentes instalados).
- `core/profile-apps` (aplicaciones por profile).
- `home/persist` y `home/profiles/<p>/persist` (persistencia bajo el home).
- `workspaces/` (espacios de trabajo).
- `pocketools/<package>` (paquetes locales de Pocketools).
- `logs/mito.log` y rotación `logs/mito.log.N` (`diagnostics.py`); si `logs/` no es escribible, `%TEMP%/MITO-logs/<identity>`.

## Fuera de alcance / diferencias deliberadas

- **PostHog**: no se porta. El error tracking queda sólo como log local (`logs/mito.log`). El comando `posthog-check` no tiene equivalente en Rust (➖).
- **CPython, Pillow y Textual del bootstrap**: no son necesarios en Rust. `bootstrap_tools.py` se porta como preparación de herramientas privadas, no como instalación de un intérprete Python.
- **`release`** publica en `martinarturogr/MITO` (repositorio GitHub fijo del original). Confirmar el destino antes de portar `publish.py`.
- **`Operation` explícito** (ADR 0008): se pasa como parámetro en lugar de la `ContextVar` implícita.
- **Log con `tracing`** (ADR 0008): los eventos DEBUG/TRACE no se escriben (el log solo conoce info, warning y error); el saneado se aplica también a los detalles de texto; `runtime` informa de `rust` en lugar de `python` y `function` de cada frame es null.
- **Sin deduplicación en el log local**: en el original solo limitaba los envíos a PostHog.
- **`persist` de archivo sin origen previo** se crea como archivo (hardlink), no como directorio (bug del original). Archivo o directorio se decide como Scoop: el último segmento tiene un punto seguido de al menos un carácter. Una ruta `persist` vacía es un error en lugar de enlazar toda la carpeta de la aplicación.
- **Junctions con la crate `junction`** en lugar de lanzar PowerShell.
- **`posthog.*`** no son propiedades reconocidas: se conservan como claves desconocidas, sin efecto.
- **`network.check_status`** exige dígitos ASCII (el original aceptaba dígitos Unicode con `isdecimal()`).
- **Validadores reubicados**: `validate_url`, `badge_text`, `validate_source` y `validate_env` viven en `mito-core::config` para validar la configuración sin depender de módulos de fases posteriores; F4, F9 y F10 deben reutilizarlos.
- **Saneado de rutas**: se mantiene el orden del original (perfil de usuario de Windows antes que la raíz de la instancia); una instancia bajo `C:\Users\…` se muestra como `<USER>\…`.
