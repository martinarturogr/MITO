# isolation.py → mito-engine::isolation

Origen: `core/app/mito/isolation.py` (61 líneas), `core/tests/test_isolation.py` (6 tests).

## Responsabilidad

Decide si un componente está **aislado por profile** a partir solo de dos campos declarativos del manifiesto (`persist` y `shortcuts`). Si lo está, calcula la ruta de su copia por profile, la ruta de sus datos persistentes por profile y crea la copia independiente (`prepare_payload`). Si no, todos los profiles comparten el payload único de `core/apps` y la persistencia compartida de `home/persist`.

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `isolated(lock)` (`isolation.py:12-14`) | lock (`{'manifest', …}`) | `bool` | — |
| `persist_root(root, lock, profile)` (`:17-19`) | raíz, lock, nombre de profile | ruta | `YitoError` si `profile` no es un identificador válido (`identifier`). |
| `payload_path(root, lock, profile)` (`:22-26`) | raíz, lock, profile | ruta | `YitoError` si `lock['path']` escapa de la raíz (`inside`). |
| `prepare_payload(root, lock, profile)` (`:29-61`) | raíz, lock, profile | ruta de la copia (o del payload compartido) | `YitoError` con «Enlace no admitido en la copia del componente: {p1}» (`:47`). `OSError` de copia. |

Todas las funciones usan `effective(lock['manifest'])` (de `catalog`) para resolver el manifiesto efectivo antes de leer `persist` y `shortcuts` (`:13`, `:33`). Duda: `isolated` usa `lock['manifest']` pero `payload_path` no lo necesita salvo a través de `isolated`.

### Constantes y textos literales

| Literal | Línea | Uso |
|---|---|---|
| `core/apps` | `:26` | Prefijo que se quita de la ruta del componente para obtener `<bucket>/<app>`. |
| `core/profile-apps` | `:26`, `:31` | Raíz de las copias por profile. |
| `home/profiles` | `:18` | Raíz de la persistencia por profile. |
| `home/persist` | `:18` | Persistencia compartida (no aislada). |
| `persist` | `:14` | Clave del manifiesto que activa el aislamiento junto con `shortcuts`. |
| `shortcuts` | `:14` | Idem. |
| `.mito-persist.json` | `:38` | Fichero excluido de la copia, solo en la raíz del payload. |
| `.mito-defaults` | `:53` | Carpeta dentro de la copia con el contenido inicial de cada definición de persist. |
| `core/staging` | `:48` | Padre del directorio temporal de la copia. |
| `Enlace no admitido en la copia del componente: {p1}` | `:47` | Enlace simbólico o junction en el árbol del componente fuera de las rutas excluidas. |

## Formatos de disco

**Copia por profile** (solo aislados): `core/profile-apps/<profile>/<bucket>/<app>`. Ejemplo con el lock `main/editor` y profile `one`: `core/profile-apps/one/main/editor/`. Contiene `editor.exe`, `.mito.json` (copiado del template) y `.mito-defaults/data/default.txt` (contenido inicial). En `data/` hay un junction a la persistencia del profile.

**Persistencia por profile** (solo aislados): `home/profiles/<profile>/persist/<key>/`. Ejemplo: `home/profiles/one/persist/main/editor/data/default.txt`.

**Persistencia compartida** (no aislados): `home/persist/<key>/`. Ejemplo: `home/persist/main/tool/data/keep.txt`.

**Payload compartido** (no aislados): `core/apps/<bucket>/<app>/`, el mismo que `lock['path']`.

**`.mito-defaults`**: dentro de la copia, `inside(copy/'.mito-defaults', relative)` por cada definición de `persist` cuyo origen existe y no es enlace. Si es carpeta se copia entera; si es fichero, `copy2`. Solo se crea si no existe ya (`:53-57`).

Duda: el contenido de `.mito-defaults` no lo consume este módulo; lo consume `persistence.prepare` al enlazar. El contrato de `persistence` no está en este documento.

