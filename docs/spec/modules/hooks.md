# hooks.py → mito_engine::hooks

Origen: `core/app/mito/hooks.py` (270 líneas). No existe `core/tests/test_hooks.py`. La cobertura viene de:
`core/tests/test_burn.py:161-176` (`expand_installer`), `core/tests/test_inno.py:19-43` (`expand_inno`),
`core/tests/test_engine.py:384-660` (hooks ejecutados por el motor, PowerShell real) y
`core/tests/test_installer_files.py` (ver `installer_files.md`).

Nota de crate: PARITY.md asigna `hooks.py` a `mito-engine`. Los helpers `Expand-*` y el runner PowerShell son parte del mismo módulo.

## Responsabilidad

Ejecutar, como código de confianza y no en sandbox, los pasos `pre_install`, `installer.script` e `installer.file` del manifiesto, con un contexto JSON, un runner `.ps1` generado y helpers `Expand-*` que vuelven a MITO para extraer archivos dentro del payload. `post_install` nunca se ejecuta (la decisión se toma en `engine.py:248-250`).

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `run_pre_install(root, key, manifest, payload, log)` (124) | raíz, `key` `bucket/app`, manifiesto efectivo, `payload: Path`, `log` | `None` | `YitoError` de `run_script` |
| `run_installer(root, key, manifest, payload, log)` (128) | igual | `None` (no hace nada si no hay `installer`) | `YitoError('No existe {p1}')` si `installer.file` no existe en el payload; `run_script` |
| `run_script(root, key, manifest, payload, log, hook, script, *, file=None, arguments=None)` (143) | `hook` = `'pre_install'`, `'installer.file'` o `'installer.script'`; `script` texto o lista; `file`/`arguments` solo para `installer.file` | `None` | ver «Errores» |
| `expand(root, payload, archive, destination, extract_dir='.')` (203) | rutas; `extract_dir` | `None` | `YitoError` por rutas fuera del payload, `ExtractDir` ausente; errores de `extract_7zip` |
| `expand_inno(root, payload, archive, destination, extract_dir='{app}')` (219) | igual | `None` | igual, con `inno_root` |
| `expand_installer(kind, root, payload, archive, destination, extract_dir='.')` (237) | `kind` `'dark'` o `'msi'` | `None` | `YitoError` por rutas, `Extractor no soportado`, `ExtractDir` ausente |
| `__main__` (259-270) | `argv` (ver «Protocolo de helpers») | exit 0 | cualquier excepción: `print(str(exc), file=stderr)` y `sys.exit(1)` |
| `SCRIPT` (15-121) | constante: runner PowerShell (preámbulo) | texto | — |

## Preámbulo PowerShell (`SCRIPT`), copiado literalmente

Es parte del contrato. Cada hook escribe este texto como `<hook>.ps1` (UTF-8 con BOM) en `payload.parent`, lo ejecuta con `powershell.exe` y le pasa la ruta del JSON de contexto como único argumento posicional (`param([string]$ContextPath)`).

