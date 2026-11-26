# Buckets (snapshots, refresco, importación, configuración y filtros) → mito-catalog::buckets

Origen: la lógica no está en un único módulo. Funciones leídas:
- `core/app/mito/catalog.py`: `sources` (65-73), `import_bucket` (75-97), `_archive_manifests` (99-133), `all` (135-138), `refresh_installed` (140-142), `refresh_keys` (144-189).
- `core/app/mito/bucket_resources.py` (80 líneas, completo): `required_buckets`, `sources`, `prepare`. Se incluye porque los scripts de hook se leen del snapshot del bucket.
- `core/app/mito/__main__.py`: argumentos 49-54 y ejecución 230-241 (`bucket import` / `bucket refresh`), y comando `search` 242-247 sólo para el uso de `rank_entries`.
- `core/app/mito/mcp_tools.py`: `catalog_refresh` (145-157).
- `core/app/mito/update_checks.py` (31 líneas, completo): `daily` (10-31).
- `core/app/mito/ui.py`: 809-810, 864-866, 900, 970-996, 1183-1193, 1219, 1634, 1720-1723.
- `core/app/mito/configuration.py`: 35 (`PREFIXES`), 87-90 (validación de `bucket.*`).
- `core/app/mito/release.py`: 11-14 (`DEFAULT_CONFIG`) y 60.

Tests: ver la tabla de casos de test.

## Responsabilidad
Mantener un snapshot local por bucket (origen y manifiestos originales en `core/buckets/<bucket>.json`), refrescarlo desde GitHub o desde una carpeta local, declarar las fuentes en `config.properties` y ocultar buckets en la búsqueda de la UI mediante `core/state/catalog-filters.json`. Esta capa no instala componentes ni ejecuta código.

## Snapshot: formato en disco y ubicación

### Ubicación
- Carpeta: `<root>/core/buckets/` (catalog.py:63). Está relacionada con `Catalog.folder`.
- Un archivo por bucket: `core/buckets/<bucket>.json`. El nombre del bucket es el `stem` del archivo (catalog.py:136-138; no se valida al listar).
- Archivos temporales de la importación: directorio temporal creado dentro de `core/buckets/` (catalog.py:78) y archivos `*.tmp` de `write_json` (storage.py:61). El glob `*.json` no los recoge.

### Formato
Escrito por `write_json` (storage.py:59-70): JSON con sangría de 2 espacios, `ensure_ascii=False`, salto de línea final, escritura atómica (archivo temporal, `fsync`, reemplazo).

```json
{
  "schema": 1,
  "source": "https://github.com/ScoopInstaller/Main",
  "apps": {
    "ejemplo": {
      "version": "1.0",
      "url": "https://example.org/ejemplo.zip",
      "hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }
  }
}
```

Ejemplo sintético con la forma que escribe el código. Campos:
- `schema`: se escribe siempre con valor `1` (catalog.py:96, 152, 181). No se valida al leer; ningún código lo compara.
- `source`: texto con el origen tal como se pasó (URL de GitHub o ruta local). Se escribe en la importación (catalog.py:96) y en la creación de un índice nuevo durante el refresco (catalog.py:181). Al leer es opcional (`index.get('source')`, catalog.py:153, bucket_resources.py:31, versions.py:67).
- `apps`: `dict` nombre de app → manifiesto **original** (sin `effective`, sin normalizar). Lo lee `all()` (catalog.py:137).

Los manifiestos se leen con `utf-8-sig` (storage.py:55-56), por lo que un BOM al principio se tolera.

### Qué sustituye cada escritura
- Importación (`import_bucket`): reemplaza el archivo completo (catalog.py:96).
- Refresco por clave (`refresh_keys`): modifica sólo `apps[<app>]`, conservando el resto de entradas y el `source` (catalog.py:181-183).