## Invariantes

1. **Regla de aislamiento**. `isolated(lock)` es verdadero si y solo si `effective(manifest).persist` y `effective(manifest).shortcuts` son ambos truthy (`:14`). Quitar cualquiera de los dos lo desactiva (test `test_rule_and_independent…`).
2. **Ruta del payload**. Sin aislamiento, `payload_path` devuelve `inside(root, lock['path'])`. Con aislamiento devuelve `inside(root/core/profile-apps/<profile>, <ruta relativa a core/apps>)` (`:22-26`).
3. **Persistencia**. Sin aislamiento: `home/persist/<key>`. Con aislamiento: `home/profiles/<profile>/persist/<key>` (`:17-19`).
4. **Nombre de profile validado**. Tanto `persist_root` como `payload_path` pasan `profile` por `identifier` (`:18`, `:26`). Un nombre inválido lanza `YitoError` antes de construir la ruta.
5. **Copia independiente**. `prepare_payload` crea la copia solo si `isolated` y la ruta destino **no existe** (`:34`). Una vez creada, no se vuelve a copiar; los cambios de la plantilla llegan a las copias solo mediante reemplazo en `publish_plan` (ver `components.md`).
6. **Las copias no siguen enlaces heredados**. Antes de copiar, `os.walk(base, followlinks=False)` recorre el árbol y lanza error si encuentra un junction o enlace simbólico fuera de las rutas excluidas (`:40-47`).
7. **Exclusiones**. La copia omite (a) las rutas de `persist` (`excluded`, `:35`) y (b) `.mito-persist.json` en la raíz del componente (`:38`). Las rutas excluidas se comprueban antes que los enlaces: un junction de persist es aceptable (`:43-45`).
8. **Defaults**. Antes de borrar la persistencia del template, se guarda su contenido inicial en `.mito-defaults` (`:51-57`), que es lo que `prepare` usa para sembrar cada profile nuevo.
9. **Enlace tras copiar**. Después de crear la copia (o si ya existe), siempre se llama a `persistence.prepare(root, target, key, manifest, persist_root(...))` (`:60`). Esto reconstruye los junctions (incluido tras reubicar la raíz) y no toca datos existentes.
10. **Sin aislamiento, `prepare` sigue llamándose**. Para un componente no aislado, `prepare_payload` no copia nada, pero sí enlaza la persistencia compartida dentro del payload (`:60`).
11. **Las copias existentes no se reescriben**. Si `target` existe, `prepare_payload` no vuelve a copiar, ni siquiera si la plantilla cambió. La actualización reemplaza copias en `publish_plan` moviéndolas a staging.
12. **Copia atómica**. La copia se construye en un directorio temporal bajo `core/staging` y se publica con `rename` (`:48-59`). Si falla antes del `rename`, no queda una copia a medias en `core/profile-apps`.

## Casos límite y errores

