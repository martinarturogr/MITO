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