```powershell
param([string]$ContextPath)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
try {
    $context = Get-Content -LiteralPath $ContextPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $dir = $context.dir
    $original_dir = $dir
    $persist_dir = $context.persist_dir
    $app = $context.app
    $bucket = $context.bucket
    $bucketsdir = $context.bucketsdir
    $version = $context.version
    $architecture = '64bit'
    $global = $false
    $manifest = $context.manifest
    $fname = $context.fname
    function info { param([string]$Message) Write-Output $Message }
    function warn { param([string]$Message) Write-Output "Aviso: $Message" }
    function error { param([string]$Message) throw $Message }
    function abort { param([string]$Message) throw $Message }
    # Like Scoop, the Expand-* helpers accept the archive by pipeline
    # (Get-Item ... | Expand-7zipArchive); FullName binds files and directories by
    # their complete path. Each piped archive is expanded in turn.
    function Invoke-MitoExpand {
        param([string]$Kind, [string]$Name, [string]$Path, [string]$DestinationPath, [string]$ExtractDir, [bool]$Removal)
        if (!$DestinationPath) { $DestinationPath = Split-Path $Path }
        $arguments = @('-B', '-I', '-m', 'mito.hooks')
        if ($Kind) { $arguments += $Kind }
        & $context.python @arguments $context.root $dir $Path $DestinationPath $ExtractDir
        if ($LASTEXITCODE -ne 0) { throw "$Name falló" }
        if ($Removal) { Remove-Item -LiteralPath $Path -Force }
    }
    function Expand-7zipArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '.',
            [switch]$Removal
        )
        process { Invoke-MitoExpand '' 'Expand-7zipArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Expand-InnoArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '{app}',
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'inno' 'Expand-InnoArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Expand-DarkArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'dark' 'Expand-DarkArchive' $Path $DestinationPath '.' $Removal }
    }
    function Expand-MsiArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '.',
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'msi' 'Expand-MsiArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Get-HelperPath {
        [CmdletBinding()]
        param([Parameter(Mandatory=$true)][string]$Helper)
        if ($Helper -ne 'Innounp') { throw "Helper no soportado: $Helper" }
        $helperPath = Join-Path $context.root 'core/tools/innounp/innounp.exe'
        if (!(Test-Path -LiteralPath $helperPath -PathType Leaf)) { throw 'Herramienta incompleta: innounp' }
        return $helperPath
    }
    function Invoke-ExternalCommand {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true)][string]$FilePath,
            [string[]]$ArgumentList = @(),
            [string]$LogPath
        )
        $output = & $FilePath @ArgumentList 2>&1
        $code = $LASTEXITCODE
        if ($LogPath) { $output | Out-File -LiteralPath $LogPath -Encoding utf8 }
        if ($code -ne 0) { throw "$FilePath falló ($code): $($output -join [Environment]::NewLine)" }
        return $output
    }
    Set-Location -LiteralPath $dir
    if ($context.file) {
        $installerArgs = @($context.arguments)
        $global:LASTEXITCODE = 0
        & $context.file @installerArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
        & ([scriptblock]::Create($context.script))
    }
} catch {
    [Console]::Error.WriteLine($_.ToString())
    exit 1
}
```

Variables que ve el script del catálogo (todas de solo lectura por convención; el original no las protege):

| Variable | Valor |
|---|---|
| `$dir` | `payload` (carpeta de trabajo; `Set-Location` a ella antes de ejecutar) |
| `$original_dir` | igual que `$dir` |
| `$persist_dir` | `root/home/persist/<bucket>/<app>` o, si la app está aislada, `payload.parent/persist` |
| `$app`, `$bucket` | partes de `key` |
| `$bucketsdir` | `payload.parent/hook-buckets` (ver `bucket_resources.md`) |
| `$version` | `str(manifest['version'])` |
| `$architecture` | siempre `'64bit'` |
| `$global` | siempre `$false` |
| `$manifest` | el manifiesto efectivo, como objeto JSON |
| `$fname` | nombre local de cada URL (`download_filename`): cadena si hay una URL, array si hay varias |

Funciones: `info` (escribe), `warn` (escribe `Aviso: …`), `error` y `abort` (lanzan), `Invoke-MitoExpand`, `Expand-7zipArchive`, `Expand-InnoArchive`, `Expand-DarkArchive`, `Expand-MsiArchive`, `Get-HelperPath` (solo acepta `Innounp`), `Invoke-ExternalCommand` (ejecuta con `2>&1`, opcionalmente vuelca a `LogPath` en UTF-8, lanza `"<ruta> falló (<código>): <salida>"` si el código no es 0).

## Cómo se pasan las variables y el contexto

1. `run_script` construye un `dict` con `root`, `python`, `dir`, `persist_dir`, `app`, `bucket`, `bucketsdir`, `version`, `manifest`, `fname` y `script` (las líneas unidas con `\n`). Para `installer.file` añade `file` y `arguments`.
2. Se escribe como JSON UTF-8 (`ensure_ascii=False`) en `payload.parent/hook-context.json`. El fichero queda fuera del payload y no se publica.
3. Se escribe el runner en `payload.parent/<hook>.ps1` (`pre_install.ps1`, `installer.file.ps1`, `installer.script.ps1`), en UTF-8 con BOM.
4. `$context.python` = `Path(sys.executable).with_name('python.exe')`. Es el intérprete de MITO.

### Argumentos de `installer.file`