## Configuración: `config.properties`
- Archivo: `<root>/config.properties`, leído como UTF-8 tolerando BOM (catalog.py:67-69).
- Línea que cuenta: la que **empieza literalmente** por `bucket.` y contiene `=` (catalog.py:70). Se divide en el primer `=` (`split('=', 1)`). El nombre es `identifier(lo que sigue a 'bucket.' sin espacios de los lados)` y el valor se recorta (catalog.py:71-72). Las líneas que empiezan por `#` o `!` no coinciden con `startswith('bucket.')`. Si un nombre aparece dos veces, gana la última línea (es un `dict`).
- Ejemplo literal (valores de `release.py:11-14`, el predeterminado al generar el paquete):

```properties
bucket.main=https://github.com/ScoopInstaller/Main
bucket.extras=https://github.com/ScoopInstaller/Extras
bucket.versions=https://github.com/ScoopInstaller/Versions
bucket.java=https://github.com/ScoopInstaller/Java
```

- Validación al guardar (configuration.py:87-90): `identifier(nombre)` y valor no vacío. No se valida URL ni ruta en ese punto. Duda: un origen mal formado se detecta sólo en el refresco.
- Preservación: `PREFIXES` incluye `bucket.` (configuration.py:35), lo que hace que las claves se conserven al guardar la configuración (test_configuration.py:19-29 y test_bootstrap_proxy.py:81-117).
- `release.py:60` escribe `DEFAULT_CONFIG` como `config.properties` al generar el paquete (contexto de release).

## Operaciones

### 1. `bucket import <nombre> <origen>` (`__main__.py:232-233`) y `import_bucket`
Código: catalog.py:75-97.

1. `name = identifier(name)`: se normaliza a minúsculas, se valida (`[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}`, sin nombres reservados).
2. Crea `core/buckets/` si falta y un directorio temporal dentro.
3. Si el origen empieza por `http://` o `https://`:
   - Coincide `origen.rstrip('/')` con la regex `https?://github.com/([\w-]+)/([\w.-]+)`. Si no coincide: `La fuente debe ser un repositorio GitHub HTTPS o una carpeta local` (catalog.py:81-83).
   - Descarga `https://api.github.com/repos/{owner}/{repo}/zipball` en `stage/bucket.zip` con `download(url, archivo, root=self.root)` (catalog.py:85). No se pasa hash esperado.
   - `manifests = _archive_manifests(zip)` (catalog.py:86).
4. Si no es URL: `source_path = Path(origen)`. Si `source/bucket` es carpeta, se usa; si no, `source`. `manifests = {identifier(stem): read_json(archivo)}` para `bucket.glob('*.json')` en orden (catalog.py:88-92). No es recursivo.
5. Si `manifests` está vacío: `No hay manifiestos en {origen}` (catalog.py:93-94).
6. `write_json(core/buckets/<name>.json, {"schema": 1, "source": origen, "apps": manifests})` (catalog.py:96). Devuelve `len(manifests)`.

Nota: `import_bucket` no escribe en `config.properties`. El comando `bucket import` tampoco (`__main__.py:232-233`).

### 2. `_archive_manifests(archivo)` (lectura del ZIP sin extraer)
Código: catalog.py:99-133, estático. Es la lectura del ZIP descargado: no escribe nada en disco.

1. Abre el ZIP y lista `infolist()`. Si hay más de 150000 entradas o la suma de `file_size` supera `MAX_EXPANDED` (12 GiB, transfer.py:20): `Archivo demasiado grande para extraer` (catalog.py:103-105).
2. Por cada entrada, en orden:
   - `path = portable_path(entry.filename)` (storage.py:24-32). Un error se propaga.
   - `key = str(path).casefold()`. Si ya está visto: `Entrada duplicada: {nombre}` (catalog.py:108-111). Esto también rechaza `tool.json` frente a `TOOL.json`.
   - Si el modo UNIX de la entrada (`external_attr >> 16`) es enlace simbólico (`stat.S_ISLNK`): `Los enlaces del archivo no están permitidos` (catalog.py:113-114).
   - Si `path.parts` está vacío: `Ruta no portable: {nombre}`.
   - `roots.add(path.parts[0])`.
