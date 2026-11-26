# 0001. Registrar las decisiones con ADRs

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El port tiene un original con muchos contratos implícitos (`spec/ARCHITECTURE.md` del
proyecto Python). Las decisiones de diseño del port se pierden si solo viven en PRs o
conversaciones.

## Decisión

Cada decisión que afecte a la arquitectura, a un formato de disco, a la CLI o a una
diferencia deliberada con el original se registra en `docs/adr/NNNN-titulo.md`.

## Consecuencias

Los PRs que cambien un contrato deben enlazar su ADR. Un ADR aceptado no se reescribe:
se sustituye por uno nuevo.
