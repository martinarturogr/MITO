# host_exposure.py → mito-win::host_exposure

Origen: `core/app/mito/host_exposure.py` (242 líneas), `core/tests/test_host_exposure.py` (6 tests).
Flujo de UI: `core/app/mito/ui.py`, rama `host-expose` (línea 1770) y botón `expose-<n>` «Host · ✓/✗» (línea 1127).

## Responsabilidad

Es la inversa de `host_data.py`. Crea, a petición explícita y para toda la instancia, junctions en el AppData del
host (`%APPDATA%` para `data`, `%LOCALAPPDATA%` para `cache`) que apuntan a las carpetas de datos de MITO
(`home/AppData/<Roaming|Local>/<relativo>`). Así, una aplicación que arranca directamente desde el host (inicio
automático, asociaciones de archivo, otro programa) encuentra sus datos dentro de MITO. Solo se comparten datos:
el entorno del profile no se aplica (docstring del módulo). Si la carpeta del host ya existe, se renombra
(«apartar») y se restaura al desenlazar; nunca se borra.

## API pública del original

| Función / constante | Entradas | Salida | Errores |
|---|---|---|---|
| `REGISTRY` (18) | — | `'core/state/host-exposed-links.json'` | — |
| `registry(root)` (21) | raíz | `dict` del registro o `{}` | `read_json` |
| `save(root, records)` (26) | raíz, dict | `None` | E/S |
| `normal(path)` (30) | ruta | cadena normalizada (sin `\\?\` ni `\??\`, `normcase(normpath)`) | — |
| `points_to(link, folder)` (38) | junction, carpeta | `bool`: `link.is_junction()` y destino normalizado igual; no resuelve el destino (puede estar roto) | captura `OSError`, `ValueError` → `False` |
| `junction_target(link)` (46) | junction | cadena normalizada o `None` | — |
| `words(engine, lock)` (53) | motor, lock | lista ordenada de stems (casefold) de los ejecutables declarados, con `len >= 3` | — |
| `candidates(engine, lock)` (58) | motor, lock | lista de tuplas `(kind, relative_str)` | — |
| `plan(root, kind, relative)` (85) | raíz, `kind`, ruta relativa | `(source, host)`: origen en MITO y destino en el host | ver «Casos límite» |
| `create_junction(path, folder)` (105) | ruta, carpeta destino | `None` (`check=True`) | `subprocess.CalledProcessError` |
| `backup_path(host)` (112) | carpeta del host | `host.with_name(f'{name}.mito-backup-YYYYMMDD-HHMMSS')` | — |
| `entries(root, installation)` (116) | raíz, identidad | lista de tuplas `(kind, record)` del registro | — (no descubre junctions) |
| `linked(root, installation)` (122) | raíz, identidad | `bool` | captura errores de `plan` |
| `link(engine, installation, kind, relative, aside=False)` (133) | motor, identidad, kind, relativo, `aside` | `None` o la ruta de la carpeta apartada (`Path`) | ver «Casos límite» |
| `unlink_locked(root, installation)` (177) | raíz, identidad | `None` | `YitoError` de integridad |
| `unlink(root, installation)` (197) | igual | `None` | igual, dentro de `mutation(root)` |
| `relocate(root, old, copied, log=print)` (202) | raíz, antigua raíz, `copied`, `log` | número de enlaces afectados | captura y registra errores por enlace |

`KINDS` no se define aquí: se importa de `host_data` (`{'data': 'Roaming', 'cache': 'Local'}`).

## Formatos de disco

**Registro** `core/state/host-exposed-links.json` (`write_json`, mismo formato que `host-data-links.json`, ver
`host_data.md`). Primer nivel: identidad de instalación (`main/mattermost`, la clave `bucket/app` del componente). Segundo nivel:
`data` o `cache`. Lista de `{relative, backup}`:

```json
{
  "main/mattermost": {
    "data": [
      {
        "relative": "Mattermost",
        "backup": null
      }
    ]
  }
}
```

Con carpeta del host apartada:

```json
{
  "main/mattermost": {
    "data": [
      {
        "relative": "Mattermost",
        "backup": "C:\\Users\\usuario\\AppData\\Roaming\\Mattermost.mito-backup-20261127-101500"
      }
    ]
  }
}
```

- `relative`: ruta bajo `home/AppData/Roaming` o `Local` (la misma que en MITO).
- `backup`: ruta absoluta de la carpeta apartada, o `null` si no había carpeta previa.
- Junction en el host: `C:\Users\usuario\AppData\Roaming\Mattermost` → `<root>\home\AppData\Roaming\Mattermost`.
- Ruta de apartado: hermana de la carpeta del host, con nombre `Mattermost.mito-backup-AAAAMMDD-HHMMSS` (hora local, segundos).
- Lock: `mutation(root)` → `core/state/mutation.lock`.

## Invariantes

1. `link` y `unlink` van bajo `mutation(root)`. `linked`, `entries`, `candidates`, `plan` no escriben.
2. Solo se crean junctions cuando el componente está instalado (`installation in engine.installed()`; error si no).
3. `plan` exige que la ruta portable sea una carpeta real existente: `Se esperaba una carpeta de datos en MITO: {p1}`.
   Además, ningún componente de la ruta portable (desde `relative` hasta la base) puede ser junction o symlink.
4. El destino del host debe quedar dentro de la base del AppData del host (`host.parent` resuelto dentro de `host_base`),
   no puede ser la base y no puede haber solapamiento circular con la raíz de MITO.
5. Conflicto: ninguna ruta del host puede ser igual, ancestro o descendiente de otra ya registrada (todas las
   categorías de todas las instalaciones). Excepción idempotente: misma instalación, misma categoría, misma ruta y
   `points_to(host, source)` → retorna sin cambios.
6. Si `host` ya es un junction que apunta a `source`, `link` solo registra y devuelve `None` (no crea nada).
7. Si `host` es junction o symlink que no apunta a `source`: `No se reemplazan enlaces ajenos a MITO` (aunque `aside=True`).
8. Si `host` existe y no es un junction: `aside=False` → `La carpeta del host ya existe: {p1}` (no se toca nada);
   `aside=True` → se renombra a `backup_path(host)` (nunca se borra) y se devuelve esa ruta.
9. Si falla la creación del junction: se borra el junction parcial (`os.rmdir` si `is_junction()`), se restaura la
   carpeta apartada (`moved.rename(host)`) y se relanza la excepción. El registro no se guarda.
10. `unlink_locked`: por cada registro, el junction del host debe apuntar a la base de MITO (`points_to`); si no:
    `El enlace ha cambiado fuera de MITO: {p1}`. Se borra solo el junction (`os.rmdir`). Si la ruta del host existe como
    carpeta/archivo real: `Se esperaba una junction gestionada: {p1}`. Después, si hay `backup` y es un directorio y el host
    no existe, se renombra de vuelta a `host`. Al final se elimina la identidad del registro y se guarda.
11. `unlink` nunca borra datos del host ni datos de MITO: solo quita junctions y restaura carpetas apartadas.
12. `relocate` (tras mover o copiar la instancia): los enlaces cuyo destino del host apunta a la antigua raíz
    (`target == old` o empieza por `old + \`) se tratan así: en **movimiento** (`copied=False`) se recrea el junction
    hacia la nueva ruta (`os.rmdir` + `create_junction`) y se registra `Enlaces del host actualizados a la nueva ruta: {p1}`;
    en **copia** (`copied=True`) el enlace se suelta del registro y el junction sigue apuntando a la instancia original.
    Solo se guarda el registro si hubo cambios; no se llama a `notify` (no hay SHChangeNotify aquí).
13. Los errores de `relocate` por enlace no abortan: `No se pudo actualizar el enlace del host {p1}: {p3}`, y el registro conserva la entrada.

## Casos límite y errores

| Clave (texto español) | Cuándo |
|---|---|
| `El componente ya no está instalado` | `link` sin la instalación en `engine.installed()` |
| `Se esperaba una carpeta de datos en MITO: {p1}` | `plan`: la ruta portable no es un directorio |
| `No se reemplazan enlaces ajenos a MITO` | junction/symlink en la ruta portable (`plan`) o en el destino del host (`link`) |
| `Destino fuera del AppData de MITO` | `plan`: destino del host fuera de la base o igual a la base |
| `Selecciona datos externos a MITO, sin enlaces circulares` | `plan`: solapamiento entre base del host y raíz de MITO |
| `Esta ruta del host ya está gestionada por MITO` | conflicto de rutas con un registro (invariante 5) |
| `La carpeta del host ya existe: {p1}` | destino existente sin `aside`, o existente y no directorio con `aside` |
| `Enlaces del host actualizados a la nueva ruta: {p1}` | log de `relocate` |
| `No se pudo actualizar el enlace del host {p1}: {p3}` | log de `relocate` por error |
| `El enlace ha cambiado fuera de MITO: {p1}` | `unlink_locked` |
| `Se esperaba una junction gestionada: {p1}` | `unlink_locked` |

Otros: `backup_path` usa `time.strftime('%Y%m%d-%H%M%S')` con resolución de un segundo; dos apartados en el mismo
segundo sobre la misma carpeta provocan colisión (ver Dudas).

## Casos de test del original

| Test | Archivo:líneas | Qué comprueba |
|---|---|---|
| `test_candidates_use_declared_executable_names_and_skip_junctions` | test_host_exposure.py:52-64 | `candidates` devuelve `('data','Mattermost')`, `('data','Other/Mattermost')` (anidada, por ruta relativa) y `('cache','mattermost-updater')`; ignora `Mattermost-linked` (junction). |
| `test_link_shares_mito_data_and_unlink_keeps_it` | :66-80 | `link` devuelve `None`, crea junction, el host ve `config.json`; `linked` es `True`; repetir no llama a PowerShell; `unlink` borra el junction y los datos de MITO siguen; registro vacío. |
| `test_existing_host_folder_is_set_aside_and_restored` | :82-95 | Carpeta real del host: `link` sin `aside` → `YitoError`; con `aside=True` devuelve la carpeta apartada con su contenido; `unlink` la restaura con su contenido original. |
| `test_foreign_or_modified_junctions_are_never_replaced` | :97-110 | Junction ajeno en el host: `link` con `aside=True` falla; tras quitarlo, un `link` correcto y luego un junction ajeno sustituto hacen que `unlink` falle sin tocar el junction. |
| `test_failed_junction_restores_host_folder` | :112-120 | Si `create_junction` lanza `OSError`, la carpeta del host vuelve a su sitio con su contenido y el registro queda vacío. |
| `test_relocation_retargets_after_move_and_releases_after_copy` | :122-135 | Tras mover (`copied=False`) el junction apunta a la nueva instancia y sigue `linked`; tras copiar (`copied=True`) el junction sigue apuntando a la antigua y el registro queda vacío. |

## Dependencias

- `host_data`: `KINDS`, `destination`, `host_folder` (esta última también se parchea en los tests).
- `i18n.tr`; `storage`: `YitoError`, `mutation`, `read_json`, `write_json`.
- `engine`: `installed()`, `application_entries(lock)`, `root`, `selected`.
- `subprocess` (PowerShell `New-Item -ItemType Junction`).
- Llamadores: `ui.py` (`host-expose`, `expose-<n>`, `linked` para el botón), `relocation.py` (`relocate`, no revisado en este contrato).

## Flujo de UI (`host-expose`, ui.py:1770)

Acción sin diálogo previo; no exige profile. Textos exactos:

1. Si `exposure.entries(root, target)` tiene registros: confirmación
   `¿Eliminar los enlaces del host?\n{p1}\nLos datos de MITO se conservan.` (`p1` = rutas del host unidas por `\n`, botón `Desenlazar`) → `unlink`, y log `Enlaces del host retirados. Los datos de MITO se conservan.`
2. Si no: `candidates(engine, installed[target])`. Si no hay candidatas, log `No hay carpetas en el AppData de MITO que coincidan con el ejecutable. Abre antes la aplicación desde MITO.`
3. Por cada `(kind, relative)`: `plan`; confirmación
   `{p0} en el host: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nSi la aplicación arranca fuera de MITO (inicio automático, asociaciones) usará estos datos, pero no el entorno del profile. Cierra la aplicación antes de enlazar.`
   con `p0` = `KINDS[kind]` (`Roaming` o `Local`, sin traducir), `p2` = `host`, `p4` = `source`. Si se rechaza, se salta.
4. `aside = (host.exists() or host.is_junction()) and not points_to(host, source)`. Si `aside`: confirmación
   `La carpeta del host {p1} ya existe. Se renombrará a {p3} y se restaurará al desenlazar.` (`p1` = host, `p3` = `backup_path(host).name`, botón `Apartar y enlazar`). Si se rechaza, se salta.
5. `link(engine, target, kind, relative, aside=aside)` y log `Enlace creado en el host: {p1}` (`p1` = host).

El botón «Host · ✓» se muestra cuando `linked(root, installation)` es verdadero (sin junctions descubiertas). Se
deshabilita con `busy` y se muestra solo si `can_open(lock)`. Clase `host-data-button`, id `expose-<n>`.

## Notas para Rust

- Crate `mito-win`. Mismas reglas de `unsafe` que `host_data` (`// SAFETY:` por bloque).
- Reproducir `normal()`: quitar prefijos `\\?\` y `\??\`, normalizar y pasar a minúsculas (`normcase` en Windows).
- Junctions: como en `host_data.md` (comando PowerShell o `FSCTL_SET_REPARSE_POINT`). Borrar con `remove_dir` sobre el junction.
- Apartado: `std::fs::rename` dentro de la misma carpeta padre, con el nombre `…mito-backup-%Y%m%d-%H%M%S` (`chrono` o `time`, zona local).
- `relocate` lo llama la lógica de movimiento/copia de instancia: no tiene equivalente de `notify`.

## Dudas

1. Colisión de nombres en `backup_path` (resolución de segundo): dos apartados de la misma carpeta en el mismo segundo fallan en `rename`. ¿Añadir sufijo o contador? Hoy el original no lo contempla.
2. Coherencia de la palabra: `words` ignora ejecutables de menos de 3 caracteres; `host_data.candidates` usa el stem sin mínimo. ¿Es intencional?
3. `entries`/`linked` solo leen el registro; un junction creado a mano dentro del host no cuenta como «Host · ✓» (en `host_data` sí se descubre). Confirmar.
4. `unlink_locked` puede fallar a mitad: si un registro lanza, los anteriores ya hicieron `rmdir` y no se guarda el registro. No hay rollback. ¿Aceptable?
5. En la UI, `aside` puede ser `True` con `host` como junction ajeno; entonces `link` lanza `No se reemplazan enlaces ajenos a MITO` y aborta toda la operación (no se captura por candidata). Confirmar el manejo deseado.
6. `plan` puede lanzar dentro del bucle de la UI antes de cualquier confirmación; no hay captura por candidata. Confirmar.
7. `relocate` con `copied=True` deja el junction en el host apuntando a la instancia original. Confirmar que la copia no debe reclamarlo.
8. Etiqueta por defecto de `HostDataConfirm` para este flujo: `confirm_host_data(message, label=None)` no fija botón; el texto del botón de aceptar lo decide `host_apps_ui.py` (no revisado).
9. `KINDS[kind]` se inserta sin `tr` en el mensaje de confirmación (`Roaming`/`Local`). Es un nombre de carpeta de Windows; confirmar que no debe traducirse.