3. Si hay un número de raíces distinto de 1: `Ruta fuera de su raíz: {archivo}` (catalog.py:119-120). Un ZIP de GitHub tiene una única raíz (`owner-repo-sha/`).
4. `nested = any(len(p.parts) >= 2 and p.parts[1].casefold() == 'bucket' and (len(p.parts) > 2 or e.is_dir()))`. Es decir: hay una carpeta `bucket/` dentro de la raíz (catalog.py:121-122).
5. Para cada archivo en orden de ruta:
   - Se ignoran directorios y archivos que no terminan en `.json` (sin distinguir mayúsculas) (catalog.py:125-126).
   - Con `nested`, la ruta debe tener exactamente 3 partes (`raíz/bucket/app.json`). Sin `nested`, exactamente 2 (`raíz/app.json`) (catalog.py:127-130).
   - `name = identifier(path.stem)`. Si no es válido, error.
   - `manifests[name] = json.loads(source.read(entry).decode('utf-8-sig'))`. Un JSON inválido produce `JSONDecodeError`, que **no** es `YitoError` (Duda: decidir cómo se presenta).
6. Devuelve `manifests`.

Layouts aceptados (test_catalog_performance.py:31-39): `repo/tool.json` (plano) y `repo/bucket/tool.json` (con carpeta `bucket/`). En ambos casos `repo/docs/ignored.json` se ignora.

### 3. `bucket refresh [nombre]` (`__main__.py:235-241`) y MCP `catalog_refresh` (mcp_tools.py:145-157)
- CLI: si se pasa `nombre` y no está en `sources()`: `Bucket no configurado` (`__main__.py:237-238`). Si no, se hace `import_bucket` de **todas** las fuentes configuradas (o de la indicada), bajo `mutation(root)` (`__main__.py:231`).
- MCP: igual. Con `bucket` desconocido: `INVALID_ARGUMENT: unknown bucket` (mcp_tools.py:151). Ejecución como job (`job.check()` entre fuentes).
- Consecuencia: un bucket que está en el snapshot pero no en `config.properties` no se refresca con estos comandos. Duda: el original no lo documenta; se trata como comportamiento observado.

### 4. Refresco por clave: `refresh_installed` / `refresh_keys` (catalog.py:140-189)
Es el refresco ligero que usa la UI. No descarga ZIP ni sincroniza buckets completos.

1. `refresh_installed(inventario)` toma `record['key']` de cada entrada y llama a `refresh_keys`.
2. `refresh_keys(claves)`: itera `sorted(set(claves))`. Cada clave:
   a. `bucket, app = map(identifier, clave.split('/'))`. Si no hay exactamente una barra, `ValueError` capturado en 5.
   b. `index = read_json(core/buckets/<bucket>.json)` si existe, o `{"schema": 1, "apps": {}}` (catalog.py:150-152).
   c. `source = sources[bucket]` de la configuración; si no hay, `index['source']`. Si no hay ninguno: `No hay origen registrado para el catálogo` (catalog.py:153-155).
   d. **Origen GitHub** (regex del paso 1, sin distinguir `http`/`https`): si no coincide, `La fuente debe ser un repositorio GitHub o una carpeta local` (catalog.py:157-159). Descarga `https://raw.githubusercontent.com/{owner}/{repo}/HEAD/bucket/{app}.json`. Si responde HTTP 404, descarga `https://raw.githubusercontent.com/{owner}/{repo}/HEAD/{app}.json`. Otros códigos HTTP se propagan (catalog.py:160-169).
   e. **Origen local**: `carpeta = origen/bucket` si existe como carpeta, o `origen`; `manifest = read_json(carpeta/{app}.json)` (catalog.py:172-174).
   f. Validación: si no es `dict` o si `version` no es texto, entero o flotante: `Manifiesto sin versión válida`. Después `identifier(str(version))` (catalog.py:175-177).
   g. Bajo `mutation(root)` (sin espera, `wait=0`): releer el índice, `index['apps'][app] = manifest` y `write_json` (catalog.py:180-183). Si el índice no existía, se crea con `source` (catalog.py:181).
   h. `refreshed += 1`.