Se expanden con `split(arguments)` (de `arguments.py`) y después un único `re.sub` con este patrón (el orden de alternativas importa):

```
\$\{(dir|original_dir|persist_dir|version|architecture|global)\}|\$(dir|original_dir|persist_dir|version|architecture|global)\b
```

Valores: `dir` y `original_dir` = payload; `persist_dir`; `version`; `architecture` = `64bit`; `global` = `False`. Las variables `${…}` siempre se sustituyen. `$dir_suffix` no se sustituye (la `\b` lo impide). `$(throw "…")` se pasa literal. El PowerShell del hook recibe los argumentos en `$context.arguments` y los pasa con `& $context.file @installerArgs`.

## Entorno del proceso

Se parte de `os.environ` con estos cambios, en este orden:

1. `PATH` = `MITO_HOST_PATH` si existe; si no, el `PATH` actual.
2. Se eliminan las variables listadas en `MITO_ACTIVE_VARS` (separadas por `;`).
3. `proxy_environment(root, env)` (de `network.py`) añade los proxies configurados.
4. Se fijan `HOME`, `USERPROFILE` = `root/home`; `APPDATA` = `home/AppData/Roaming`; `LOCALAPPDATA` = `home/AppData/Local`; `TEMP`, `TMP` = `home/tmp`; `HOMEDRIVE`, `HOMEPATH`; `MITO_ROOT` = `root`.
5. Se eliminan `MITO_PROFILE` y `MITO_ACTIVE_VARS`.

Ejecución: `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <runner> <context.json>`, con `cwd=payload`, `capture_output=True`, `text=True`, `encoding='utf-8'`, `errors='replace'`, `creationflags=CREATE_NO_WINDOW`. Sin `timeout`.

Persistencia de aislamiento: `isolated({'manifest': manifest})` decide `persist = payload.parent/'persist'`; si no, `root/home/persist/<key>` (con `key` = `bucket/app`, es decir, dos niveles). Se crea con `mkdir(parents=True, exist_ok=True)` antes de ejecutar.

## Salida, código de salida y mensajes

- Antes de ejecutar: `log(tr('{p0}: ejecutando {hook}', p0=key, hook=hook))`.
- Cada línea de stdout: `log(f'{key}: {línea}')` (sin `tr`).
- Código distinto de 0: `YitoError(tr('{p0}: {hook} falló ({p2}): {p4}', p0=key, hook=hook, p2=returncode, p4=stderr.strip()))`.
- Stderr no vacío con código 0: `log(f'{key}: {stderr.strip()}')` (sin `tr`).
- `OSError` al lanzar `powershell.exe`: `YitoError(tr('{p0}: no se pudo ejecutar {hook}: {p2}', …))`.
- Tras éxito, cualquier `path.is_symlink() or path.is_junction()` en `payload.rglob('*')` provoca `YitoError(tr('{p0}: {hook} creó un enlace no gestionado: {p2}', p2=path.name))`.
- `installer.file` con código distinto de 0: el script hace `exit $LASTEXITCODE`, así que el código se propaga. Si el error es de PowerShell (`throw`), el `catch` escribe `$_.ToString()` en stderr y sale con 1.

Con `file`, `run_installer` ya ha comprobado `inside(payload, installer['file']).is_file()`. Si falla: `YitoError(tr('No existe {p1}', p1=installer['file']))`.

## Protocolo de los helpers `Expand-*`

`Invoke-MitoExpand(Kind, Name, Path, DestinationPath, ExtractDir, Removal)`:

- Si `DestinationPath` está vacío, se usa `Split-Path $Path` (la carpeta del archivo).
- Ejecuta `& $context.python -B -I -m mito.hooks [Kind] <root> <dir> <Path> <DestinationPath> <ExtractDir>`. Con `-I` (modo aislado de Python), `-B` (sin `.pyc`).
- Si el código no es 0: `throw "$Name falló"`. El mensaje de Python sale por stderr del proceso, no en el texto del error PowerShell.
- Con `-Removal` borra `Path` tras éxito (`Remove-Item -LiteralPath -Force`).

