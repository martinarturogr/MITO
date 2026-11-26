# 0008. Criterios del port y kernel compartido

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

Los contratos de F1–F3 (`docs/spec/modules/`) muestran dudas recurrentes: comportamientos
sin test que parecen bugs, dependencias implícitas entre módulos (`ContextVar`, `tr()`
global, `diagnostics.write` desde cualquier sitio) y piezas que en Rust ya no existen
(CPython, `bootstrap.ps1`).

## Decisión

1. **El código del original manda sobre su documentación.** Se replican mensajes,
   constantes y formatos. Los bugs y huecos de seguridad sin test se **corrigen** sin
   cambiar el comportamiento visible ni los formatos, y cada corrección se anota en
   `docs/PARITY.md` («Diferencias deliberadas»). Ejemplo: el límite de 12 GiB de los
   ZIP se aplica a los bytes reales, no solo al tamaño declarado.
2. **Ejecutable en `core\bin\mito.exe`.** `mito.cmd` sigue en la raíz y lo invoca. La
   raíz se deduce con `Layout::from_executable`. No hay `bootstrap.ps1` ni Python: el
   ejecutable muestra la bienvenida y prepara las herramientas privadas (ADR 0005).
   Los tests del original que dependen de `bootstrap.ps1` no aplican.
3. **Solo Windows x64.** No se portan las ramas Unix del original.
4. **`Operation` explícito.** Se pasa `&Operation` como parámetro en lugar de la
   `ContextVar` implícita. `Operation::detached()` equivale a no tener operación activa.
5. **Mensajes traducibles con `Message`.** Los errores llevan la plantilla española y
   sus argumentos sin traducir. La TUI traduce con `mito-i18n`; la CLI y el log muestran
   el español (como el original). Así `mito-core` no depende de `mito-i18n`.
6. **Log mediante `tracing`.** Las librerías emiten eventos `tracing` con el campo
   `operation` (p. ej. `download.hash`) y sus detalles. `mito-core::diagnostics` aporta
   la capa que escribe el JSON por línea de `logs/mito.log` con el formato, el saneado
   y la rotación del original. Escribir el log nunca falla hacia el llamador.
7. **Errores.** `mito_core::Error` es el `YitoError` compartido. Cada crate define su
   propio enum con `thiserror` que lo envuelve.
8. **Lock de mutación con `File::try_lock`** de la biblioteca estándar sobre
   `core/state/<name>`, con reintentos cada 100 ms hasta `wait`.
9. **Huellas byte a byte.** Toda huella o clave calculada con `json.dumps` en el original
   (huella de manifiesto, claves de estado) se reproduce con un serializador que imita
   exactamente sus opciones (`ensure_ascii`, separadores, orden), para que una instancia
   creada por el original no vea sus componentes como cambiados.
10. **Configuración inyectada.** Los crates de capa baja (`mito-transfer`, `mito-catalog`)
    no leen `config.properties`: reciben ya resueltos los orígenes de buckets, el proxy,
    etc. Los lee la capa que los usa (motor o CLI).

## Consecuencias

- Las firmas públicas llevan `&Operation` y devuelven `Result` con errores tipados.
- Los tests comprueban los textos exactos de los mensajes en español.