3. Si cualquier paso falla (excepción): `capture(root, exc, operation='catalog.refresh-installed', component=clave)` (error_tracking) y el mensaje `No se pudo comprobar {clave}: {error}. Se conserva el catálogo anterior.` (catalog.py:185-188). Continúa con la siguiente clave.
4. Devuelve `refreshed`.

Callers:
- `ui.py:980`: comprobación de componentes instalados (dentro de `daily`, ver 5).
- `ui.py:1634`: `install_from_catalog` llama `refresh_keys([target], log)`. Si el resultado es distinto de 1, la instalación se detiene (`return`), y el catálogo anterior se conserva (ARCHITECTURE, sección Contratos).

### 5. Consulta diaria de componentes instalados (`update_checks.daily`)
Código: update_checks.py:10-31. Invocado desde `ui.py:977-986` con nombre `'component-updates'` y clave `{'inventory': inventario, 'sources': catalog.sources()}`.

Registro en disco: `core/state/component-updates-check.json` con la forma:

```json
{
  "day": "2026-09-13",
  "key": "<sha256 hex de json.dumps(clave, sort_keys=True)>",
  "token": "<uuid4 hex>",
  "result": 2
}
```

(`result` es `null` mientras la operación no ha terminado o si falló.)

Algoritmo:
1. `day = date.today().isoformat()` (fecha local, update_checks.py:11).
2. `identity = sha256(json.dumps(clave, sort_keys=True))` (update_checks.py:12). Obsérvese que este `json.dumps` usa separadores por defecto (`, ` y `: `), a diferencia de la huella de manifiestos.
3. Dentro de `mutation(root, 'update-checks.lock', wait=15)` (update_checks.py:17): lee el registro (si falla, `{}`). Si no es forzado y `day` coincide y `key` coincide, devuelve `result` guardado sin red (update_checks.py:22-23).
4. Antes de la red, escribe `{day, key, token, result: null}` (update_checks.py:25-26). Así, errores o cierres no se reintentan hasta el día siguiente.
5. Ejecuta la operación fuera del lock (update_checks.py:27).
6. Bajo el lock de nuevo: si el `token` guardado sigue siendo el suyo, escribe `result` (update_checks.py:28-30). Si otra consulta reemplazó el registro, no escribe.
7. Devuelve el resultado.

Activadores:
- Al abrir la UI: `ui.py:900` (`on_mount` → `check_component_updates` → `refresh_component_updates()` sin `force`).
- `Buscar actualizaciones` (acción `check-updates`): `ui.py:1720-1722` → `refresh_component_updates(force=True)`, que se salta la caché (test_update_checks.py:115-122).

Invalidación: la clave incluye el inventario completo y las fuentes. Cambiar cualquiera de los dos hace que la siguiente llamada del mismo día vuelva a consultar (test_update_checks.py:135-148 comprueba el cambio de clave `'source'` a `'another source'`).

### 6. Filtros de búsqueda por bucket (`catalog-filters.json`)
- Ruta: `<root>/core/state/catalog-filters.json` (ui.py:809).
- Formato: lista JSON ordenada de nombres de buckets **deshabilitados**. Ejemplo (test_ui.py:815): `["extras"]`.
- Lectura al abrir la UI: `set(read_json(ruta))` si existe; si no, conjunto vacío (ui.py:810). Un archivo que no sea lista produce resultados no definidos (Duda: el original no lo valida; ver casos límite).
- Casillas: `Checkbox` por nombre en `sorted(set(claves de sources()) | {stems de core/buckets/*.json})` (ui.py:864-866). Valor: el nombre no está deshabilitado. Id: `bucket-<nombre>`.
- Al cambiar una casilla (ui.py:1183-1193): si queda marcada, se quita del conjunto; si no, se añade. Escribe `sorted(conjunto)` con `write_json` (ui.py:1190), sin `mutation`. Después refiltra la lista.
- Efecto: `rank_entries(entries, término, conjunto)` (ui.py:1219). Excluye las filas cuyo bucket está en el conjunto. El comando de CLI `search` no usa este archivo (`__main__.py:244-245`).

