# components.py → mito-engine::components

Origen: `core/app/mito/components.py` (331 líneas), `core/tests/test_components.py` (17 tests), `core/tests/test_component_environment.py` (2 tests).

## Responsabilidad

Ciclo de vida genérico de los componentes instalados (no depende de ningún nombre de aplicación concreto): actualizar a la versión del catálogo, actualizar en lote, retener una versión concreta («pin») y liberarla, eliminar un componente conservando sus datos persistentes, validar que una actualización no cambia el diseño de datos, y publicar un plan completo de instalación (`publish_plan`) con rollback. También expone la inspección de entorno de un componente sin escribir nada (`component_environment`, en `engine.py:460`, que se prueba aquí).

Las funciones se llaman desde `Engine` (`engine.py`); casi todas toman `engine` como primer argumento.

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `validate_update_layout(old, manifest)` (`:9-14`) | lock instalado, manifiesto nuevo | — | `YitoError` con «La actualización cambia persist o el aislamiento…» (`:14`). |
| `check_unused(folder, removing=False)` (`:17-37`) | carpeta, flag eliminar | — | Solo Windows. `YitoError` con «Cierra la aplicación…» (`:36`/`:37`). No-op en otros SO (`:20-21`). |
| `version_key(value)` (`:41-51`) | cadena de versión | tupla comparable o `None` | — |
| `available_update(engine, lock, catalog=None)` (`:54-66`) | lock, catálogo opcional | `{'version', 'status'}` o `None` | — |
| `users(engine, installation)` (`:69-70`) | id de instalación | lista de nombres de profile que la contienen | — |
| `refresh_launchers(engine, names)` (`:73-77`) | nombres de profile | — | Propaga errores de `_write_shims` y de `terminal.prepare`. |
| `remove(engine, installation, log=print)` (`:80-145`) | id, logger | — | `YitoError` (ver tabla de mensajes). |
| `install_version(engine, key, manifest, pin, log, confirm_unverified)` (`:148-158`) | clave, manifiesto concreto, pin, logger | clave | Igual que `install`. |
| `release_version(engine, installation, log)` (`:161-173`) | id | `bool` | `YitoError` si no está instalado. |
| `update(engine, installation, log, confirm_unverified)` (`:176-189`) | id | id de instalación (`old['key']`) | `YitoError` en los casos de la tabla de mensajes; cualquier error de `engine.install`. |
| `pending_updates(engine, catalog=None)` (`:192-200`) | catálogo opcional | lista de ids con actualización `Compatible` | — |
| `update_all(engine, installations, log, confirm_unverified)` (`:203-218`) | lista de ids | lista de ids actualizados | Se detiene en el primer error (sin capturarlo). |
| `publish_plan(engine, records, current, profile, data, log)` (`:221-331`) | registros nuevos, inventario actual, profile solicitante, datos de ese profile, logger | — | Cualquier error tras el checkpoint revierte todo (ver invariantes). `YitoError` si la limpieza final falla. |

### Constantes y textos literales

Todos los textos pasan por `tr()` (`i18n`). Los valores `{pN}` son interpolaciones.

