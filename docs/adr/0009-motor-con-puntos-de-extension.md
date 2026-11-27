# 0009. Motor con puntos de extensión para fases posteriores

- Estado: Aceptado
- Fecha: 2026-11-26

## Contexto

`engine.py` y `components.py` llaman directamente a módulos que se portan más tarde:
hooks de PowerShell (`hooks.py`, F7), MSI/Inno/Burn (F7), `terminal.prepare` (F5),
`shortcuts.migrate_application_links` (F8), PATH de Pocketools (F10) y Job Objects para
lanzamientos acoplados (F8). F4 tiene que ser usable y verificable sin ellos.

## Decisión

`mito-engine` recibe esas piezas como puntos de extensión inyectados (traits o
callbacks), con un comportamiento por defecto **seguro y explícito**:

| Punto | Fase | Por defecto en F4 |
|---|---|---|
| Ejecutor de hooks (`pre_install`, `installer.script`/`file`) | F7 | Error de dominio «Pasos no soportados: <campo>» antes de descargar; nunca se omite en silencio |
| Extractores MSI/Inno/Burn | F7 | Los ganchos `msi`/`inno` de `mito-transfer` vacíos: «Formato de archivo todavía no soportado» |
| Refresco de Windows Terminal tras cambios | F5 | No-op |
| Migración de accesos de aplicaciones | F8 | No-op |
| Directorios de Pocketools en PATH | F10 | Lista vacía |
| Lanzamiento con Job Object (`--coupled`, `--owned`) | F8 | Error «no soportado todavía»; el lanzamiento normal usa `std::process::Command` |
| Ruta de 7-Zip | F5 | `core/tools/7zip/7z.exe` si existe |

Los shims de `bin` y los lanzadores invocan `core\bin\mito.exe` (ADR 0008) en lugar del
Python privado.

## Consecuencias

- Cada fase posterior sustituye su punto de extensión sin tocar el flujo del motor.
- Un manifiesto que requiera una capacidad aún no portada falla con un mensaje claro
  en lugar de instalarse incompleto.