| Helper PowerShell | `Kind` | `ExtractDir` por defecto | Función Python | Extractor |
|---|---|---|---|---|
| `Expand-7zipArchive` | `''` | `.` | `expand` | `archives.extract_7zip` |
| `Expand-InnoArchive` | `inno` | `{app}` | `expand_inno` | `inno.extract_inno` |
| `Expand-DarkArchive` | `dark` | `.` (no es parámetro) | `expand_installer('dark')` | `burn.extract_burn` |
| `Expand-MsiArchive` | `msi` | `.` | `expand_installer('msi')` | `msi.extract_msi` |

Despacho en `__main__`:

- `argv[1]` en `('dark','msi')`: `expand_installer(argv[1], argv[2:6]…, argv[6])`. Es decir, `root, payload, archive, destination` y luego `extract_dir`.
- `argv[1] == 'inno'`: `expand_inno(argv[2:6]…, argv[6])`.
- Otro caso: `expand(argv[1:5]…, argv[5])`. Aquí `argv[1]` es `root` (no empieza por un `Kind`).

Todas las rutas se convierten con `Path(arg)`. El mensaje de error se escribe en stderr y el código de salida es 1.

### Qué hace cada helper en Python

- `expand`: `resolve()` del payload, archivo y destino. Si `archive` o `destination` no están dentro de `payload` (`is_relative_to`): `YitoError(tr('Expand-7zipArchive requiere rutas dentro del payload'))`. Crea un `TemporaryDirectory(dir=payload.parent)`. Llama `extract_7zip(archive, expanded, root)` con sus límites por defecto. `source = inside(expanded, extract_dir)`; si no es directorio: `YitoError(tr('Expand-7zipArchive: ExtractDir no encontrado'))`. Copia con `shutil.copytree(source, destination, dirs_exist_ok=True)`.
- `expand_inno`: mismas comprobaciones de rutas. El mensaje se obtiene traduciendo la cadena `Expand-7zipArchive…` y reemplazando después `Expand-7zipArchive` por `Expand-InnoArchive` (ver Dudas). `inno_root(extract_dir)`: si es `None`, `YitoError` con `ExtractDir no encontrado`. Luego `extract_inno(archive, expanded, root, constant)` y `copytree`.
- `expand_installer(kind, …)`: mensaje no traducido `f'Expand-{kind}Archive requiere rutas dentro del payload'`. `dark` → `extract_burn(…, log=print)`; `msi` → `extract_msi(…, log=print)`; otro → `f'Extractor no soportado: {kind}'`. `source = inside(expanded, extract_dir)`; si no es directorio, `f'Expand-{kind}Archive: ExtractDir no encontrado'` (no traducido). Copia con `copytree`.

Los extractores `burn` y `msi` escriben su `log` con `print`, así que sus líneas salen por stdout del proceso Python y el runner las vuelca al log de MITO.

## Flujo de ejecución (contexto del motor)

- Orden en `engine._install_payload`: descarga → extracción → `prepare_resources` (ver `bucket_resources.md`) → `run_pre_install` → `run_installer` → validación de `bin` → publicación.
- `pre_install` fallido impide `installer` (test `test_pre_install_failure_prevents_installer_script`).
- Un fallo en `pre_install` o `installer` no publica ni cambia el perfil (test `test_pre_install_failure_does_not_publish_or_change_profile`).
- `run_script` retorna sin ejecutar si `script` está vacío y no hay `file`. Por eso `installer.script = ''` o `[]` son válidos (ver `catalog.installer_definition`).

## Invariantes

1. `post_install` no se ejecuta nunca. Se registra en el log `"<key>: post_install omitido por la política general de MITO"` (`engine.py:250`).
2. El runner se ejecuta con `cwd` y `$dir` = payload, y las variables de entorno de `HOME`/`USERPROFILE`/`TEMP` apuntan a `root/home`, nunca al perfil real del usuario.
3. El contexto JSON y el runner nunca están dentro del payload: se escriben en `payload.parent`.
4. Un hook no puede dejar enlaces ni junctions en el payload tras ejecutarse.
5. `installer.file` solo acepta `.ps1` dentro del payload. Lo valida `catalog.installer_definition` antes de descargar.
6. Los helpers `Expand-*` solo escriben dentro del payload (comprobado en Python con `is_relative_to`).
7. `Get-HelperPath` solo resuelve `innounp`. Cualquier otro helper lanza `Helper no soportado: <nombre>`.
8. Ejecución de MSI: `msi.py` lee la base en modo solo lectura y nunca ejecuta acciones (test `test_msi`: `MustNeverRun`).

