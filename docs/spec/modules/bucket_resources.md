# bucket_resources.py → mito_catalog::resources

Origen: `core/app/mito/bucket_resources.py` (80 líneas), `core/tests/test_bucket_resources.py` (107 líneas, 6 tests).

Nota: este archivo no existía en el repositorio. Se crea aquí con la parte `sources` (F3, resumen; el contrato completo está en `buckets.md` §7) y la parte `prepare` (F7).

## Responsabilidad

Dar a los hooks (`pre_install`, `installer`) acceso a los scripts de **su propio bucket** y de otros buckets referenciados con `$bucketsdir`, mediante una copia privada por instalación. El origen (`sources`) se resuelve sin descargar nada; la copia (`prepare`) solo se hace después de la aprobación de las descargas sin hash.

## Parte `sources` (F3, resumen)

Funciones: `required_buckets(key, manifest)` (líneas 11-23), `sources(catalog, key, manifest)` (26-38). Detalle en `buckets.md` §7.

- `required_buckets` busca `$bucketsdir` o `${bucketsdir}` (sin distinguir mayúsculas) en `pre_install` e `installer`. Después exige `<nombre>/scripts` o `$bucket/scripts`. Sin esa forma: `Pasos no soportados: referencia dinámica a $bucketsdir`.
- `sources` obtiene el origen de `catalog.sources()` o de `<bucket>.json` (`source`). Mensajes: `No hay origen registrado para los recursos del catálogo: {nombre}` y `La fuente debe ser un repositorio GitHub o una carpeta local`.

## Parte `prepare` (F7)

### API

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `prepare(root, origins, destination, log)` (41-80) | raíz de MITO; `origins` = `{bucket: origen}` (de `sources`); `destination` = `stage/hook-buckets`; `log` | `None`; crea `destination/<bucket>/scripts/...` | `YitoError` (ver «Errores») |

Llamador único: `engine.py:416`, después de `extract`/`copy-payload` y antes de `run_pre_install` (`engine.py:413-419`). Se pasa `hook_sources(self.catalog, key, m)` como `origins`.

Docstring: «Called only after the installation plan's unverified-download approval» (línea 42). Ver «Aprobación previa».

### Algoritmo

1. `destination.mkdir(parents=True, exist_ok=True)`.
2. Por cada `(name, origin)` de `origins`:
   a. `log(f'{name}: preparando recursos del catálogo')` (sin `tr`).
   b. `scratch` = `TemporaryDirectory(dir=destination.parent)`.
   c. Si `origin` empieza por `http://` o `https://`:
      - Debe cumplir `https?://github.com/([\w-]+)/([\w.-]+)` (tras quitar `/` final), si no: `La fuente debe ser un repositorio GitHub o una carpeta local`.
      - `download(f'https://api.github.com/repos/{owner}/{repo}/zipball', scratch/'repository.zip', root=root)`. Sin hash (la aprobación ya se pidió en `installation_plan`).
      - `extract_zip(archive, scratch/'repository')`.
      - La carpeta extraída debe tener **una** entrada y ser directorio (la raíz del snapshot de GitHub). Si no: `El snapshot del catálogo debe tener una sola raíz`.
      - `source` = esa carpeta.
   d. Si no (ruta local):
      - `source = Path(origin)`.
      - Si `source.name.casefold() == 'bucket'` y `source/'scripts'` no es directorio: `source = source.parent`. Acepta la raíz del repositorio o su carpeta `bucket/`.
   e. `scripts = source/'scripts'`. Si no es directorio, es enlace o junction: `No hay recursos scripts válidos en el catálogo: {name}`.
   f. Recorrido `scripts.rglob('*')`, con `count` desde 1:
      - Enlace o junction: `Los enlaces del catálogo no están permitidos`.
      - `target = inside(destination/identifier(name)/'scripts', path.relative_to(scripts).as_posix())`.
      - Si es fichero: se añade `(path, target)` y se suma su tamaño a `total`.
      - Si `count > 150000` o `total > MAX_EXPANDED` (12 GiB): `Recursos del catálogo demasiado grandes`.
   g. Copia: `target.parent.mkdir(parents=True, exist_ok=True)` y `shutil.copyfile(path, target)` por cada fichero.

`identifier(name)` valida y pasa a minúsculas el nombre del bucket (`storage.identifier`, ver `storage.md`).

### Invariantes

1. Solo se copian ficheros dentro de `scripts/`. Los archivos fuera no entran.
2. Los enlaces y junctions del origen se rechazan antes de copiar cualquier cosa de su rama (en el recorrido, antes de `inside`).
3. Las rutas de destino pasan por `inside` dentro de `destination/<bucket>/scripts`. Un `..` en el snapshot remoto ya lo rechaza `extract_zip` (ver `transfer.md`).
4. La copia se hace en `stage/hook-buckets`, así que el origen puede cambiar después sin efecto. Test: se modifica `template.reg` en el origen y la copia conserva `fixture`.
5. Una sola copia por instalación, compartida por `pre_install` e `installer` (misma carpeta `$bucketsdir`).
6. Límite: ≤ 150 000 entradas y ≤ 12 GiB.
7. El origen remoto siempre es un repositorio de GitHub (`/zipball`, sin hash).

### Aprobación previa (`installation_plan`)

En `engine.py:284-292`, durante la planificación (antes de cualquier descarga):

- Para cada instalación nueva, `hook_sources(catalog, key, m)` da los orígenes.
- Cada origen remoto (`http://` o `https://`) añade `f'{key} · recursos del catálogo {name}'` a `plan['unverified']`.
- `install()` exige `confirm_unverified` si hay elementos en `unverified`: si no se pasa o devuelve `False`, `YitoError('Instalación cancelada: descargas sin hash no confirmadas')`. Esto ocurre **antes** de `prepare` y de cualquier descarga.

