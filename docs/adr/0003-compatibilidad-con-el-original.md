# 0003. Compatibilidad de formatos y CLI con el original

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El objetivo es parecerse lo más posible al MITO original. Fijar los formatos sirve
además de especificación cerrada y permite comparar las dos implementaciones.

## Decisión

MITO en Rust lee y escribe la misma estructura de instancia que el original
(`config.properties`, `profiles/<name>.json`, `.mito.json`, `core/state/*.json`,
`core/apps/<bucket>/<app>`, `core/profile-apps`, `home/`, `workspaces/`, `pocketools/`,
`logs/`) y ofrece los mismos subcomandos y opciones. La lista completa está en
`docs/PARITY.md`.

## Consecuencias

- Los tests de formato usan ejemplos reales generados por el original (golden tests).
- Cualquier diferencia necesita un ADR propio.
- La serialización JSON debe conservar el orden y los campos del original.