## Casos límite y errores

- Argumentos `installer.args` no texto ni lista de textos: `catalog` lo rechaza antes de descargar con `tr('{hook} debe ser texto o una lista de líneas', hook='installer.args')`.
- `installer.file` con extensión distinta de `.ps1` (`setup.exe`, `setup.msi`), con rutas fuera del payload (`../outside.ps1`, `C:/outside.ps1`), o con claves no soportadas (`script`, `keep`): rechazado antes de descargar (`Pasos no soportados: installer.file` o `installer.<clave>`).
- `installer.file` ausente del payload: `No existe <file>`.
- Código de salida 19 de `installer.file`: el mensaje es `…installer.file falló (19): …`.
- Un hook que lanza `throw` en `installer.script` produce `installer.script falló (1): <stderr>`.
- Runner de `pre_install` con lista de líneas: se une con `\n` antes de escribir el JSON.
- Si `file` y `script` aparecen a la vez, `installer_definition` lo rechaza antes.

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_hook_helper_extracts_app_inside_payload_only` | `test_inno.py:19-43` | `expand_inno` pasa `{app}` por defecto y `{code_GetDestDir}` si se indica; `outside` (fuera del payload) y `{sys}` fallan. |
| `test_hook_helpers_are_confined_and_msi_uses_existing_extractor` | `test_burn.py:161-176` | `expand_installer('msi')` llama una sola vez a `extract_msi`; `payload/tool.exe` aparece; destino fuera del payload falla para `msi` y `dark`. |
| `test_installer_runner_exposes_generic_inno_helpers` | `test_engine.py:490-497` | El texto de `SCRIPT` contiene `function Expand-InnoArchive`, `Get-HelperPath`, `Invoke-ExternalCommand`, `@('-B', '-I', '-m', 'mito.hooks')` y `Invoke-MitoExpand 'inno' 'Expand-InnoArchive'`. |
| `test_no_component_exemptions_for_hooks` | `test_engine.py:384-390` | Un `installer.file` no `.ps1` da `Pasos no soportados: installer.file` para cualquier componente. |
| `test_pre_install_runs_and_post_install_is_ignored_for_every_component` | `test_engine.py:392-402` | `pre_install` escribe `prepared.txt`; `post_install` se omite con el log `post_install omitido` y se registra `ejecutando pre_install`. |
| `test_jar_and_icon_install_preserves_bytes_and_launches_profile_java` | `test_engine.py:404-447` | Un `pre_install` crea `tool.cmd` con `javaw -jar`; los archivos `.jar`/`.ico` se conservan sin extraer. |
| `test_installer_script_nested_archive_order_context_and_architecture` | `test_engine.py:455-488` | Orden `pre_install` antes de `installer.script`, y antes de `Validando`; `$fname`, `$architecture`, `$global`, `$original_dir`, `$manifest.version` correctos; `-Removal` borra el 7z. |
| `test_installer_unsupported_fields_and_malformed_scripts_fail_before_download` | `test_engine.py:499-511` | Formas no válidas de `installer` fallan antes de `download`; `None`, `{}`, `{'script':''}`, `{'script':[]}` son `Compatible`. |
| `test_installer_failure_preserves_installed_version_and_profile` | `test_engine.py:513-528` | Un `installer.script` que modifica `bin/tool.exe`, borra o crea junction no cambia la instalación previa. |
| `test_pre_install_failure_prevents_installer_script` | `test_engine.py:530-536` | `pre_install` falla → `pre_install falló` y `run_installer` no se llama. |
| `test_pre_install_failure_does_not_publish_or_change_profile` | `test_engine.py:538-545` | Con `pre_install` fallido, el perfil y el registro quedan vacíos. |
| `test_extensionless_download_renamed_by_real_pre_install` | `test_engine.py:547-561` | `$fname` (`download` y `Tool File`) permite renombrar con `pre_install`. |
| `test_pre_install_multiple_download_names_and_extract_to` | `test_engine.py:563-574` | `$fname` es array con dos URL; `extract_to` por URL. |
| `test_download_names_are_shared_and_confined` | `test_engine.py:590-601` | `download_filename` rechaza `#/../escape`, `#/NUL`, `#/a%5Cb`, etc., y el manifiesto queda sin compatibilidad. |
| `test_pre_install_nested_archive_helper` | `test_engine.py:603-623` | `Expand-7zipArchive` desde `pre_install`; `expand` fuera del payload → `dentro del payload`. |
| `test_installer_script_pipes_archives_into_expand_helper` | `test_engine.py:625-647` | `Get-Item … \| Expand-7zipArchive -DestinationPath` (pipeline por `FullName`). |
| `test_pre_install_persist_scope_and_architecture_override` | `test_engine.py:649-664` | `$persist_dir` compartido o aislado; `$env:USERPROFILE` apunta a `root/home`. |