### Errores y mensajes

| Mensaje | Origen | Tr |
|---|---|---|
| `{name}: preparando recursos del catálogo` | `log` | no |
| `La fuente debe ser un repositorio GitHub o una carpeta local` | `prepare` | no |
| `El snapshot del catálogo debe tener una sola raíz` | `prepare` | no |
| `No hay recursos scripts válidos en el catálogo: {name}` | `prepare` | no |
| `Los enlaces del catálogo no están permitidos` | `prepare` | no |
| `Recursos del catálogo demasiado grandes` | `prepare` | no |
| `Instalación cancelada: descargas sin hash no confirmadas` | `engine.install` | sí (`tr`) |
| `{key} · recursos del catálogo {name}` | `plan['unverified']` | no |

## Casos límite

- Origen local que apunta a `…/bucket` sin carpeta `scripts` propia: se usa su padre.
- Origen local sin `scripts/`: error `No hay recursos scripts válidos`.
- Snapshot de GitHub con dos raíces o con un fichero en la raíz: `El snapshot del catálogo debe tener una sola raíz`.
- Snapshot con `repo/scripts/../../escape`: `extract_zip` lo rechaza antes de copiar nada (test: `good`/`bad`).
- Fallo a mitad del `prepare` de la reinstalación: `destination` es temporal y la instalación anterior no cambia.
- Origen inexistente (`root/missing`) en una reinstalación: `prepare` falla y la instalación anterior se conserva (test `test_resource_failure_preserves_previous_installation`).

## Casos de test del original

| Test | Archivo:línea | Qué comprueba |
|---|---|---|
| `test_resolves_literal_current_and_cross_bucket_references` | `test_bucket_resources.py:26-36` | Parte `sources`: `$bucketsdir/other/...` y `${bucketsdir}/${bucket}/...` resuelven; `$computed` y bucket inexistente fallan; `compatibility` no es `Compatible`. |
| `test_local_resources_are_copied_and_shared_between_hook_phases` | `test_bucket_resources.py:38-43` | `prepare` local copia `scripts/tool/template.reg` a `destino/test/scripts/tool/template.reg`; un cambio posterior en el origen no afecta a la copia. |
| `test_remote_snapshot_and_traversal_rejection` | `test_bucket_resources.py:45-60` | `prepare` remoto (`download` parcheado) extrae `repo/scripts/tool/template.reg`; con `repo/scripts/../../escape`, error y sin `template.reg`. |
| `test_unverified_resources_in_dependency_require_approval_before_download` | `test_bucket_resources.py:62-75` | Dependencia con origen remoto: `plan['unverified'] == ['test/tool · recursos del catálogo test']`; sin confirmación (`None` o `False`) → `sin hash no confirmadas` y ninguna descarga (`download` ni `bucket_resources.download`). |
| `test_resources_prepare_pre_install_and_installer_with_real_powershell` | `test_bucket_resources.py:77-90` | Con PowerShell real: `pre_install` comprueba `template.reg` y `installer.script` copia `$bucketsdir/test/scripts/tool/template.reg` a `second.reg`. Ambos quedan en el payload. |
| `test_resource_failure_preserves_previous_installation` | `test_bucket_resources.py:92-107` | Instalación previa OK; reinstalación con `source` inexistente falla con `YitoError`; `installed()` y `tool.exe` (`original`) no cambian. |

## Dependencias

- `storage`: `YitoError`, `identifier`, `inside`, `read_json`.
- `transfer`: `download`, `extract_zip`, `MAX_EXPANDED`.
- `catalog` (parte `sources`): `Catalog.sources()`, `Catalog.folder`.
- `engine` (llamador de `prepare` y de `sources` en la planificación).
- `i18n.tr`: no se usa en `prepare`. Solo `engine.install` traduce la cancelación.

## Notas para Rust

- `prepare` es síncrono: copia con `std::fs::copy` dentro de un directorio temporal en `stage`.
- Recorrido sin seguir enlaces (p. ej. `walkdir` con `follow_links(false)`). Detección de enlaces y junctions con `symlink_metadata` y `FILE_ATTRIBUTE_REPARSE_POINT` (Windows). Equivale a `is_symlink() or is_junction()`.
- El snapshot de GitHub usa el mismo `download` y `extract_zip` que los paquetes (`transfer.md`). Sin hash en esta ruta.
- Límites: contador de entradas y suma de tamaños en el mismo recorrido, como en el original.

## Dudas

1. Este archivo no existía en el repositorio. Se crea con la parte `sources` resumida. Si se prefiere mantener todo en `buckets.md`, basta con eliminar la primera sección.
2. `prepare` no valida `origin` cuando es local y no existe: `Path(origin)` no se comprueba hasta `scripts.is_dir()`. El mensaje es `No hay recursos scripts válidos…`, no «origen no existe». Confirmar si basta.
3. El snapshot de GitHub se descarga con la URL de la API `api.github.com/…/zipball` sin hash. Un MITM que cambie el ZIP solo se detecta por la aprobación del usuario. Es el comportamiento documentado en `unverified`.
4. `count` cuenta todas las entradas del recorrido (ficheros y directorios). El límite de 150 000 incluye directorios.
5. `prepare` ignora el hash de la descarga del snapshot porque no tiene. No hay comprobación del contenido, igual que con `pre_install`.
6. `log` de `prepare` no está traducido (`'{name}: preparando recursos del catálogo'`). Decidir si entra en i18n.