### 7. Recursos de scripts por bucket (`bucket_resources.py`)
Estos recursos son la carpeta `scripts/` del bucket, que los hooks de instalación pueden referenciar con `$bucketsdir`.

- `required_buckets(clave, manifiesto)` (bucket_resources.py:11-23): busca `$bucketsdir` o `${bucketsdir}` en `pre_install` e `installer`. Después comprueba el patrón `<nombre>/scripts` o `$bucket/scripts`. Si no lo cumple, `Pasos no soportados: referencia dinámica a $bucketsdir`. Devuelve nombres ordenados.
- `sources(catalog, clave, manifiesto)` (bucket_resources.py:26-38): para cada bucket requerido, origen de la configuración o del snapshot. Si falta: `No hay origen registrado para los recursos del catálogo: {nombre}`. Si la URL no es GitHub: `La fuente debe ser un repositorio GitHub o una carpeta local`.
- `prepare(root, orígenes, destino, log)` (bucket_resources.py:41-80): solo después de la aprobación de descargas sin hash (docstring, bucket_resources.py:42; engine.py:414-416).
  - GitHub: descarga `api.github.com/.../zipball` en `scratch/repository.zip`, lo extrae (`extract_zip` de transfer.py:380) y exige una sola raíz (`El snapshot del catálogo debe tener una sola raíz`).
  - Local: si la ruta se llama `bucket` y no tiene `scripts/`, usa el padre.
  - Exige `scripts/` como carpeta no enlace (`No hay recursos scripts válidos en el catálogo: {nombre}`). Rechaza enlaces y junctions (`Los enlaces del catálogo no están permitidos`). El destino de cada archivo se valida con `inside` (storage.py:35-40). Límite: 150000 entradas o `MAX_EXPANDED` (`Recursos del catálogo demasiado grandes`).
  - Copia a `destino/<bucket>/scripts/...` con `shutil.copyfile`.

## Invariantes
1. Un snapshot guarda el origen y los manifiestos originales; la escritura es atómica (catalog.py:96; storage.py:59-70).
2. Refrescar no modifica locks de profiles, payloads ni ejecuta código. El refresco sólo escribe `core/buckets/<bucket>.json` (catalog.py:180-183; test_components.py:49-66 comprueba que los payloads y los perfiles no cambian).
3. El refresco por clave sólo cambia `apps[<app>]` del bucket y conserva las demás entradas (catalog.py:181-183; test_components.py:85-106).
4. Un error de refresco conserva el snapshot anterior y lo registra en el log (catalog.py:185-188; test_components.py:68-84).
5. Un ZIP inválido no cambia el snapshot previo (test_catalog_performance.py:41-59).
6. La lectura del ZIP no escribe nada fuera del temporal y no extrae el repositorio (catalog.py:99-133; test_catalog_performance.py:31-39).
7. La consulta diaria se ejecuta como máximo una vez por día local y por clave, salvo `force` (update_checks.py:10-31).
8. Instalar desde el catálogo refresca sólo la clave elegida. Si el refresco no tiene éxito, no se instala (ui.py:1634).
9. Un `bucket.<nombre>` de la configuración es la fuente preferida. Sin él, se usa `source` del snapshot (catalog.py:153; bucket_resources.py:31; versions.py:67).
10. La lógica de buckets nunca instala componentes automáticamente (ARCHITECTURE, sección Contratos).

