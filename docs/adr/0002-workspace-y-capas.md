# 0002. Workspace Cargo y orden de capas

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El original tiene 69 módulos Python en un solo paquete. En Rust conviene separar
responsabilidades en crates para compilar en paralelo, aislar `unsafe` y permitir que
varios agentes trabajen a la vez sin pisarse.

## Decisión

Workspace en `crates/` con este orden de dependencias (un crate solo puede depender de
los que tiene a su izquierda o encima):

```
mito-win
  └─ mito-core
       ├─ mito-i18n
       └─ mito-transfer
            └─ mito-catalog
                 └─ mito-engine   (también usa mito-win)
                      ├─ mito-terminal
                      ├─ mito-mcp
                      └─ mito-tui  (usa mito-i18n y mito-terminal)
                           └─ mito (binario)
```

Versiones de dependencias y lints se declaran una sola vez en el `Cargo.toml` raíz.

## Consecuencias

- Un ciclo entre crates es un error de diseño: se resuelve moviendo tipos a la capa inferior.
- `mito-win` no depende de ningún crate interno.
- La TUI y el MCP usan los mismos servicios del motor que la CLI.
