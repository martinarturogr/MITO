# 0004. Motor síncrono con `Operation` para progreso y cancelación

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

El original ejecuta las operaciones largas en workers de Textual y comunica progreso y
cancelación mediante `mito.operation` (una operación por hilo). Un motor async
complicaría todos los crates sin beneficio claro: el trabajo es de E/S de disco y
descargas secuenciales.

## Decisión

El motor es síncrono. Las operaciones largas reciben un `Operation` con un flag de
cancelación atómico y un canal de progreso; sin `Operation`, el progreso es un no-op.
La TUI ejecuta el motor en hilos y recibe eventos por canales. Tokio solo se usa dentro
de `mito-mcp`.

## Consecuencias

- La cancelación se comprueba entre pasos y durante la lectura de descargas, como en el
  original; sustituir rutas estables no es cancelable.
- Las crates HTTP deben ofrecer API bloqueante.