## Casos límite y errores
- Origen con `https://github.com/owner/repo/` (barra final): `rstrip('/')` la quita antes de la regex (catalog.py:81, 157).
- Origen GitHub con `http://` (no `https`): la regex acepta `https?`, pero el mensaje de error de la importación dice `HTTPS` (catalog.py:81-83). Duda: el mensaje y la regex no coinciden en la importación.
- En `refresh_keys` el mensaje de origen no GitHub dice `repositorio GitHub o una carpeta local` y no `HTTPS` (catalog.py:159). Son mensajes distintos para el mismo caso.
- Clave de app con mayúsculas: `identifier` la normaliza, y la ruta raw de GitHub usa el nombre en minúsculas (catalog.py:150, 163). Duda: GitHub distingue mayúsculas en rutas raw; la importación por ZIP compara en minúsculas (catalog.py:109, 121, 129).
- `manifest.json` inválido en un bucket local: `read_json` lanza error no `YitoError`. En `refresh_keys` se captura y registra; en `import_bucket` se propaga (catalog.py:92, 132).
- Un `schema` distinto de `1` no se detecta.
- Snapshot sin `apps`: `all()` lanza `KeyError` (catalog.py:137). Duda: no hay tratamiento.
- Refresco con el bloqueo ocupado: `mutation(root)` con `wait=0` falla de inmediato (storage.py:74-101). La descarga ya se hizo y se pierde; se registra como fallo del catálogo. Duda: el original no reintenta.
- `bucket import` no añade la fuente a `config.properties` (`__main__.py:232-233`). Por eso un bucket importado y no configurado no entra en `bucket refresh` (sección 3), aunque sí en `refresh_keys` porque usa `index['source']`. Duda: ¿es intencionado?
- ARCHITECTURE indica «Importar una fuente local los copia; nunca deja un enlace necesario a C:\scoop». El original copia los manifiestos al snapshot, pero conserva la ruta local en `source` y la usa para refrescar. Duda: el contrato y el código no coinciden del todo; decidir si el refresco de carpeta local es correcto.
- Archivo `catalog-filters.json` corrupto o no lista: `set(read_json(...))` lanza error al abrir la UI (ui.py:810), sin captura. Duda: el original no define comportamiento.
- Escritura de `catalog-filters.json` sin lock: dos instancias pueden escribir a la vez (ui.py:1190). Duda: el original no usa `mutation` aquí.
- Nombre de bucket con mayúsculas en `config.properties`: `identifier` lo normaliza a minúsculas en `sources()` (catalog.py:72) y en la validación de configuración (configuration.py:88). Un `bucket.Main=…` pasa a `main`.
- Línea `bucket.` sin `=`: se ignora (catalog.py:70).
- Consulta diaria con error de red: el registro queda con `result: null` y no se reintenta hasta el día siguiente (update_checks.py:25-26; test_update_checks.py:150-160).

