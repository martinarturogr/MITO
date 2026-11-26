# 0006. Política de errores y de `unsafe`

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

MITO manipula rutas, registro y procesos de Windows. Un pánico o un `unsafe` mal
acotado puede dejar la instancia o el host en mal estado.

## Decisión

- Librerías: errores tipados con `thiserror`, un enum por crate o por módulo.
- Binario: `anyhow` con contexto en cada capa.
- Prohibidos `unwrap`, `expect`, `panic!` y `todo!` fuera de tests (clippy en `deny`).
- `unsafe` solo en `mito-win`, envuelto en funciones seguras y con `// SAFETY:` en cada
  bloque (`clippy::undocumented_unsafe_blocks`). Los demás crates usan
  `#![forbid(unsafe_code)]`.
- Las llamadas a Win32 usan el crate `windows`.

## Consecuencias

El código Win32 queda revisable en un solo crate y el resto del workspace no puede
introducir `unsafe` por accidente.
