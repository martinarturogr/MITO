# Plan de desarrollo de MITO en Rust

Plan aprobado el 2026-11-26. Objetivo: portar a Rust el
[MITO original](https://github.com/danielgube/mito) con la mayor paridad posible.

## Decisiones

| Tema | Decisión |
|---|---|
| Alcance | Paridad total, por fases |
| Interfaz | TUI con ratatui dentro del Windows Terminal privado |
| Compatibilidad | Mismos formatos de disco y misma CLI que el original |
| Git | Una rama `feat/fNN-…` y un PR por fase, con CI; el merge lo hace el propietario |
| Bootstrap | Híbrido: ZIP/TAR/gz/xz/bz2 nativos; 7-Zip, innounp y Windows Terminal privados con hash fijado |
| Telemetría | Solo log local (`logs/mito.log`); sin PostHog |
| Recursos | Se reutilizan las traducciones y el branding del original, con permiso de su autor |
| Validación | El agente ejecuta fmt, clippy, tests, la app e instalaciones reales (E2E en `C:\mito-rs`) |
| Licencia | Código bajo `MIT OR Apache-2.0` |
| Idioma | Identificadores en inglés; docs, comentarios y commits en español |
| Binario | `mito.exe`, con `mito.cmd` como envoltorio |
| Releases | `martinarturogr/MITO` |

## Arquitectura

Ver [ADR 0002](adr/0002-workspace-y-capas.md). Crates en `crates/`:

| Crate | Responsabilidad |
|---|---|
| `mito-win` | Única capa con `unsafe`: junctions, Job Objects, registro, `.lnk`, Known Folders, MSI |
| `mito-core` | Rutas confinadas, JSON atómico, locks, `config.properties`, log local |
| `mito-i18n` | Traducciones y detección de idioma |
| `mito-transfer` | HTTP/HTTPS, reanudación, proxy, hashes, extracción validada |
| `mito-catalog` | Manifiestos Scoop, buckets, búsqueda y clasificación |
| `mito-engine` | Dependencias, instalación, profiles, entorno, persist, hooks |
| `mito-terminal` | Windows Terminal privado: ajustes, pestañas, identidad |
| `mito-tui` | Gestor en ratatui |
| `mito-mcp` | Servidor MCP local |
| `mito` | Binario y CLI (clap) |

## Fases

| Fase | Contenido | Criterio de cierre |
|---|---|---|
| F0 Fundaciones | Workspace, CI, lints, ADRs, matriz de paridad, recursos | CI verde con el esqueleto |
| F1 Núcleo | Storage, config, logs, i18n | Tests de confinamiento (proptest) e ida y vuelta de `config.properties` |
| F2 Transfer | Descargas, reanudación, hashes, extracción validada | Tests contra servidor HTTP local y fixtures maliciosos |
| F3 Catálogo | `bucket refresh/import`, `search`, clasificación | Lee los buckets reales de Scoop |
| F4 Motor | install, activate/deactivate, profiles, entorno, persist, `run`/`shell` | Instala un paquete real y lo ejecuta desde un profile |
| F5 Bootstrap y Terminal | Herramientas privadas, límite de ruta, pestañas | `mito.cmd` arranca desde una carpeta vacía |
| F6 TUI | Inicio, catálogo, profiles, configuración, log, operaciones | **v0.1: MVP usable** |
| F7 Formatos avanzados | Hooks, MSI, Inno, Burn/WiX | Paquetes reales de cada tipo |
| F8 Integración Windows | Accesos e iconos, apps del host, Data/Cache, «En host», «Abrir con», protocolos, modo acoplado | Tests sobre HKCU y carpetas temporales |
| F9 Red | Proxy, portal de autenticación, estado de Internet | Tests con proxy local |
| F10 Pocketools | Consumo, autoría, editor, Git | Ciclo completo contra repo local |
| F11 Ciclo de vida | Export/import, traslado, migración, versiones, auto-update, `release` | Actualización offline y online E2E |
| F12 MCP | Servidor, trabajos, diálogo | Un cliente MCP gestiona profiles |
| F13 Cierre | `doctor`, README, MANUAL, paridad completa | **v1.0** |

F1, F2 y F3 son independientes y pueden avanzar en paralelo; también F8, F9 y F10.

## Flujo por módulo y reparto entre modelos

1. **Haiku** lee el módulo Python y sus tests y escribe el contrato en
   `docs/spec/modules/<módulo>.md` (plantilla en [docs/spec/modules/README.md](spec/modules/README.md)).
2. **Opus** revisa el contrato, define la API pública (tipos y traits) y los criterios de aceptación.
3. **Sonnet** implementa con TDD en un worktree aislado hasta que fmt, clippy y tests pasan.
4. **Opus** revisa el diff, integra y abre el PR.

| Modelo | Tareas |
|---|---|
| Opus | Arquitectura, APIs, `unsafe` de `mito-win`, publicación con rollback, MSI/Burn, Job Objects, revisión |
| Sonnet | Módulos con contrato cerrado: catálogo, config, CLI, pantallas TUI, Pocketools, «Abrir con», protocolos, MCP |
| Haiku | Contratos, recursos, fixtures, CHANGELOG, documentación, matriz de paridad |

## Seguimiento

- [PARITY.md](PARITY.md): estado de cada módulo, test y comando del original.
- [adr/](adr/): decisiones de arquitectura.
- [CHANGELOG.md](../CHANGELOG.md): cambios por versión.