| Mensaje | Línea | Cuándo |
|---|---|---|
| `La actualización cambia persist o el aislamiento por profile; requiere una migración de datos explícita.` | `components.py:14` | `validate_update_layout` detecta cambio de aislamiento o de definiciones de persist. |
| `Cierra la aplicación antes de eliminar; archivo en uso o sin permiso de eliminación: {p1}` | `:36` y `:117` | Bloqueo detectado al eliminar (`check_unused` o `PermissionError` durante el rename). |
| `Cierra la aplicación antes de actualizar; archivo en uso o sin permiso de eliminación: {p1}` | `:37` | Bloqueo detectado al actualizar (`check_unused` sin `removing`). |
| `El componente ya no está instalado` | `:84`, `:166`, `:179` | `remove`, `release_version`, `update` sin lock. |
| `Ruta de instalación no válida para eliminar` | `:88` | La ruta del lock no es exactamente `core/apps/<bucket>/<app>` o es enlace. |
| `Copia de profile fuera de su directorio` | `:100` | Una copia por profile resuelve fuera de `core/profile-apps` o es enlace. |
| `Componente retirado; quedan archivos en uso en {p1}. Cierra la aplicación para poder limpiarlos.` | `:135` | `rmtree` del staging falla (log, no error). |
| `No se pudieron retirar los enlaces del host: {p1}` | `:142` | `unlink_locked` falla (log). |
| `Eliminado {p1} {p3}. Datos persistentes conservados.` | `:145` | Éxito de `remove`. |
| `{p1} {p3} instalado y retenido: Actualizar no lo sustituirá hasta liberarlo en Info.` | `:156` | Éxito de `install_version`. |
| `Versión liberada: {p1} {p3}. Actualizar volverá a proponer la versión del catálogo.` | `:172` | `release_version` devuelve `True`. |
| `No hay una versión posterior en el catálogo local` | `:182` | `update` sin candidato (incluye versiones retenidas, ver invariante 6). |
| `Actualizado {p1}: {p3} → {p5}. Ruta, persistencia y activación conservadas.` | `:188` | Éxito de `update`. |
| `Actualizando {p1} de {p3}: {p5}` | `:215` | Por cada componente en `update_all`. |
| `Componentes actualizados: {p1}` | `:217` | Fin de `update_all`. |
| `Ruta de instalación enlazada no válida` | `:245` | La ruta base del componente es enlace o junction. |
| `Instalación incompleta existente: {p1}` | `:248` | Existe la carpeta base de un componente que no está en el inventario. |
| `Copia de profile no válida` | `:256` | Una copia por profile es enlace o junction. |
| `PUBLICANDO` / `Publicando {p1}; ya no se puede cancelar.` | `:275` | Fase no cancelable de `publish_plan`. |
| `Componente publicado; limpieza pendiente en {p1}: {p3}` | `:331` | La publicación tuvo éxito pero no se pudo borrar el staging. |

Mensajes que **no** están en este módulo, pero que el flujo de `update`/`install` puede producir (`engine.py`): `Instalación cancelada: descargas sin hash no confirmadas` (`engine.py:305`), `Dependencia circular: {p1}` (`:245`), `Comando ausente: {p1}` (`:429`), `Instalación incompleta existente: {p1}` (`:434`).

Duda: el texto `Versión liberada: …` describe una vuelta al catálogo; el contrato de `Info` (UI) no está en el alcance de este módulo. Confirmar si la UI usa estos textos literales o los reconstruye.

## Formatos de disco

Este módulo lee y escribe los siguientes ficheros. Todos JSON UTF-8, `indent=2`, claves en orden de inserción (ver `storage.md`).

**1. Marcador de componente** `core/apps/<bucket>/<app>/.mito.json`, escrito por `engine.py:431` y por `publish_plan` (`:268-269` vía `_install_payload`):

```json
{
  "key": "main/tool",
  "manifest_sha256": "<sha256 de fingerprint(manifest)>",
  "record": {
    "key": "main/tool",
    "version": "1.9",
    "manifest_sha256": "<sha256>",
    "path": "core/apps/main/tool",
    "manifest": {
      "version": "1.9",
      "url": "http://example.org/tool.zip",
      "hash": "0000000000000000000000000000000000000000000000000000000000000000",
      "persist": "data"
    }
  }
}
```

Con retención (`install_version`), `record` incluye `pinned`:

```json
"record": {
  "key": "main/tool",
  "version": "1.5",
  "manifest_sha256": "<sha256>",
  "path": "core/apps/main/tool",
  "manifest": { "version": "1.5", "...": "..." },
  "pinned": { "version": "1.5", "source": "git", "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
}
```

`release_version` elimina la clave `record.pinned` y reescribe solo este fichero (`components.py:161-172`).

**2. Catálogo local** `core/buckets/<bucket>.json`, que los tests escriben y que lee `catalog`:

```json
{ "apps": { "tool": { "version": "2.0", "url": "https://example.org/tool.zip", "hash": "0000…" } } }
```

Duda: el fichero real puede llevar también `"source"` (URL de origen); aparece en `test_refresh_only_installed_keys_once…`. El formato completo es responsabilidad de `catalog`.

**3. Profile** `profiles/<nombre>.json` (`engine.py:88-99`):

```json
{ "workspace": "one", "apps": ["main/tool"], "env": {} }
```

`publish_plan` solo modifica el campo `apps` de los profiles afectados (`components.py:305`).

**4. Registro de accesos directos** `core/state/application-shortcuts/<nombre>.json`. Forma antes de migrar (test `test_update_migrates_registered_shortcut…`):

```json
{ "path": "<ruta .lnk>", "profile": "one", "installation": "main/tool", "badge": "TEST",
  "coupled": true, "arguments": ["--name", "two words"] }
```

Forma después de migrar (el campo pasa de `installation` a `app`):

```json
{ "path": "<ruta .lnk>", "profile": "one", "app": "main/tool", "badge": "TEST",
  "coupled": true, "arguments": ["--name", "two words"] }
```

Esta reescritura la hace `shortcuts.migrate_application_links` (`components.py:307-308`), módulo no documentado aquí (F8).

**5. Staging** (bajo `core/staging/`, nombres con UUID aleatorio):
- `core/staging/install-<uuid>/new/<i>` (payload nuevo, `components.py:262-269`),
- `core/staging/install-<uuid>/old/<i>` (payload anterior movido aside, `:281`),
- `core/staging/remove-<uuid>/<i>` (eliminación, `:109`).

El staging se borra al final (`rmtree`). En error de publicación se restaura todo y luego se borra.

## Invariantes

1. **Ruta estable**. Una actualización conserva `record['path']` (`core/apps/<bucket>/<app>`). El test `test_update_switches…` y `test_update_installs_new_payload…` lo comprueban.
2. **Diseño de datos inmutable en actualización**. `validate_update_layout` rechaza cambiar el aislamiento (`isolated`) o el conjunto de definiciones de `persist` (`components.py:9-14`). La comparación de conjuntos es sobre `definitions(effective(...))`, no sobre el orden.
3. **Sin cambios de perfil antes de instalar**. Si `engine.install` falla, ningún profile cambia (`test_update_switches_profiles_only_after_install_succeeds`).
4. **Plan completo antes de reemplazar**. Todas las descargas y validaciones (`bin` presente, dependencias, hashes) se hacen en staging antes de mover nada. `publish_plan` solo mueve tras `operation.checkpoint()` (`:274`). Ver también `activation.md` (`test_entire_plan_is_prepared_before_any_replacement`).
5. **Punto sin retorno**. Después de `checkpoint()` y de la fase `PUBLICANDO` (`:275`) la operación no es cancelable. Desde ese punto, cualquier excepción dispara el bloque `except` (`:277-326`) que restaura en este orden:
   a. enlaces de accesos directos (`links`, contenido previo o borrado si no existía),
   b. borra lo publicado (`published`, en orden inverso),
   c. restaura lo movido (`moved`, en orden inverso),
   d. restaura los profiles escritos (`written`) desde el snapshot inicial,
   e. borra el staging,
   f. `refresh_launchers(affected)`,
   g. relanza la excepción original.
