# catalog.py → mito-catalog::catalog

Origen: `core/app/mito/catalog.py` (262 líneas), `core/tests/test_catalog_performance.py` (4 tests). Los tests de otros archivos que ejercen la compatibilidad, `resolve` o la huella se listan en la tabla de casos de test.

## Responsabilidad
Lee los manifiestos Scoop guardados en los snapshots de bucket (`core/buckets/<bucket>.json`), los identifica como `bucket/app`, los normaliza con `effective`, calcula su huella (`fingerprint`) y decide si MITO puede instalarlos (`compatibility`, que devuelve `"Compatible"` o un mensaje de rechazo). La importación, el refresco y la configuración de buckets están en `buckets.md`.

## API pública del original
| Función / método | Entradas | Salida | Errores |
|---|---|---|---|
| `array(value)` (catalog.py:17-18) | cualquier valor | `[]` si `None`; el mismo valor si es lista; `[value]` en otro caso | ninguno |
| `effective(manifest)` (catalog.py:21-27) | `dict` | copia superficial del `dict`; si hay `architecture`, se sustituye cada campo de `architecture['64bit']` sobre la raíz | `YitoError('No hay versión Windows x64')` si `architecture` no tiene clave `64bit` |
| `fingerprint(manifest)` (catalog.py:30-31) | `dict` | hexadecimal SHA-256 (64 caracteres, minúsculas) del JSON canónico del manifiesto | ninguno |
| `installer_definition(manifest)` (catalog.py:34-57) | `dict` | `None` si no hay `installer` (o es `{}`); si no, el propio `dict` `installer` | `YitoError` con `Pasos no soportados: …` o `installer.args debe ser texto o una lista de líneas` (ver reglas más abajo) |
| `Catalog(root)` (catalog.py:60-63) | ruta raíz de MITO | — (`folder = root/core/buckets`) | ninguno |
| `Catalog.sources()` (catalog.py:65-73) | — | `dict` nombre → origen, leído de `config.properties` | `YitoError` si un nombre de bucket no es válido (detalle en buckets.md) |
| `Catalog.import_bucket(name, source)` (catalog.py:75-97) | nombre, origen | nº de manifiestos importados | ver buckets.md |
| `Catalog._archive_manifests(archive)` (catalog.py:99-133, estático) | ruta a ZIP | `dict` nombre → manifiesto | ver buckets.md |
| `Catalog.all()` (catalog.py:135-138) | — | iterador de `("bucket/app", manifiesto)` en orden de nombre de archivo y de clave | `JSON` inválido en un snapshot se propaga (no se captura) |
| `Catalog.refresh_installed(inventory, log)` (catalog.py:140-142) | inventario (`dict` con `record['key']`) | nº de claves refrescadas | nunca (los errores por clave se registran) |
| `Catalog.refresh_keys(keys, log)` (catalog.py:144-189) | claves `bucket/app` | nº de claves refrescadas | nunca (los errores por clave se registran) |
| `Catalog.resolve(name)` (catalog.py:191-195) | `"bucket/app"` o `"app"` | `(key, manifiesto)` | `YitoError('Componente no encontrado o ambiguo: {name}. Usa bucket/app.')` si no hay exactamente una coincidencia |
| `Catalog.compatibility(key, manifest)` (catalog.py:197-262) | clave, manifiesto (sin `effective` previo) | `str`: `"Compatible"` o un mensaje de rechazo | nunca: captura `KeyError`, `TypeError`, `ValueError` y `YitoError` y devuelve un mensaje |

## Formatos de disco

### Manifiesto Scoop (campos que lee MITO)
El manifiesto es un objeto JSON. Ejemplo sintético con el formato que espera el código (no proviene de un bucket real):

