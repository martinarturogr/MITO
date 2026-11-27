# 0011. Interfaz de texto con ratatui

- Estado: Aceptado
- Fecha: 2026-11-27

## Contexto

La interfaz del original es una aplicación Textual (`ui.py` y sus diálogos): widgets
retenidos con foco, ratón, CSS, pantallas modales y *workers* en hilos que esperan
respuestas de la interfaz (`push_screen_wait`). ratatui es de modo inmediato: no trae
widgets con foco, ni pantallas modales, ni bucle de eventos.

## Decisión

1. **Arquitectura Elm.** El estado de la aplicación (`App`) solo cambia en
   `App::update(Event) -> Vec<Effect>`. `render` dibuja el estado sin modificarlo,
   salvo el mapa de zonas pulsables del último fotograma. Los efectos (lanzar una
   operación, copiar al portapapeles, salir) los ejecuta el bucle. Así la interfaz se
   prueba sin terminal: eventos simulados, `TestBackend` y snapshots de insta.
2. **Kit mínimo de widgets propio** (`widgets/`): botón, casilla, campo de texto,
   selector, tabla con cursor, registro con desplazamiento y diálogo modal. El foco se
   recorre con Tab/Mayús+Tab, Intro o Espacio activan, y el ratón pulsa por zonas.
   Mismas etiquetas, ids lógicos y orden que el original; colores del CSS original.
3. **Diálogos modales en pila.** Cada diálogo devuelve un resultado tipado al cerrarse;
   la aplicación lo recibe junto al motivo por el que lo abrió (la continuación que en
   Textual era el `await`).
4. **Operaciones en un hilo de trabajo**, una a la vez, con `mito_core::Operation` para
   el progreso y la cancelación. El diálogo de operación lee `Operation::state()` en
   cada tic (como el `set_interval` del original). Cuando el motor pide una
   confirmación (versión distinta, descargas sin hash), el hilo se bloquea en un canal
   y la interfaz muestra el diálogo y devuelve la respuesta.
5. **Traducción en el borde.** El registro (`LogLine`), el progreso (`OperationState`)
   y los errores llevan `Message` sin traducir; la interfaz los traduce con
   `mito-i18n`. El log local sigue en español.
6. **Portapapeles con la API de Windows** (`mito-win`), en lugar del proceso de
   PowerShell del original. El texto ya no pasa por un proceso externo.

## Consecuencias

- La interfaz no depende de un runtime asíncrono; el motor sigue siendo síncrono
  (ADR 0004).
- Las funciones de fases posteriores (red, MCP, Pocketools, host, «Abrir con»,
  protocolos, auto-update) dejan su botón visible pero inactivo, o lo ocultan, igual
  que cuando el original no las tiene disponibles. Cada fase las conecta.
- El aspecto no es idéntico al de Textual (bordes, sombras, animaciones), pero se
  conservan la disposición, los textos, los colores y los atajos.