6. **Versión retenida no se actualiza**. Si `lock['pinned']` existe, `available_update` devuelve `None` (`components.py:55-56`), y `update` falla con «No hay una versión posterior…» (`:182`). `update_all` omite el componente sin error.
7. **Solo versiones estrictamente mayores**. `available_update` exige `new > old` según `version_key` (`:61-62`). Un manifiesto igual o inferior no es candidato.
8. **Solo candidatos `Compatible`**. `update` lanza el propio texto de `status` si no es `Compatible` (`:183-184`); `pending_updates` y `update_all` los omiten.
9. **Sin cambio de profile fuera de sus dependencias**. En `publish_plan`, a un profile que no es el solicitante solo se le añaden las dependencias (recursivas, vía `dependencies()` `:226-239`) de los componentes reemplazados que ya tiene. Nunca se añade el componente solicitante (`:298-304`). Ver `test_dependency_replacement…` en `activation.md`.
10. **Profiles afectados**. `affected` = profiles que contienen un id que cambia **y que ya estaba instalado** (`:227-230`), más el profile solicitante. Los componentes nuevos no añaden perfiles afectados.
11. **Copias por profile en aislamiento**. Para cada componente aislado (`isolated(record)`), se reemplazan todas las copias `core/profile-apps/<p>/<bucket>/<app>` existentes, incluidas las de profiles inactivos o sin fichero `profiles/<p>.json` (`:251-260`). Duda: la lógica depende de que el directorio de la copia exista; ver `activation.md` (`test_migration_keeps_profile_copies…`).
12. **Enlaces rechazados**. Si la ruta base o una copia es enlace simbólico o junction, la publicación se aborta antes de tocar nada (`:244-245`, `:255-256`).
13. **Recheck tras descargar**. `check_unused` se vuelve a ejecutar después de la fase no cancelable (`:284-285`), porque otro proceso pudo abrir un binario mientras se descargaba.
14. **Eliminación sin tocar persistencia**. `remove` mueve el payload y las copias a staging y borra el staging; la carpeta `home/persist/...` o `home/profiles/<p>/persist/...` no se toca. Los enlaces `junction` dentro del payload se eliminan sin seguirlos (`:132`).
15. **Eliminación solo de rutas canónicas**. Antes de mover nada, `remove` exige que `payload.resolve()` esté bajo `core/apps` y que la ruta relativa tenga exactamente dos segmentos (`:88`). Esto impide eliminar la raíz de un bucket o un directorio ajeno.
16. **Eliminación de todas las copias**. Si el componente está aislado, se eliminan las copias de **todos** los directorios de `core/profile-apps/*/<bucket>/<app>`, incluidos los de profiles que ya no existen (`:93-103`). Así no quedan copias huérfanas.
17. **Eliminación desregistra el componente de sus profiles**. Solo se reescriben los profiles que lo contenían (`users`, `:69-70`), y los `apps` resultantes no contienen el id (`:121-124`). Si falla la escritura de algún profile, se revierten todos los ya escritos (`:127-131`) y se restauran los movimientos (`:112-119`).
18. **Liberar no toca payload ni profiles**. `release_version` modifica solo `.mito.json`. El payload y su versión se mantienen (docstring `:162`).
19. **Pin persistente entre dependencias**. Si un componente retenido se instala como dependencia de otro, `_installation_plan` conserva su manifiesto y su `pinned` (`engine.py:241-243`). Ver `test_install_version_pins…`.
20. **Comparación de versiones**. Ver `version_key` más abajo; el orden de comparación es el de la tupla `(numbers, not prerelease, parts)`.

## Casos límite y errores

### version_key (`components.py:41-51`)

- Expresión regular de entrada: `[vV]?(\d+(?:[._-]\d+)*)(.*)`. Si no encaja → `None` (y `available_update` devuelve `None`, `:58-59`).
- Los números se separan por `.`, `_` o `-`. Los ceros finales se eliminan (`1.0.0` ≡ `1`, `2.0` ≡ `2`), pero el primer número se conserva siempre.
- Sufijo: `suffix = match[2].lower().lstrip('-._+')`. Es prerelease si empieza por `alpha`, `beta`, `preview`, `pre`, `rc`, `ea`, `dev` o `snapshot` (`re.match`, es decir, prefijo, no palabra completa). Ejemplo: `2.0-ea` y `2.0-earth` son prerelease.
- `parts`: `re.findall(r'\d+|[a-z]+', suffix)`. Un dígito se codifica `(1, int)` y un texto `(0, str)`. Por tanto, en el mismo prefijo numérico, un texto ordena **antes** que un número.
- Orden de comparación: primero `numbers` (lexicográfico, `[1,10] > [1,9]`), luego `not prerelease` (la versión estable gana a la prerelease de los mismos números), luego `parts`.

