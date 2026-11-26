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