## Dependencias

- `i18n.tr` (líneas 136, 184, 191, 195, 200 y los helpers de `expand*`).
- `storage`: `YitoError`, `inside`.
- `catalog`: `installer_definition`, `array`. `catalog.installer_definition` valida la forma de `installer`.
- `transfer.download_filename`.
- `network.proxy_environment`, `isolation.isolated`, `arguments.split`.
- `archives.extract_7zip` (`expand`), `inno.extract_inno`/`inno_root` (`expand_inno`), `burn.extract_burn` y `msi.extract_msi` (`expand_installer`).
- Llamadores: `engine.py:419` (`run_pre_install`), `engine.py:422` (`run_installer`), `engine.py:413-414`.

## Notas para Rust

- Mantener el preámbulo PowerShell tal cual, como cadena de recursos (`include_str!` de un `.ps1` versionado o constante). Cambiar una línea cambia el contrato con los catálogos existentes.
- El protocolo `python -m mito.hooks …` debe reproducirse con `mito.exe` y un subcomando interno (p. ej. `mito.exe __expand <kind> <root> <payload> <archive> <destination> <extract_dir>`). No es un cambio de formato de disco, pero sí de la ruta de ejecución de los `Expand-*`. Hay que documentarlo en ADR.
- Crate `std::process::Command` con `creation_flags(0x08000000)` (CREATE_NO_WINDOW), `current_dir(payload)` y `env_clear()` + la lista exacta de variables del original.
- Los `log` de stdout y stderr del hook se registran tal cual en `logs/mito.log`.
- `hook-context.json` y `<hook>.ps1` van en la carpeta de staging, no en el payload.

## Dudas

1. `$context.python` es `sys.executable` con `python.exe`. En un MITO empaquetado, ¿qué intérprete corresponde? Hay que definir el subcomando Rust equivalente.
2. `python -I -m mito.hooks` debe encontrar el paquete `mito`. Con `-I` no entra el directorio actual ni `PYTHONPATH`. El original no dice cómo se resuelve esto. Confirmar el mecanismo (`.pth` o instalación) antes de portarlo.
3. `run_script` hereda todo `os.environ` salvo lo indicado. Variables como `PYTHON*` del host pasan al hook. ¿Es intencional?
4. `expand_inno`: el mensaje de error se obtiene traduciendo `Expand-7zipArchive…` y reemplazando luego el nombre del helper. Si la traducción cambia la cadena, el reemplazo no ocurre. Rust debe usar una clave propia por helper.
5. `Invoke-MitoExpand` no captura el mensaje de Python. Solo se ve en stderr del proceso. ¿Debe Rust incluirlo en el error `"<Name> falló"`?
6. `installer.args` ausente (`None`): `split(None)` depende de `arguments.py`. Verificar que el original no falla con `installer` solo `{'file': …}` sin `args` (el test `test_installer_files` siempre pasa `args`).
7. `run_installer` usa `installer.get('args')` para `installer.file`, pero en `catalog.installer_definition` `args` solo se valida si existe. Confirmar el caso `args` = `[]`.
8. `Expand-DarkArchive` no acepta `-ExtractDir` (fija `.`). Mantener así.
9. El check de enlaces (`is_junction`) solo corre tras éxito. Un hook que falla y deja un junction no se detecta, pero el staging se descarta igual.