```json
{
  "version": "1.2.3",
  "description": "Herramienta de ejemplo",
  "notes": ["Ejemplo sintético"],
  "license": "MIT",
  "url": ["https://example.org/tool.zip"],
  "hash": ["sha256:0000000000000000000000000000000000000000000000000000000000000000"],
  "architecture": { "64bit": { "url": "https://example.org/tool-x64.zip" } },
  "bin": [["bin/tool.exe", "tool", "--modo rapido"]],
  "shortcuts": [["tool.exe", "Herramienta", "", "icono.ico"]],
  "env_set": { "TOOL_HOME": "$dir" },
  "env_add_path": ["bin"],
  "persist": [["data", "data"]],
  "extract_dir": "tool-1.2.3",
  "extract_to": "bin",
  "depends": "main/otra-app",
  "innosetup": false,
  "pre_install": ["Write-Output 'preparando'"],
  "installer": { "script": ["Write-Output 'instalando'"] },
  "post_install": "omitido por política",
  "psmodule": false
}
```

Campos que lee el original y dónde:

| Campo | Lectura en el original | Uso |
|---|---|---|
| `version` | catalog.py:175-177, 205; components.py:63-66; engine.py:270, 294, 330, 430, 440; hooks.py:169, 175 | obligatorio para compatibilidad; `str()` y luego `identifier` (ver reglas) |
| `url` | catalog.py:224-233; engine.py:293, 324; hooks.py:166 | obligatorio; cadena o lista; define el nombre local de descarga |
| `hash` | catalog.py:227; engine.py:293, 325 | cadena o lista de la misma longitud que `url`; ver «Reglas de hash» |
| `bin` | catalog.py:246-254; engine.py:445 | cadena o lista `[ejecutable, alias, argumentos?]` |
| `shortcuts` | catalog.py:234-240; engine.py:411, 477, 505; shortcuts.py:61; isolation.py:14 | lista de listas `[ejecutable, etiqueta, argumentos, icono?]`; sólo se valida el índice 3 |
| `env_set` | catalog.py:255-259; engine.py:471 | objeto `nombre → valor` |
| `env_add_path` | catalog.py:240-243; engine.py:465; pocketools.py:89 | cadena o lista de rutas relativas |
| `persist` | catalog.py:212-223; persistence.py:12, 37; isolation.py:14; ui.py:474 | cadena o lista de cadenas o `[origen, destino]` |
| `depends` | engine.py:255; components.py:234 | cadena o lista de claves `bucket/app` (no se valida en `compatibility`) |
| `architecture` | catalog.py:21-27; ui.py:1254-1262 | objeto que puede contener `64bit`; se fusiona sobre la raíz |
| `extract_dir`, `extract_to` | catalog.py:240-243; engine.py:347-361 | rutas relativas; validadas con `portable_path` |
| `innosetup` | catalog.py:210-211; engine.py:350, 353, 409 | booleano |
| `pre_install` | catalog.py:202-204; hooks.py:125; bucket_resources.py:12 | cadena o lista de líneas |
| `installer` | catalog.py:34-57; engine.py:421; ui.py:500-502 | `{ "script": … }` o `{ "file": "*.ps1", "args": … }` |
| `post_install` | engine.py:248-250 (sólo se registra como omitido); ui.py:504, 1261 | no se valida ni se ejecuta |
| `psmodule` | catalog.py:244-245 | debe ser falso o ausente |
| `description` | search.py:27; ui.py:454 | texto |
| `notes` | search.py:28; ui.py:493, 1250 | texto o lista de textos |
| `license` | ui.py:20-22 | cadena, o objeto con `identifier` o `url` |
| `autoupdate` | versions.py:258 (no es de compatibilidad) | objeto del mecanismo de autoactualización (fuera de este módulo) |

### Huella (`fingerprint`)
- Cálculo: `sha256(json.dumps(manifest, sort_keys=True, separators=(',', ':')).encode()).hexdigest()` (catalog.py:31). `json.dumps` usa `ensure_ascii=True` por defecto, así que los caracteres no ASCII se escriben como `\uXXXX`.
- Se calcula sobre el manifiesto tal y como viene del snapshot, **no** sobre `effective(manifest)` (engine.py:266, 270, 285, 430).
- Donde se guarda: en `.mito.json` del payload, clave `manifest_sha256` y dentro de `record` (engine.py:430-431). Ejemplo de la forma que escriben los tests:

```json
{
  "key": "main/tool",
  "manifest_sha256": "<64 caracteres hex>",
  "record": {
    "key": "main/tool",
    "version": "1.0",
    "path": "core/apps/main/tool",
    "manifest": { "version": "1.0" },
    "manifest_sha256": "<64 caracteres hex>"
  }
}
```

- Comprobaciones: al activar (engine.py:568) se compara `marker['manifest_sha256']`, `lock['manifest_sha256']` y `fingerprint(lock['manifest'])`; si difieren, el entorno falla con un mensaje que contiene `incoherente` (test_engine.py:675). El plan de instalación compara la huella instalada con la del manifiesto del catálogo (engine.py:285; el efecto exacto de esa comparación se documenta en el contrato de engine). Al actualizar, components.py:225 compara las huellas.
- `storage_migration.py:18-20` recalcula la huella y crea alias `clave@<primeros 16 caracteres>`.

## Invariantes
1. `compatibility` no accede al disco ni resuelve rutas del host. Toda validación de ruta es sintáctica (test_catalog_performance.py:14-22).
2. `"Compatible"` es la única cadena que significa instalable. Cualquier otra es un mensaje de rechazo que se muestra tal cual. Consumidores: engine.py:252-253 (la planificación aborta con `f'{key}: {status}'`), ui.py:1222 (`Disponible` si es `Compatible` y no está instalado; `Todavía no soportado` en otro caso) y components.py:192-197 (`pending_updates` sólo considera `Compatible`).
3. La compatibilidad se aplica a cada componente resuelto del plan, incluidas las dependencias, antes de cualquier descarga (engine.py:251-253 dentro de la resolución recursiva; test_engine.py:283-287).
4. `effective` se aplica antes de validar. Los campos de `architecture.64bit` sustituyen por completo al campo de la raíz (fusión superficial: `dict.update`). Una clave `env_set` en `64bit` reemplaza el `env_set` entero (catalog.py:26).
5. Toda ruta del manifiesto pasa por `portable_path` (storage.py:24-32): sin unidad ni raíz, sin `..`, sin `:` en ninguna parte, sin `<>"|?*` ni caracteres de control, sin espacio o punto al final de una parte, y sin nombres reservados (`CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, comparando la parte antes del primer punto en mayúsculas).
6. `version` convertido con `str()` debe cumplir `identifier` (storage.py:16-21): regex `[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}`, sin punto final, sin nombre reservado. Se normaliza a minúsculas.
7. `resolve(name)` exige exactamente una coincidencia: `name` igual a la clave completa, o, si `name` no contiene `/`, el nombre de app igual a `name`, comparado sin normalizar (catalog.py:192).
8. Un snapshot que contiene una clave `bucket/app` no cambia por lo que se haga con los payloads; la huella del payload la fija el manifiesto con que se instaló (engine.py:430).

## Reglas de `compatibility` (en orden; la primera que falla decide)
Entrada: `key`, `manifest`. Todo lo siguiente va dentro de un `try`. Si se lanza `KeyError`, `TypeError`, `ValueError` o `YitoError` en cualquier punto, el resultado es `"Manifiesto no soportado: " + <texto del error>` (catalog.py:261-262). Los mensajes de la tabla son las cadenas que pasan por `tr()`; los tests comparan con las cadenas en español.

| Paso | Regla | Resultado si falla | Referencia |
|---|---|---|---|
| 0 | `m = effective(manifest)`. Si hay `architecture` sin `64bit`, error. | `Manifiesto no soportado: No hay versión Windows x64` | catalog.py:23-26 |
| 1 | `required_buckets(key, m)` sobre `pre_install` e `installer`. Una referencia `$bucketsdir` sin nombre estático lanza error. | `Manifiesto no soportado: Pasos no soportados: referencia dinámica a $bucketsdir` | catalog.py:199-201; bucket_resources.py:11-23 |
| 2 | `pre_install`, si existe, debe ser texto o lista de textos. Ausente o `null` es válido (`null` pasa el `is not None`). | `pre_install debe ser texto o una lista de líneas` (sin prefijo) | catalog.py:202-204 |
| 3 | `identifier(str(m['version']))`. Sin clave `version`, `KeyError`. | `Manifiesto no soportado: 'version'` o `Manifiesto no soportado: Nombre no válido: '<valor>'` | catalog.py:205 |
| 4 | `installer_definition(m)`. Su mensaje se devuelve sin prefijo. | el mensaje de `installer_definition` (ver reglas de installer) | catalog.py:206-209 |
| 5 | Si existe `innosetup` y no es booleano. | `Pasos no soportados: innosetup` | catalog.py:210-211 |
| 6 | Para cada elemento de `definitions(m)` (persistence.py:11-18): `portable_path(origen)` y `portable_path(destino)`; si alguno es `.` o vacío, error; si el origen se solapa con uno ya visto, error. Solapamiento: igualdad, o uno es ancestro del otro, comparando como `PureWindowsPath` (sin distinguir mayúsculas). | `persist requiere una ruta concreta`; `persist contiene rutas solapadas`. Un elemento lista de longitud distinta de 2 produce `Manifiesto no soportado: persist debe ser una ruta o [origen, destino]` | catalog.py:212-223 |
| 7 | `urls = array(m.get('url'))`. Si la lista está vacía, error. (Una cadena vacía `""` no está vacía: `array("")` es `[""]`.) | `Falta URL de descarga` | catalog.py:224-226 |
| 8 | `download_hashes(urls, m.get('hash'))` (transfer.py:96-101). Con número distinto de hashes, error. Cada hash válido según `hash_verifier`. | `Manifiesto no soportado: La cantidad de hashes debe coincidir con las URL` u otro texto de `hash_verifier` | catalog.py:227; transfer.py:74-101 |
| 9 | Para cada URL en orden: el esquema debe ser `http` o `https`. Después, `download_filename(url).lower()`; si tiene extensión (`Path(...).suffix` no vacío) y no termina en una de las extensiones permitidas, error. | `Requiere HTTP o HTTPS`; `Formato de archivo todavía no soportado` | catalog.py:228-233 |
| 10 | Para cada entrada `shortcuts` que sea una lista de longitud mayor que 3 con el índice 3 verdadero: el icono debe ser texto con extensión `.ico`, `.exe` o `.dll` (sin distinguir mayúsculas); después `portable_path(icono)`. Las entradas que no son listas o tienen longitud ≤ 3 no se validan aquí. | `Formato de icono no soportado para el acceso directo: <icono>` | catalog.py:234-239 |
| 11 | Para `extract_dir`, `extract_to` y `env_add_path`: cada elemento no nulo de `array(valor)` pasa por `portable_path`. | `Manifiesto no soportado: <error de portable_path>` | catalog.py:240-243 |
| 12 | Si `psmodule` es verdadero. | `Pasos no soportados: psmodule` | catalog.py:244-245 |
| 13 | Para cada elemento de `array(bin)`: si es lista, su longitud debe ser 2 o 3; `ejecutable, alias = item[:2]`. Si no es lista, `alias = stem(item con '\' cambiado por '/')`. Después `portable_path(ejecutable)` y `command_name(alias)`. | `bin debe tener ejecutable, alias y argumentos opcionales`; errores de `portable_path` o `command_name` con prefijo | catalog.py:246-254; storage.py:43-52 |
| 14 | Para cada `env_set` `(nombre, valor)`: `nombre.upper()` no puede estar en la lista reservada, y el nombre debe cumplir `[A-Za-z_][A-Za-z0-9_]*`. Después, en el valor se eliminan las subcadenas `${dir}`, `$dir`, `${persist_dir}`, `$persist_dir` (en ese orden); si queda algún `$` o hay algún `%`, error. | `Variable reservada: <nombre>`; `Variable no soportada: <nombre>` | catalog.py:255-259 |
| 15 | Ningún error. | `Compatible` | catalog.py:260 |

Lista reservada de nombres de `env_set` (catalog.py:256, comparada en mayúsculas): `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATH`, `COMSPEC`, `SYSTEMROOT`, `WINDIR`, `HOMEDRIVE`, `HOMEPATH`, `MITO_ROOT`, `MITO_PROFILE`.

Extensiones de URL permitidas (catalog.py:232, comparadas en minúsculas): `.zip`, `.7z`, `.rar`, `.tar`, `.tar.gz`, `.tgz`, `.tar.xz`, `.tar.bz2`, `.nupkg`, `.msi`, `.exe`, `.dll`, `.jar`, `.ico`, `.ps1`, `.cmd`, `.bat`, `.com`. Una URL sin extensión (p. ej. `…/download?format=zip`) se acepta. `.iso` se rechaza.

### Reglas de `installer_definition` (catalog.py:34-57)
1. `installer` ausente, `None` o `{}`: no hay instalador (devuelve `None`).
2. Si no es un `dict`: `Pasos no soportados: installer`.
3. Si contiene la clave `file`, las claves permitidas son `file` y `args`; si no, sólo `script`. Cualquier otra clave: `Pasos no soportados: installer.<clave>` (varias claves ordenadas y separadas por `, `).
4. Con `file`: debe ser texto que termine en `.ps1` (sin distinguir mayúsculas), y `portable_path(file)`. Si `args` existe, debe ser texto o lista de textos; si no: `installer.args debe ser texto o una lista de líneas`. Devuelve el `dict`.
5. Sin `file`: `script` debe ser texto o lista de textos. Si falta, es `None` y se rechaza. `""` y `[]` son válidos. Si no: `installer.script debe ser texto o una lista de líneas`.

### Reglas de hash (transfer.py:74-101)
- `None`, `""`, `"skip"` (sin distinguir mayúsculas, con espacios recortados) y `"   "` no declaran hash.
- Una lista declara un hash por URL. Una lista de longitud distinta de la de URLs se rechaza. Un elemento `"skip"` o `None` se acepta como sin hash.
- Un valor de hash con `alg:` (algoritmo explícito) o sin prefijo (se deduce por la longitud: 32 md5, 40 sha1, 64 sha256, 128 sha512) se valida por `hash_verifier`. El detalle de `hash_verifier` está en transfer.py y se usa desde `mito-transfer`.

## Casos límite y errores
- `url` ausente o `null`: `Falta URL de descarga`. `url: ""`: el esquema es vacío, por tanto `Requiere HTTP o HTTPS`.
- `version` numérico o booleano: `str(1.0)` es `"1.0"` y `str(True)` es `"True"`, que pasa la regla de `identifier` (Duda: `true` como versión se acepta en la compatibilidad; ver la nota en `refresh_keys`).
- `version` con caracteres como `+` (p. ej. `1.0+build`): no cumple `identifier`, rechazo `Manifiesto no soportado: Nombre no válido: '1.0+build'`.
- `architecture` sin `64bit` o con un valor no diccionario: error en el paso 0 (`TypeError` o `ValueError` según el caso; siempre con prefijo `Manifiesto no soportado`).
- `persist` solapados como `data` y `data/sub` (cualquier orden): `persist contiene rutas solapadas`. El solapamiento se comprueba sólo para el origen, no para el destino (Duda: el original no comprueba destinos).
- `shortcuts` con icono `icon.dll,0`: la extensión es `.dll,0`, rechazada. Icono como objeto `{"path": "..."}`: rechazado por no ser texto.
- `bin` como entero u objeto (no texto ni lista): `item.replace(...)` lanza `AttributeError`, que **no** está en la lista de excepciones capturadas. El error se propaga fuera de `compatibility` (Duda: el original no lo define; decidir si se convierte en `Manifiesto no soportado`).
- `env_set` no diccionario, o valor no textual: `.items()` o `value.replace(...)` lanzan `AttributeError`, sin captura (mismo caso que el anterior).
- `post_install` con cualquier valor: no se valida. Engine sólo lo registra como omitido (engine.py:248-250).
- `depends` con ciclo o referencias ausentes: no se valida aquí. Se detecta en la planificación (engine.py:244-245, `Dependencia circular`; `Componente no encontrado` vía `resolve`).
- `innosetup`: sólo booleano. Valores `"true"` o `1` se rechazan.
- `installer` con `file` y `args` no textual: mensaje de `installer.args`, sin prefijo.
- `refresh_keys` y `refresh_installed`: un manifiesto que no sea `dict`, o cuyo `version` no sea texto, número entero o flotante, se rechaza con `Manifiesto sin versión válida` (catalog.py:175-176). Un booleano pasa esa comprobación (`bool` es subclase de `int` en Python) y después `identifier(str(...))` lo acepta como `True`. Duda.
- `resolve`: con dos buckets que tienen la misma app, `resolve('app')` falla por ambigüedad; `resolve('bucket/app')` no.
- Una clave `bucket/app` sin `/`, o con más de una, en `refresh_keys`: `ValueError` capturado y registrado (catalog.py:150, 185).

## Casos de test del original
| Test (archivo:línea) | Qué comprueba |
|---|---|
| `test_classification_does_not_resolve_host_paths` (test_catalog_performance.py:14-22) | Con `Path.resolve` parcheado para lanzar error, un manifiesto válido con `bin`, `persist`, `env_add_path` y `extract_dir` devuelve `Compatible`. Con `extract_dir` igual a `../escape`, `C:/absolute`, `/rooted`, `\rooted`, `file:stream`, `CON` o `trailing.` no devuelve `Compatible`. |
| `test_inside_still_checks_physical_confinement` (test_catalog_performance.py:24-29) | `storage.inside` (no `compatibility`) lanza `YitoError` si tras `resolve` la ruta sale de la raíz. Se incluye porque la clasificación depende de la misma regla de rutas. |
| `test_archive_layouts_and_no_extraction` (test_catalog_performance.py:31-39) | `_archive_manifests` acepta ZIP con `repo/` y `repo/bucket/`, lee `tool.json` con BOM, ignora `repo/docs/ignored.json` no válido y no escribe nada en disco (ver buckets.md). |
| `test_bad_archives_preserve_previous_snapshot` (test_catalog_performance.py:41-59) | ZIP malos (`repo/../escape`, `repo/CON`, `repo/bucket/TOOL.json` duplicado, `repo/link` simbólico) hacen fallar `import_bucket` y dejan el snapshot anterior byte a byte (ver buckets.md). |
| `test_architecture_overrides_common_fields` (test_engine.py:102-105) | `effective` sustituye `url` por el de `architecture.64bit` y conserva `env_set` de la raíz. |
| `test_scripts_and_persist_not_silently_ignored` (test_engine.py:107-109) | `installer.file` y `env_set.HOME` devuelven un estado distinto de `Compatible`. |
| `test_command_names_and_generated_batch_preserve_literal_aliases` (test_engine.py:211-220) | Alias de `bin` con espacios, `&`, `%`, `!` y guion inicial son `Compatible`. |
| `test_ambiguous_bucket_requires_namespace` (test_engine.py:327-331) | `resolve('tool')` con dos buckets que tienen `tool` falla con «ambiguo». |
| `test_dependency_cycle_preflight` (test_engine.py:277-281) | Dependencia circular `one`↔`two` se rechaza con «circular» antes de descargar (planificación, usa `resolve`). |
| `test_dependency_incompatibility_preflight` (test_engine.py:283-287) | Una dependencia con `installer.file` aborta la instalación antes de descargar. |
| `test_no_component_exemptions_for_hooks` (test_engine.py:384-390) | Con `installer.file`, `compatibility` devuelve exactamente `Pasos no soportados: installer.file` para `main/nodejs`, `main/nodejs-lts`, `extras/vscode` y `other/tool`, incluso con política en `core/policies.json` cuya huella coincide. |
| `test_pre_install_runs_and_post_install_is_ignored_for_every_component` (test_engine.py:392-402) | `post_install` no se ejecuta ni rechaza el componente; `pre_install` sí se ejecuta. (Comportamiento de engine con manifiesto válido.) |
| `test_declared_shortcut_icons_are_relative_supported_files` (test_engine.py:449-453) | Iconos `../outside.ico`, `C:/outside.ico`, `icon.dll,0`, `icon.png` y `{'path': 'icon.ico'}` no devuelven `Compatible`. |
| `test_installer_unsupported_fields_and_malformed_scripts_fail_before_download` (test_engine.py:499-511) | Once instaladores inválidos (`True`, `False`, `'setup.exe'`, `[]`, `{'file':…}`, `script`+`args`, claves extra, `script: null`, `script: 7`, `script` con elemento no textual) hacen fallar la instalación sin descargar. `None`, `{}`, `{'script': ''}` y `{'script': []}` son `Compatible`. |
| `test_extensionless_download_renamed_by_real_pre_install` (test_engine.py:547-561) | URL sin extensión (`…/download?channel=release`) y con fragmento `#/Tool%20File` son `Compatible` y se instalan. |
| `test_pre_install_multiple_download_names_and_extract_to` (test_engine.py:563-574) | Dos URL con hashes en lista y `extract_to` por URL. Valida el manejo de listas de `url`/`hash`/`extract_to` en la instalación (no afirma `Compatible` directamente). |
| `test_download_names_are_shared_and_confined` (test_engine.py:590-601) | `download_filename` rechaza nombres con `/`, `..`, `C:`, `NUL`, `a\b`, `?`, punto final; `compatibility` de esas URL no es `Compatible`; `tool.iso` no es `Compatible`. |
| `test_paths_reject_escape_and_windows_special_names` (test_engine.py:88-91) | `storage.inside` rechaza `../escape`, `C:\outside`, `\outside`, `folder/file:stream`, `aux.txt`, `dir/file.`, `x/../bad` y `dir/a?b` (reglas de `portable_path`). |
| `test_altered_lock_is_rejected_on_activation` (test_engine.py:666-676) | Si el manifiesto del `.mito.json` cambia, activar falla con «incoherente» (comprobación de huella, engine.py:568). |
| `test_resolves_literal_current_and_cross_bucket_references` (test_bucket_resources.py:26-36) | `$bucketsdir/$computed/scripts/x` y `$bucketsdir/missing/scripts/x` fallan en `sources`, y `compatibility` no devuelve `Compatible` para el dinámico. |
| `test_numeric_versions_and_unsupported_update` (test_components.py:39-47) | `version_key` ordena `1.10 > 1.9`, `2.0 > 2.0-rc1`, `1.0.0 == 1`. Una versión nueva con `installer.file` no devuelve `Compatible`. Sin versión nueva, `available_update` devuelve `None`. |
| `test_pending_updates_lists_only_compatible_newer_versions` (test_components.py:131-139) | `pending_updates` sólo incluye las actualizaciones con estado `Compatible`. |
| `test_http_download_redirect_and_hash_verification` (test_transfer.py:16-58, línea 54) | Un manifiesto con `url` HTTP y `hash` de 64 ceros devuelve `Compatible`. |
| `test_optional_and_mixed_hashes` (test_transfer.py:145-160) | `download_hashes` acepta `None`, `skip`, lista con `skip` y `md5:`; `hash_verifier('   ')` devuelve `(None, None)`; lista de longitud incorrecta y `md5:invalid` fallan. `compatibility` es `Compatible` con `hash` `None`, `''`, `'skip'` y `[None, 'md5:…']`. |

## Dependencias
- `storage.py`: `YitoError`, `identifier`, `command_name`, `portable_path`, `read_json`, `write_json`, `mutation`. Las dos primeras son la base de casi todas las reglas de nombres.
- `transfer.py`: `download`, `download_hashes`, `download_filename`, `MAX_EXPANDED`. `download_hashes`, `hash_verifier` y `download_filename` son funciones puras de validación; `download` sólo se usa en importación y refresco (buckets.md).
- `bucket_resources.py`: `required_buckets` (paso 1).
- `persistence.py`: `definitions` (paso 6). Se importa de forma perezosa dentro de `compatibility` para evitar un ciclo, porque `persistence.py` importa `array` de `catalog.py`.
- `i18n.py`: `tr` para todos los mensajes.
- `error_tracking.py`: `capture` (sólo `refresh_keys`).
- Consumidores (no documentados aquí): engine.py, components.py, hooks.py, isolation.py, shortcuts.py, persistence.py, pocketools.py, ui.py, mcp_tools.py, __main__.py.

## Notas para Rust
- Crates: `serde` y `serde_json` para los manifiestos. Recomendación: `serde_json::Value` como representación de los manifiestos, con accesores tipados por campo, porque el original acepta formas variables (texto o lista). Para esos campos, un tipo `OneOrMany<T>` deserializable desde texto o lista. `#[serde(flatten)]` o un `Value` por campo para conservar los campos desconocidos al calcular la huella.
- La huella debe ser idéntica byte a byte a la de Python: claves ordenadas, separadores `,` y `:`, escapado de caracteres no ASCII como `\uXXXX` (con pares sustitutos para caracteres fuera del BMP), y representación numérica de Python. `serde_json` no escapa no ASCII por defecto y su representación numérica puede diferir. Sin esto, los `.mito.json` existentes no coincidirán. Duda: confirmar con fixtures reales de instalaciones previas.
- El orden de claves: `serde_json::Map` ordena por UTF-8 cuando no está activa la función `preserve_order`, lo que coincide con el orden por punto de código de Python. Con `preserve_order` (IndexMap) habría que ordenar a mano antes de serializar.
- Validación de rutas: reproducir `PureWindowsPath` como análisis textual; no usar `std::path::Path` del host (el requisito del test es no tocar el disco). Duda: valorar una crate de rutas Windows (p. ej. `typed-path`) frente a un parser propio.
- Los mensajes de rechazo son texto en español que pasa por `tr()`. Propuesta: un enum `Compatibilidad` con variantes por motivo y un método que devuelva el texto localizado, conservando `Compatible` como variante sin texto. Duda: si la Rust necesita el texto exacto del original (para pruebas de compatibilidad) o sólo el motivo.
- Errores: `thiserror` para `YitoError` equivalente con variantes; el `Manifiesto no soportado:` como prefijo de un variante de envoltorio.
- Captura de excepciones: en Rust no hay excepciones. Las reglas que el original sólo capta por tipo (`KeyError`, `TypeError`, `ValueError`, `YitoError`) deben convertirse en errores explícitos. Duda: ver `AttributeError` sin captura en `bin` y `env_set`.
- Hashes: `download_hashes` y `hash_verifier` deben venir de `mito-transfer`; `catalog` sólo valida sintaxis.
- `download_filename` (transfer.py:23-) depende de la URL (fragmento y ruta). Debe venir de `mito-transfer` para que sea el mismo nombre local que en la descarga.
- Duda: `effective` con `architecture` que no es objeto. El original lanza `TypeError` o `ValueError` según el caso; no está definido cuál.
- Duda: `version` booleano (`true`) se acepta como `"True"` en la compatibilidad, pero en `refresh_keys` pasa también. Decidir si Rust lo rechaza.
- Duda: el solapamiento de `persist` no compara destinos. Decidir si se conserva.
- Duda: la lista de extensiones permitidas es fija en el código. Confirmar que debe seguir siendo fija, o que debe leerse de configuración.
- `resolve` compara nombres sin normalizar: `Main/Tool` no coincide con `main/tool`. Duda: confirmar si es intencionado.
