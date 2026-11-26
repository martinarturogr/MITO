# 0007. Solo log local, sin telemetría remota

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El original envía informes de error a PostHog con la clave del proyecto de su autor,
además de mantener un log local rotado.

## Decisión

Se porta solo el log local: JSON por línea en `logs/mito.log`, 1 MiB y tres copias
`.1..3`, lock compartido entre procesos y ubicación alternativa en
`%TEMP%/MITO-logs/<hash-de-raíz>`. No se implementa la cola de PostHog ni el comando
`posthog-check`. Las claves `posthog.*` de `config.properties` se conservan sin efecto.

## Consecuencias

`docs/PARITY.md` marca `error_tracking` (parte remota) y `posthog-check` como no aplica.