Ejemplos verificados con la lógica del original:
- `version_key('1.10') > version_key('1.9')` (`:1.10` vs `:1.9`).
- `version_key('2.0') > version_key('2.0-rc1')` (`rc` es prerelease).
- `version_key('1.0.0') == version_key('1')`.
- `version_key('abc') is None`.

Duda: `int(...)` de Python no tiene límite. Un número mayor de `u64` en un componente real provocaría desbordamiento en Rust. ¿Debe fallar en tal caso o tratarse como texto?

### check_unused (`components.py:17-37`)

- Solo Windows. En otros SO no hace nada (`:20-21`).
- Recorre `os.walk(folder, followlinks=False)` y **no entra** en junctions ni en enlaces simbólicos (`:27`).
- Para cada fichero abre `CreateFileW(path, 0x10000, 7, None, 3, 0, None)`: `0x10000` es `DELETE`, `7` es `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, `3` es `OPEN_EXISTING`. Si falla, lanza el error de la tabla de mensajes. Un fichero abierto sin `FILE_SHARE_DELETE` falla aquí (es la prueba `test_remove_in_use…`).
- Cierra el handle de cada fichero que sí se pudo abrir.

### remove (`components.py:80-145`)

- Si `engine.installed()` no contiene el id, `El componente ya no está instalado` (`:84`).
- Antes de `check_unused`, se construyen `targets` = payload más copias existentes (si isolated). Las copias de profiles inexistentes también se eliminan (`:95-102`).
- `PermissionError` durante el rename (`:116-117`) → mensaje «Cierra la aplicación…» con `exc.filename` o la ruta del payload. Cualquier otra excepción se relanza tal cual (`:118`).
- Si falla `rmtree(stage)` (`:133-135`) no es error: solo se registra el aviso. Duda: el aviso no garantiza que el componente sea recuperable; el staging queda con ficheros en uso.
- Los fallos de `unexpose` (`host_exposure`) y de `unregister` (`open_with`) se registran o se ignoran; no revierten nada (`:136-144`).
- Orden de operaciones: check → mover a staging → reescribir profiles → borrar staging → refrescar launchers (`refresh_launchers(backup)`, `:136`) → desexponer host → desregistrar «Abrir con» → log final.

### update (`components.py:176-189`)

- Orden de validación: lock presente (`:178`), candidato (`:181`), `status == 'Compatible'` (`:183-184`), `validate_update_layout` con el manifiesto del catálogo (`:186`), luego `engine.install(old['key'], …)` sin `profile` ni `manifests`/`pins` (`:187`).
- Si `candidate['status']` no es `Compatible`, el mensaje de error es el propio `status`, sin `tr()` (`:184`).

### update_all (`components.py:203-218`)

- Usa `available_update(engine, lock)` sin catálogo explícito, así que lee el catálogo cada vez (`:211`).
- Si el componente ya no tiene candidato (p. ej. se actualizó como dependencia de otro anterior), se omite sin log (`:213-214`).
- El mensaje `Actualizando {p1} de {p3}` usa la longitud de la lista de entrada, no el número de actualizables (`:215`). Duda: es probable que sea deliberado (progreso sobre la selección), pero conviene confirmarlo con la UI.

### publish_plan (`components.py:221-331`)

- Fallo de descarga o validación **antes** del checkpoint: solo se borra el staging, nada más se ha tocado. Sin rollback de profiles.
- `Instalación incompleta existente` (`:248`) cuando existe la carpeta base de un componente nuevo sin entrada en el inventario. No se sobrescribe.
- `prepare_payload(engine.root, record, profile or 'default')` (`:296`) para componentes no aislados: el persist es el compartido, así que el nombre de profile solo importa para la copia (no aislada, no se usa).
- Errores de `refresh_launchers` en el camino de éxito (`:310`) disparan el rollback completo, incluido `refresh_launchers` en el `except` (`:326`). Por eso el test de accesos directos usa `side_effect=[OSError, None]`.
- Si la limpieza final falla (`:329-331`), la publicación ya es definitiva y se lanza `YitoError`. El llamador debe distinguir «publicado con error de limpieza» de «no publicado». Duda: confirmar que la UI trata este error como éxito parcial.

### Inspección sin efectos (component_environment, `engine.py:460-472`)

Ver `test_component_environment.py` en la tabla de tests. `component_environment` no escribe nada en disco, ni siquiera crea `core/`. El valor de `persist_dir` se calcula con `persist_root(root, lock, profile)` aunque el componente no sea aislado (en ese caso, `profile` se ignora).

## Casos de test del original

Notación de dependencia. Las fases son las de la hoja de ruta:
- **F5** terminal (`terminal.prepare`, `terminal.command`). Lo llama `refresh_launchers` (`components.py:76`) y `Engine.set_active` (`engine.py:171-172`).
- **F7** hooks / MSI / Inno. Lo llama `_install_payload` (`engine.py:413-422`: `run_pre_install`, `run_installer`, y `restore_declared_targets` si `innosetup`). Se ejecuta en **toda** instalación. Duda: confirmar si basta un no-op cuando el manifiesto no declara hooks.
- **F8** accesos directos e iconos. Lo llama `publish_plan` (`components.py:307-308`) en **toda** publicación y lo exige el test de migración de accesos (`shortcuts.write_link`, `shortcuts.application_icon`). Duda: como `migrate_application_links` se llama siempre, en tests sin accesos debe comportarse como no-op.
- **F10** pocketools. Lo llama `Engine._environment` (`engine.py:559-560`). Se activa en cualquier test que llame a `environment()`.
- **F11** migración / traslado.
- **Otros** (no asignados en la hoja de ruta): `catalog`, `persistence`, `host_exposure`, `open_with`, `operation`, `i18n`.

### test_components.py (17 tests)

| Test | Qué comprueba | Fase posterior |
|---|---|---|
| `test_numeric_versions_and_unsupported_update` | Orden numérico de `version_key` (`1.10 > 1.9`, `2.0 > 2.0-rc1`, `1.0.0 == 1`). `available_update` devuelve `2.0` con estado distinto de `Compatible` cuando hay `installer` y `None` si la versión es igual. | Ninguna (depende de `catalog.compatibility`). |
| `test_refresh_only_installed_keys_once_without_changing_payloads_or_profiles` | `refresh_installed` descarga una sola vez el manifiesto de los componentes instalados, sin tocar inventario, payloads ni profiles. Las claves no instaladas se ignoran. | Ninguna (`catalog`). |
| `test_refresh_local_partial_failure_preserves_previous_manifest` | Un JSON de catálogo inválido o un fallo de red no pierden el manifiesto anterior; devuelve el número de entradas refrescadas. | Ninguna (`catalog`). |
| `test_refresh_selected_uninstalled_key_preserves_other_entries_and_inventory` | `refresh_keys` actualiza solo la clave pedida, aunque no esté instalada, y deja el inventario intacto. | Ninguna (`catalog`). |
| `test_remove_retains_persist_and_other_profiles` | `remove` elimina el payload, quita el id de todos los profiles, deja el inventario vacío y conserva `home/persist/main/tool/keep.txt`. | F5 (`refresh_launchers`), F8/otros (`open_with.unregister`). |
| `test_update_switches_profiles_only_after_install_succeeds` | Si `install` falla, ningún profile cambia. Si tiene éxito, todos los profiles pasan al id nuevo y el inventario solo contiene el id viejo con versión `1.10`. | F5, F7, F8. |
| `test_update_preserves_other_components_and_inactive_profiles` | Actualizar `main/tool` no toca `versions/tool` ni su presencia en el profile `two`. | F5, F7, F8. |
| `test_pending_updates_lists_only_compatible_newer_versions` | `pending_updates` devuelve solo ids con versión mayor y estado `Compatible`. | Ninguna. |
| `test_update_all_runs_each_update_and_stops_at_first_failure` | Con `update` simulado: omite `main/done`, ejecuta en orden, se detiene en `main/broken` con `YitoError`. Las llamadas esperadas son `['main/a','main/a','main/broken']`. | Ninguna (update simulado). |
| `test_update_layout_change_is_rejected_before_install` | Cambiar `persist` de `data` a `other-data` lanza «…migración» y no llama a `install`. | Ninguna. |
| `test_update_rollback_restores_profiles_and_payload` | Si falla `save_profile` del profile `two` tras haber guardado `one`, ambos quedan con el id viejo, el inventario dice `1.9` y el payload existe. | F5, F7, F8. |
| `test_update_locked_binary_keeps_old_references` | Con `tool.exe` abierto sin `FILE_SHARE_DELETE` se lanza «Cierra la aplicación antes de actualizar», el profile conserva el id viejo y el binario sigue. | Solo Windows (`CreateFileW`); F7 en la instalación. |
| `test_update_installs_new_payload_and_preserves_shared_persist` | Tras actualizar, `tool.exe` es el nuevo, el junction `data` sigue, y `data/user.txt` conserva `keep`. Se llama también a `environment('one')`. | F5, F7, F8, F10. |
| `test_update_migrates_registered_shortcut_and_rolls_it_back_on_failure` | Un acceso directo registrado se reescribe a la versión nueva. Con fallo en el primer `refresh_launchers`, el `.lnk` y el registro vuelven al original. Tras reintentar, el registro pasa a usar `app`. | F5, F8. |
| `test_install_version_pins_until_released_and_survives_dependents` | `install_version` retiene `1.5`; `available_update` devuelve `None`; instalar una dependencia no baja ni sube la retenida; `release_version` devuelve `True` una vez y `False` después; tras liberar, `available_update` propone `2.0`. | F5, F7, F8. |
| `test_remove_in_use_reports_open_application_and_keeps_payload` | Con `tool.exe` abierto sin `FILE_SHARE_DELETE`, `remove` lanza «antes de eliminar», no deja staging, el payload y el profile siguen, y tras cerrar se elimina. | Solo Windows. |
| `test_failed_remove_profile_write_restores_payload` | Si falla `save_profile` durante `remove`, el payload vuelve y el profile conserva el id. | Ninguna (el fallo ocurre antes de `refresh_launchers`). |

### test_component_environment.py (2 tests)

| Test | Qué comprueba | Fase posterior |
|---|---|---|
| `test_inspection_resolves_scope_architecture_and_bin_without_writes` | `component_environment` expande `$dir`, `${dir}`, `$persist_dir`, `${persist_dir}` y deja `LITERAL` igual. Con `architecture.64bit.env_add_path = bin` gana sobre `env_add_path = 'wrong'`. Devuelve `[payload/bin, payload/other]` sin duplicados. Con `shortcuts` (aislado) usa `core/profile-apps/one/...` y `home/profiles/one/persist/...`; sin ellos usa `core/apps/...` y `home/persist/...`. No crea nada en la raíz. | Ninguna. |
| `test_empty_contributions` | Manifiesto `{}` con profile `None` devuelve `({}, [])` sin crear nada. | Ninguna. |

## Dependencias

- `i18n.tr` (`mito-i18n`): mensajes con `{pN}`.
- `storage`: `YitoError`, `inside`, `mutation` (lock de proceso), `read_json`, `write_json`.
- `catalog`: `effective`, `array`, `fingerprint`, `Catalog.all()`, `Catalog.resolve()`, `Catalog.compatibility()`, `refresh_installed`, `refresh_keys`.
- `isolation`: `isolated`, `payload_path`, `prepare_payload` (ver `isolation.md`).
- `persistence`: `definitions` (lista de `persist`), `prepare` (enlaces de persistencia).
- `engine` (`Engine`): `installed`, `installation_plan`, `install`, `profile`, `profiles`, `save_profile`, `_write_shims`, `_install_payload`, `catalog`, `root`, `environment`, `install_id`.
- `terminal.prepare` (F5), `shortcuts.migrate_application_links` / `write_link` / `application_icon` (F8), `host_exposure.unlink_locked` (no asignado), `open_with.unregister` (Duda: ¿F8?), `operation.phase` / `operation.checkpoint` (progreso/cancelación).
- Biblioteca estándar: `re`, `shutil`, `uuid`, `os`, `ctypes` (solo Windows).

## Notas para Rust

Firmas sugeridas (sin código):

- `version_key(&str) -> Option<VersionKey>` con `VersionKey { numbers: Vec<u64>, stable: bool, parts: Vec<Part> }`, `#[derive(Ord)]`. `Part` como `enum { Text(String), Num(u64) }` en ese orden de variantes, así el derive ordena `Text < Num` como en Python. Duda: `Vec<u64>` lexicográfico equivale a la tupla de Python.
- `available_update(engine: &Engine, lock: &Lock, catalog: Option<&Catalog>) -> Option<Candidate>`, `Candidate { version: String, status: String }`.
- `remove(engine: &Engine, installation: &str, log: &mut dyn FnMut(&str)) -> Result<(), ComponentError>`.
- `publish_plan(…)` debe devolver `Result` y encapsular el rollback en una estructura de transacción (`Vec` de acciones de deshacer, ejecutadas en orden inverso). Así se evita duplicar la lógica de `except`.
- Para inyectar fallos (el original usa `patch`), usar un trait o parámetros para `refresh_launchers`, `prepare_payload` y `save_profile`. Los tests de `test_components.py` dependen de ello.

