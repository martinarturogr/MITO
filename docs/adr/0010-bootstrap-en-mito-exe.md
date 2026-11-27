# 0010. Bootstrap dentro de mito.exe

- Estado: Aceptado
- Fecha: 2026-11-27

## Contexto

El original prepara su runtime con `bootstrap.ps1` (bienvenida en consola, proxy, log,
telemetría y CPython con wheels) y después `bootstrap_tools.py` instala 7-Zip, innounp
y Windows Terminal desde `core/bootstrap.lock.json`. En Rust no hay intérprete que
preparar (ADR 0005, 0008).

## Decisión

1. `mito.exe` hace el bootstrap: `mito bootstrap` explícito y, en `start`, si falta
   alguna herramienta. `mito.cmd` ya no invoca PowerShell.
2. **Bienvenida en consola**, como el original: mismo texto (sin las líneas de Python,
   Pillow y Textual), mismos colores y respuesta afirmativa obligatoria antes de crear
   `core/tools` o descargar. Cancelar solo registra el diagnóstico local. Se muestra
   mientras falte alguna herramienta. `--approved` y `--preview-welcome` conservan el
   significado de `-Approved` y `-PreviewWelcome`.
3. **`core/bootstrap.lock.json` solo con `tools`**: mismas entradas, URLs inmutables y
   SHA-256 que el original. Desaparecen `python` y `wheels`. Las descargas usan
   `mito_transfer::download` y la extracción `mito_transfer::archives`.
4. **Versión nueva de una herramienta**: se reinstala en staging y sustituye a la
   anterior (en el original quedaba como «Herramienta incompleta»). Un marcador
   `.mito-tool.json` corrupto equivale a herramienta ausente.
5. **Sin telemetría** (ADR 0007). El login del portal del proxy es de F9.
6. **Terminal con marca**: si `YitoTerminal.exe` no puede reconstruirse (en uso,
   recursos inválidos), se usa `WindowsTerminal.exe` y se registra un aviso, en lugar
   de impedir el arranque.
7. Los perfiles del Terminal privado ejecutan `core\bin\mito.exe shell --profile <p>`
   (y `--type powershell`), y el perfil MITO ejecuta `mito.exe ui` (F6).

## Consecuencias

- El primer arranque necesita solo `mito.exe`, `mito.cmd` y el lock.
- Los tests del original que dependen de PowerShell o Python no aplican.
