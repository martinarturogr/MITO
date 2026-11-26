# 0005. Extracción nativa y herramientas privadas fijadas

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El original descarga un CPython privado con Pillow y Textual, además de 7-Zip, innounp
y Windows Terminal. En Rust el binario ya contiene el motor y la interfaz.

## Decisión

- ZIP/nupkg, TAR y sus compresiones gzip/xz/bzip2 se extraen con crates nativas.
- 7z, RAR, CAB (MSI y Burn) e Inno Setup se delegan en 7-Zip e innounp privados.
- Windows Terminal sigue siendo privado.
- Las herramientas se descargan en el primer arranque, tras confirmación, desde URLs
  inmutables fijadas por SHA-256 en `core/bootstrap.lock.json`.

## Consecuencias

- El bootstrap ya no necesita Python ni pip.
- La validación de rutas, duplicados, ADS y límites es la misma para la extracción
  nativa y la delegada.