Diferencias inevitables:
- **Bloqueo de archivos en Windows**. `check_unused` usa `CreateFileW` con `DELETE` (`0x10000`) y `share 7`. En Rust: `windows-sys::Win32::Storage::FileSystem::CreateFileW` con los mismos parámetros. Para no seguir junctions usar `symlink_metadata` y comprobar `FILE_ATTRIBUTE_REPARSE_POINT`.
- **Junctions**. `Path::is_symlink` de Rust no detecta junctions. Usar `symlink_metadata` + `FILE_ATTRIBUTE_REPARSE_POINT` (o la crate `junction`).
- **Eliminar sin seguir junctions**. `std::fs::remove_dir_all` no debe atravesar junctions, pero conviene verificarlo con un test. Duda: confirmar el comportamiento de la versión de Rust/crate que se use.
- **Renombrar directorios**. `std::fs::rename` sobre directorios en Windows hace `MoveFileEx`, equivalente a `os.replace`/`Path.rename` de Python. Los reintentos por antivirus (`test_relocation_rebuilds…`, `rename_retrying`) deben reproducirse en Rust.
- **Versiones**. `regex` y `once_cell` para la expresión de `version_key`.
- **Logs**. `log(tr(...))` en el original recibe cadenas. En Rust, un callback `FnMut(&str)`.

Dudas abiertas:
1. Duda: `update_all` imprime `de {p3}` con la longitud de la entrada y no con el número de actualizables (`components.py:215`). ¿Es intencionado?
2. Duda: tras un fallo de la limpieza final (`:331`) el componente está publicado. ¿La UI debe mostrarlo como éxito parcial?
3. Duda: `remove` (`:133-135`) acepta ficheros en uso en staging sin error. ¿Debe registrarse un estado persistente para reintentar la limpieza?
4. Duda: `Compatible` es un literal del catálogo (`catalog.py`). Confirmar el conjunto completo de estados.
5. Duda: `version_key` no acota el tamaño de los números (ver sección de casos límite).
6. Duda: `pending_updates` y `update_all` leen el catálogo; `update` no recibe catálogo y lo lee de nuevo. Confirmar si el catálogo debe ser un snapshot por operación.
