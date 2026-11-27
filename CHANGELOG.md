# Changelog

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/);
el proyecto sigue [SemVer](https://semver.org/lang/es/).

## [Sin publicar]

### Añadido

- F0: workspace Cargo con los crates de la arquitectura, CI (fmt, clippy, tests, docs,
  cargo-deny), ADRs, plan, matriz de paridad y recursos del original.
- F1: núcleo compartido en `mito-core` (`Message`, `Error`, rutas confinadas, JSON atómico,
  `Operation`, `Layout`, lock de mutación), `config.properties` con comentarios conservados,
  log JSON por línea con saneado y rotación, registro local de errores y pánicos,
  persistencia con junctions y hardlinks, y `mito-i18n` con los 24 idiomas incrustados y
  detección del idioma de Windows.
- F2: `mito-transfer` con descargas HTTP/HTTPS (reintentos, reanudación con `Range`,
  reintento sin verificar TLS, detección de portales, cancelación de lecturas
  bloqueadas), verificación de 14 algoritmos de hash y extracción validada de ZIP, TAR
  (gzip/xz/bzip2) y 7z/RAR mediante 7-Zip.
- F3: `mito-catalog` con el modelo de manifiestos Scoop, huella idéntica a la del
  original, clasificación de compatibilidad, búsqueda con ranking, snapshots de buckets
  (refresco desde el ZIP de GitHub sin extraerlo, importación local, consulta diaria de
  manifiestos instalados) y filtros de buckets.
- F4: `mito-engine` con profiles, inventario `.mito.json`, construcción del entorno y
  shims, instalación con dependencias, staging, publicación con rollback, copias por
  profile, componentes (actualizar, eliminar, versiones) y lanzamiento de aplicaciones,
  binarios y shells; puntos de extensión para las fases posteriores (ADR 0009).
- F4: CLI `mito.exe` con los subcomandos del original (`installed`, `logs`, `doctor`,
  `bucket`, `search`, `profile`, `install`, `activate`, `deactivate`, `shell`, `run`) y
  `mito.cmd` en la raíz; los comandos de fases posteriores se reconocen y avisan de que aún
  no están disponibles.
- F5: bootstrap de 7-Zip, innounp y Windows Terminal desde `mito.exe` (bienvenida en
  consola, hashes fijados, reinstalación por versión), límite de longitud de la raíz,
  `settings.json` del Terminal privado con perfiles por profile, `YitoTerminal.exe` con el
  icono de MITO, identidad de la barra de tareas y los comandos `start`, `bootstrap` y
  `terminal`.
- F6: interfaz de texto (`mito-tui`, ratatui) integrada en `mito.exe`: `mito` y
  `mito ui [--profile]` abren la aplicación con barra lateral (profiles y acciones),
  Inicio (componentes con casilla, Abrir, Info, Actualizar, Eliminar y «Actualizar
  todos»), catálogo (búsqueda, filtros por bucket, tabla, detalle, Instalar y
  Manifiesto), panel de log con «Copiar log», diálogos modales, indicador de ocupado,
  progreso con cancelación y aviso de paso largo. La terminal se restaura siempre,
  también con un pánico.
- F7: hooks del catálogo con PowerShell (`pre_install`, `installer.script` e
  `installer.file`) en `mito-engine::hooks`, con el preámbulo y el entorno del original
  y los helpers `Expand-*` que vuelven al subcomando oculto `mito.exe hook-expand`
  (ADR 0012); `post_install` sigue omitiéndose. Recursos de bucket (`$bucketsdir`) con
  copia privada por instalación antes de `pre_install`.
