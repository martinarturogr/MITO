# 0012. Los hooks vuelven a mito.exe para extraer

- Estado: Aceptado
- Fecha: 2026-11-27

## Contexto

Los hooks del catálogo (`pre_install`, `installer.script`, `installer.file`) se
ejecutan con PowerShell y un preámbulo que define las funciones de Scoop. Los helpers
`Expand-7zipArchive`, `Expand-InnoArchive`, `Expand-DarkArchive` y `Expand-MsiArchive`
vuelven a MITO para extraer dentro del payload. El original lo hace con
`& $context.python -B -I -m mito.hooks <kind> …`, y en el port no hay intérprete de
Python.

## Decisión

1. El preámbulo (`hooks::SCRIPT`) se conserva línea a línea salvo en
   `Invoke-MitoExpand`, que llama a `& $context.mito hook-expand <kind> <root> <dir>
   <archivo> <destino> <extract_dir>`. `<kind>` es siempre explícito: `7zip`, `inno`,
   `dark` o `msi`. El contexto JSON lleva `mito` (ruta de `core\bin\mito.exe`) en lugar de
   `python`. El resto del contexto, las variables, las funciones públicas
   (`info`, `warn`, `error`, `abort`, `Expand-*`, `Get-HelperPath`,
   `Invoke-ExternalCommand`) y el entorno no cambian.
2. `mito.exe hook-expand` es un subcomando interno y oculto de la ayuda. Valida que el
   archivo y el destino estén dentro del payload, como `mito.hooks.__main__`. Escribe el
   error en stderr y sale con 1.
3. Los mensajes de error de `Expand-InnoArchive` usan sus propias claves de traducción
   (el original traducía la de `Expand-7zipArchive` y reemplazaba el nombre).

## Consecuencias

- Los scripts del catálogo funcionan igual: solo usan las funciones públicas.
- `mito.exe` tiene un subcomando más, que no aparece en `--help`.