- `isolated` con `persist: ''` o `shortcuts: []` es falso (truthiness de Python). En Rust, la regla es la misma solo si se interpreta `''` y `[]` como ausencia. Duda: el manifiesto real puede traer `persist` como lista vacía; confirmar la semántica con `persistence.definitions`.
- `persist` puede ser una cadena o una lista (`test_persist_file_directory_and_relocation` usa lista `['conf','user.ini']`). Este módulo solo comprueba la truthiness; la forma de `definitions` está en `persistence`.
- `payload_path` con `profile=None` y aislado: `identifier(None)` falla. Duda: ¿debe llamarse alguna vez sin profile en un componente aislado? `component_environment` sí puede (`engine.py:463`), aunque el test `test_component_environment` solo lo hace para no aislados.
- Ruta de lock fuera de `core/apps` con aislamiento: `base.relative_to(root/'core/apps')` lanza `ValueError` antes de llegar a `inside`. Duda: en el original es un `ValueError` de Python, no un `YitoError`. ¿Debe normalizarse?
- Copia con un junction previo en el payload fuera de `persist`: se rechaza con `YitoError` antes de copiar (`:46-47`). Es el caso de `test_existing_shared_junction_is_not_copied_to_profiles`.
- Nombres de definiciones con `\` (Windows): `relative.replace('\\','/')` (`:35`) antes de `Path(...).parts`.
- `.mito-persist.json` se excluye solo en la raíz (`folder == base`, `:38`).
- Errores de sistema de ficheros (`copytree`, `rename`, `mkdir`): se propagan tal cual. No hay rollback parcial aquí; el directorio temporal se borra con su contexto (`with tempfile.TemporaryDirectory`).
- `prepare` puede fallar tras haber creado la copia (p. ej., al crear un junction). La copia queda publicada y sin enlaces. Duda: el original no limpia esa copia; `publish_plan` la trata como válida la siguiente vez.

## Casos de test del original

| Test | Qué comprueba | Fase posterior |
|---|---|---|
| `test_rule_and_independent_data_binaries_environment_and_launch` | (1) `isolated` es verdadero con `persist`+`shortcuts` y falso al quitar cualquiera. (2) `environment('one')` y `environment('two')` dan `APP_ROOT` y `APP_DATA` distintos, pero `HOME` igual. (3) `APP_ROOT` de `one` es la ruta de `payload_path`. (4) Escribir en `one/editor.exe` o `one/data` no afecta a `two` ni al template. (5) `launch('one', id)` ejecuta `Popen([payload/editor.exe])` con el entorno de `environment`. (6) `set_active` off/on y otro `environment` no reescriben los datos de `one`. | F5 (`set_active` → `terminal.prepare`), F10 (`environment` → pocketools). |
| `test_remove_cleans_all_copies_preserves_profile_persist` | `remove` elimina todas las copias de `one` y `two`, y conserva `home/profiles/<p>/persist/main/editor/data/default.txt`. | F5 (`refresh_launchers`), F10 (`environment` en la preparación). |
| `test_update_replaces_active_and_inactive_copies_preserving_data` | `update` a la versión 2 reemplaza el payload y **todas** las copias, activas e inactivas (`two` no tiene el id pero su copia se actualiza), preserva la persistencia de cada profile (`one`, `two`), y recrea el junction `data`. El id no cambia. | F5, F7 (install), F8 (`migrate_application_links`), F10. |
| `test_existing_shared_junction_is_not_copied_to_profiles` | Si la plantilla tiene un junction `data` hacia la persistencia compartida, la copia de `one` **no** hereda ese enlace ni su contenido (`private.txt` no existe en la copia). La persistencia compartida se conserva. | F10 (`environment`). |
| `test_intact_persist_links_are_not_rebuilt_on_each_launch` | Dos llamadas consecutivas a `persistence.prepare` sobre un payload ya enlazado no ejecutan ningún proceso externo (`subprocess.run`), y el junction sigue en su sitio. | Ninguna (el módulo `persistence` es el que se prueba, no se asigna fase). |
| `test_relocation_rebuilds_profile_junction` | Mover toda la instalación (`root` → `root-moved`) y ejecutar `environment('one')` con un `Engine` nuevo recrea el junction `data` de la copia para que apunte a `moved/home/profiles/one/persist/…`. | F11 (traslado), F10 (`environment`). |

Notas por test:
- El `setUp` crea `main/editor` con `persist='data'` y `shortcuts=[['editor.exe','Editor']]`, `bin='editor.exe'`, y `env_add_path='.'`. El `.mito.json` del template tiene `record = lock`. Los profiles `one` y `two` apuntan a `apps: ['main/editor']`.
- En `test_rule…` el `lock` se usa para `isolated` y `persist_root` directamente, y el entorno se obtiene por `Engine.environment`.
- `test_relocation…` usa `rename_retrying` (10 intentos, 0,5 s) porque el antivirus bloquea el renombrado de carpetas recién creadas (`WinError 5`). Este comportamiento es del entorno Windows, no de la lógica.

## Dependencias

- `i18n.tr`: mensaje de enlace no admitido.
- `catalog.effective`: resuelve el manifiesto efectivo (arquitectura, variantes).
- `storage.identifier`, `storage.inside` (validación de rutas), `storage.YitoError`.
- `persistence.definitions(manifest)` (lista de pares `(relative, …)`) y `persistence.prepare(root, target, key, manifest, persist_root=None)`. Esta función crea junctions y siembra defaults. **No se documenta aquí**. Duda: confirmar su firma y si `persist_root` es opcional (el test la llama con 4 argumentos).
- Biblioteca estándar: `os.walk`, `shutil.copytree`, `shutil.copy2`, `tempfile.TemporaryDirectory`, `pathlib`.
- Dependientes: `components.py` (`publish_plan`, `remove`, `validate_update_layout`), `engine.py` (`_environment`, `component_environment`, `launch`), `persistence`, `terminal` (vía `persist_root` en los entornos).

## Notas para Rust

Firmas sugeridas (sin código):

- `fn isolated(manifest: &EffectiveManifest) -> bool` (usar el manifiesto efectivo directamente, no el lock).
- `fn persist_root(root: &Path, key: &InstallKey, profile: Option<&ProfileName>) -> Result<PathBuf, IsolationError>` (o `profile: &ProfileName` si se elimina el caso `None`; ver Duda).
- `fn payload_path(root: &Path, record: &Record, profile: &ProfileName) -> Result<PathBuf, IsolationError>`.
- `fn prepare_payload(root: &Path, record: &Record, profile: &ProfileName) -> Result<PathBuf, IsolationError>`.

Validación de nombres: reutilizar `identifier` de `storage` (ver `storage.md`). `ProfileName` se construye solo tras validar, así `payload_path` y `persist_root` no tienen que revalidar.

Diferencias:
- **Enlaces en Windows**. `std::fs::symlink_metadata` y el atributo `FILE_ATTRIBUTE_REPARSE_POINT` para distinguir junctions de enlaces simbólicos (`Path::is_symlink` no detecta junctions). Ver `components.md`.
- **Recorrido sin seguir enlaces**. Usar `walkdir` con `follow_links(false)` y comprobar reparse points manualmente antes de descender.
- **Copia**. `fs_extra::dir::copy` no permite el filtro `ignore` por ruta absoluta con la misma semántica; más directo escribir el recorrido propio con `std::fs::copy` y comprobar `excluded` por ruta completa. Duda: confirmar si `copytree` de Python sigue enlaces dentro del árbol copiado (con `followlinks=False` no entra, pero sí copia el enlace en `symlinks=False` por defecto). El test no lo cubre.
- **Atomicidad**. El directorio temporal se crea con `tempfile::tempdir_in(root/core/staging)` y el `rename` final es el paso de publicación.
- **Defaults**. Copiar `.mito-defaults` con `copy_dir_all` si es carpeta; `fs::copy` si es fichero. Duda: las rutas de `definitions` pueden usar `\` en Windows; normalizar antes de comparar.
- **Persistencia**. El enlace (`junction`) se delega en el módulo `persistence` (no documentado).

Dudas abiertas:
1. Duda: ¿`persist_root` debe aceptar `profile = None` para no aislados? Hoy el original lo acepta si el componente no está aislado porque `identifier` solo se llama en la rama aislada (`:18`); en Rust, el tipo debe reflejarlo.
2. Duda: `isolated` con `persist: []` o `''`. Ver casos límite.
3. Duda: en `prepare_payload`, si `prepare` falla tras crear la copia, la copia queda publicada. ¿Debe limpiarse?
4. Duda: confirmar la firma de `persistence.prepare` y la forma de `definitions` (ver Dependencias).
5. Duda: `ValueError` por ruta fuera de `core/apps` (`relative_to`) debe convertirse en `YitoError`.
