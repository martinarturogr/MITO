# 0013. Lanzador sin consola e integración con Windows

- Estado: Aceptado
- Fecha: 2026-11-28

## Contexto

Los accesos directos, las entradas «Abrir con» y los protocolos del original ejecutan
`core\tools\python\pythonw.exe -B -I -X utf8 -m mito run …` (o `host-run …`):
`pythonw.exe` no abre consola. `mito.exe` es una aplicación de consola, y lanzada desde
Explorer mostraría una ventana de consola. Además, el original dependía de PowerShell
(`WScript.Shell`) para escribir los `.lnk` y de Pillow para dibujar las insignias.

## Decisión

1. **`core\bin\mitow.exe`**: segundo binario del crate `mito`, con el subsistema de
   Windows (sin consola), compilado del mismo código. Solo se usa como destino de los
   `.lnk`, de los comandos de «Abrir con» y de los protocolos. El `.lnk` de una
   aplicación es `mitow.exe run [--coupled] --profile <p> --app <id> [--shortcut <s>]
   [-- args]`, y el del host `mitow.exe host-run <identidad> --profile <p> [--coupled]
   [-- args]`. Los errores, que en consola irían a stderr, se registran en
   `logs/mito.log`.
2. **`.lnk` nativos**: se escriben y leen con `IShellLinkW`/`IPersistFile` (los mismos
   campos que `WScript.Shell`: destino, argumentos, directorio de trabajo e icono), sin
   PowerShell.
3. **Insignias**: se dibujan en Rust con el mismo subconjunto de Aileron Regular que
   usa Pillow por defecto (`ImageFont.load_default`, CC0), embebido en el binario. Los
   iconos se generan con el mismo algoritmo, tamaños, colores y caché
   (`core/state/icons`); el rasterizado puede diferir en algún píxel del de FreeType.
4. **Fallos del original que se corrigen** (política del proyecto): al reclamar un
   protocolo que ya estaba reclamado, se conserva el respaldo del dueño original del
   host en lugar de sustituirlo por el de la otra aplicación.
5. **Crate `mito-desktop`** (amplía el orden de capas del ADR 0002): accesos directos,
   iconos, aplicaciones y carpetas del host, «Abrir con» y protocolos. Depende de
   `mito-engine` y `mito-terminal` (el acceso a MITO abre su terminal) y lo usan
   `mito-tui` y el binario. Todo el `unsafe` sigue en `mito-win`.
6. **Migración**: los `.lnk`, «Abrir con» y protocolos creados por el MITO original
   (con `pythonw.exe`) se reescriben con `mitow.exe` en la migración de F11.

## Consecuencias

- La distribución lleva `core\bin\mito.exe` y `core\bin\mitow.exe`.
- Abrir una aplicación desde el escritorio no muestra ninguna consola.