## Casos de test del original
| Test (archivo:línea) | Qué comprueba |
|---|---|
| `test_archive_layouts_and_no_extraction` (test_catalog_performance.py:31-39) | `_archive_manifests` lee `repo/tool.json` y `repo/bucket/tool.json` con BOM, ignora `repo/docs/ignored.json` y no deja archivos en el temporal. |
| `test_bad_archives_preserve_previous_snapshot` (test_catalog_performance.py:41-59) | Con un ZIP que contiene `repo/../escape`, `repo/CON`, `repo/bucket/TOOL.json` (duplicado) o `repo/link` simbólico, `import_bucket` lanza `YitoError` y el snapshot anterior queda idéntico byte a byte. |
| `test_resolves_literal_current_and_cross_bucket_references` (test_bucket_resources.py:26-36) | `sources` devuelve el origen del bucket actual y de otro bucket referenciado con `$bucketsdir/other/...`; referencias dinámicas o a bucket inexistente fallan. |
| `test_local_resources_are_copied_and_shared_between_hook_phases` (test_bucket_resources.py:38-43) | `prepare` copia `scripts/` local a `destino/<bucket>/scripts/`; un cambio posterior del origen no afecta a la copia. |
| `test_remote_snapshot_and_traversal_rejection` (test_bucket_resources.py:45-60) | `prepare` remoto de ZIP correcto copia el script; un ZIP con `scripts/../../escape` falla y no deja archivo. |
| `test_unverified_resources_in_dependency_require_approval_before_download` (test_bucket_resources.py:62-75) | Un recurso de bucket de una dependencia sin hash exige aprobación antes de cualquier descarga; si no se confirma, no se descarga nada (`plan['unverified']`). |
| `test_resources_prepare_pre_install_and_installer_with_real_powershell` (test_bucket_resources.py:77-90) | Un hook referencia `$bucketsdir/test/scripts/tool/template.reg`, que existe en el snapshot; pre_install y installer lo usan (requiere PowerShell real). |
| `test_resource_failure_preserves_previous_installation` (test_bucket_resources.py:92-107) | Si la fuente de recursos falta en una reinstalación, la instalación anterior se conserva (inventario y `tool.exe` iguales). |
| `test_refresh_only_installed_keys_once_without_changing_payloads_or_profiles` (test_components.py:49-66) | `refresh_installed` descarga una sola vez la clave instalada (`raw.githubusercontent.com/.../bucket/tool.json`), no toca payloads ni perfiles, y `available_update` ve la versión nueva. `uninstalled` no se descarga. |
| `test_refresh_local_partial_failure_preserves_previous_manifest` (test_components.py:68-84) | Con carpeta local: una clave correcta se refresca (cuenta 1), una JSON inválida y una clave que no existe fallan y se registran (2 mensajes), el manifiesto anterior se conserva. Con red caída, el refresco devuelve 0 y el manifiesto ya refrescado antes (versión 2.0) sigue vigente. |
| `test_refresh_selected_uninstalled_key_preserves_other_entries_and_inventory` (test_components.py:85-95 aprox.) | `refresh_keys(['main/new'])` actualiza solo esa clave, sin tocar `main/tool`, ni el inventario instalado. |
| `test_manual_button_refreshes_components_and_mito_again` (test_update_checks.py:19-44) | Al abrir la UI se consulta una vez; el botón `#check-updates` la repite (`refresh_installed` llamado dos veces). |
| `test_catalog_new_version_requires_confirmation_and_cancel_does_not_install` (test_update_checks.py:46-90) | La instalación desde el catálogo refresca la clave (`refresh_keys`) y pide confirmación ante una versión mayor; si `refresh_keys` devuelve 0 no se instala. |
| `test_reopening_manager_skips_component_refresh_and_progress_logs` (test_update_checks.py:92-111) | Reabrir la UI en el mismo día no repite el refresco (`refresh_installed` una vez) ni muestra el mensaje de progreso. |
| `test_manual_check_bypasses_and_replaces_daily_cache` (test_update_checks.py:115-122) | `daily(..., force=True)` ejecuta la operación y reemplaza la caché del día. |
| `test_success_cached_across_calls_and_next_day_refreshes` (test_update_checks.py:135-148) | Éxito cacheado el mismo día; al cambiar el día, o la clave (`'another source'`), la operación se vuelve a ejecutar. |
| `test_failure_and_in_progress_attempt_are_not_repeated` (test_update_checks.py:150-160) | Un fallo (`OSError`) propaga la excepción en la primera llamada; la siguiente del mismo día devuelve `None` sin repetir la operación (`assert_called_once`). Una llamada de `daily` hecha desde dentro de una operación en curso (mismo nombre y clave, resultado todavía `null`) devuelve `None` sin ejecutar su operación; la operación externa termina y devuelve 42. |
| `test_catalog_filter_and_selection` (test_ui.py:620-655 aprox.) | La lista de buckets de `core/buckets/main.json` se muestra; la búsqueda filtra por texto; la selección es `main/hello`. |
| `test_bucket_filter_and_scrollable_notes` (test_ui.py:798-815) | Dos buckets con la misma app muestran 2 filas; deshabilitar `extras` deja 1 fila (`main/python`) y escribe `["extras"]` en `catalog-filters.json`. |
| `test_application_buttons_only_visible_for_active_launchable_app` (test_ui.py:704-725) | Parchea `refresh_installed` para evitar red al arrancar. No prueba el refresco en sí. |
| `test_local_bucket_has_no_history` (test_versions.py:119-121) | `versions.history` no devuelve historial para un snapshot cuyo `source` es local (usa el origen del snapshot). |
| `test_global_save_preserves_comments_unknown_properties_and_equals` (test_configuration.py:19-29) | Al guardar la configuración global se conservan `bucket.main=C:/catalog`, comentarios y claves desconocidas. |
| `test_system_proxy_no_proxy_form_and_properties_helpers` (test_bootstrap_proxy.py:81-117, líneas 83 y 117) | Al escribir `http_proxy` en `config.properties`, la línea `bucket.main=x` se conserva en su posición. |

## Dependencias
- `storage.py`: `identifier`, `portable_path`, `inside`, `read_json`, `write_json`, `mutation`, `YitoError`.
- `transfer.py`: `download` (con `root=`, para la configuración de red), `extract_zip`, `MAX_EXPANDED`, `MAX_DOWNLOAD` (4 GiB, transfer.py:19).
- `catalog.py`: `Catalog` y `compatibility` (catalog.md).
- `update_checks.py`: `daily` (usado para componentes instalados).
- `error_tracking.py`: `capture`.
- `configuration.py`: `PREFIXES` y validación de claves `bucket.*`.
- `versions.py`: lee `source` del snapshot para el histórico de versiones (versions.py:64-67). Fuera de este contrato.
- `engine.py`: `hook_sources` y `prepare` de bucket_resources en la planificación y la instalación (engine.py:288-289, 414-416).
- `ui.py` y `__main__.py`: llamadores (sección de operaciones).

## Notas para Rust
- Crates: `serde`/`serde_json` para `core/buckets/*.json`, `config.properties` (parser propio: sólo líneas `clave=valor` sin tratamiento de escapes, así que no hace falta crate de properties), `zip` para leer el ZIP sin extraer (`ZipArchive::by_index` y `unix_mode()` para el enlace simbólico), `regex` para las URLs de GitHub. Duda: la regex del original usa `\w` de Python (Unicode); `regex` de Rust con `\w` también es Unicode, pero la equivalencia de clases debe comprobarse.
- Descargas (ZIP de zipball, JSON raw, recursos de scripts): todas por la API de `mito-transfer`. Para el ZIP de la importación no hay hash esperado en el original; la Rust debe pasar `None`. Duda: confirmar si se quiere una verificación adicional.
- Bloqueos: `mutation(root)` equivale a un bloqueo de archivo liberado por el sistema operativo, con espera opcional (`wait`). En Windows el original usa `msvcrt.locking` sobre un byte. Crates posibles: `fd-lock` o `fs4`, con `windows-sys`. Duda: confirmar que el comportamiento de espera (`wait=0` falla de inmediato; `wait=15` reintenta cada 0,1 s) se puede reproducir con esas crates.
- Escritura atómica: `tempfile` (crate `tempfile`) con `persist`, y `sync_all` antes del renombrado.
- Fecha local para la consulta diaria: `chrono` (`Local::now().date_naive()`) o `time`. Duda: confirmar la zona horaria de la comparación.
- La clave de la consulta diaria se serializa con `json.dumps(sort_keys=True)` (separadores por defecto de Python). Si el estado debe ser compatible entre versiones, habrá que reproducir ese formato. Duda.
- `serde_json::Map` ordena las claves (sin `preserve_order`). El orden de `apps` en el original es el de inserción (orden del ZIP o del glob). Duda: si importa el orden en la salida de la UI.
- Los nombres de archivo del snapshot son `stem` sin validar en `all()`. En Rust conviene validarlos al escribir, pero al leer hay que aceptar lo que haya.
- Errores: `thiserror`. Equivalente a `YitoError` con variantes, y el resto (`JSONDecodeError`, `HTTPError`) como variantes propias. Duda: decidir cómo se muestran los JSON inválidos en la importación.
- Duda: la regla de `refresh_keys` (captura de cualquier excepción y continuar) debe mantenerse exactamente.
- Duda: si `bucket import` debe añadir `bucket.<nombre>` a `config.properties` (sección «Notas: bucket import»).
- Duda: si el refresco de carpetas locales debe seguir leyendo la carpeta (y no el snapshot), dado que ARCHITECTURE dice «copia».
- Duda: si la Rust debe validar `schema` y `apps`.
