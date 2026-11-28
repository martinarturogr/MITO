# host_data.py → mito-win::host_data

Origen: `core/app/mito/host_data.py` (222 líneas), `core/tests/test_host_data.py` (10 tests),
`core/tests/test_host_data_ui.py` (2 tests, Textual). Flujos de UI en `core/app/mito/ui.py`:
`offer_host_data` (1684), `confirm_host_data` (1680), rama `host-add` (1725), rama `host-cache`/`host-data` (1746).

## Responsabilidad

Enlaza, a petición explícita y para toda la instancia, carpetas de datos del AppData del host hacia el AppData
portable de MITO. `data` = `%APPDATA%` (Roaming) y `cache` = `%LOCALAPPDATA%` (Local). Cada enlace es una
junction NTFS creada en `home/AppData/<Roaming|Local>/<relativo>` que apunta a la carpeta del host. Los enlaces
son de instancia: los comparten todos los profiles (texto de la confirmación de UI). Es el sentido inverso de `host_exposure.py` (ver `host_exposure.md`).

## API pública del original

| Función / constante | Entradas | Salida | Errores |
|---|---|---|---|
| `KINDS` (12) | — | `{'data': 'Roaming', 'cache': 'Local'}` | — |
| `host_folder(kind)` (15) | `'data'`/`'cache'` | `Path` de AppData del host | `YitoError('No se pudo localizar AppData del host')` si `SHGetFolderPathW` falla |
| `candidates(root, identity, kind)` (24) | raíz, identidad de host_apps, `kind` | lista de `Path` de carpetas del host | `YitoError` de `host_apps.target` |
| `registry(root)` (47) | raíz | `dict` de `core/state/host-data-links.json` o `{}` si no existe | `read_json` |
| `save(root, records)` (52) | raíz, dict | `None` (`write_json`) | E/S |
| `destination(root, kind, relative)` (56) | raíz, `kind`, ruta relativa | `Path` portable | ver «Casos límite» |
| `plan(root, kind, folder)` (68) | raíz, `kind`, carpeta elegida | `(path_portable, folder_resolved, relative)` | ver «Casos límite» |
| `entries(root, identity, kind)` (83) | raíz, identidad, `kind` | lista de `{'relative', 'target'}` (registro + junctions descubiertas) | ninguno propio; no escribe |
| `linked(root, identity, kind)` (118) | raíz, identidad, `kind` | `bool` | — |
| `link(root, identity, kind, folder, replace=False)` (126) | raíz, identidad, `kind`, carpeta host absoluta, `replace` | `None` | ver «Casos límite» |
| `unlink_locked(root, identity, kind)` (180) | raíz, identidad, `kind` | `None` | `YitoError` de integridad |
| `unlink(root, identity, kind)` (197) | igual | `None` | igual, dentro de `mutation(root)` |
| `choose_folder(kind)` (202) | `kind` | `str` (ruta JSON) o `None` si se cancela | `subprocess.CalledProcessError` (`check=True`) |

`host_apps.remove` (host_apps.py:91) llama a `unlink_locked` para `data` y `cache` antes de quitar la referencia
del host.

## Script de `choose_folder` (literal, es parte del contrato)

Se ejecuta como `powershell.exe -NoLogo -NoProfile -STA -Command <script>`, con la carpeta inicial por stdin
(`input=str(host_folder(kind))`), `encoding='utf-8'`, `text=True`, `check=True`, `CREATE_NO_WINDOW`. El texto
empieza con un salto de línea tras la comilla de apertura. La cadena `MITO - Selecciona la carpeta del host para enlazar`
pasa por `native_ui.localize_script` antes de ejecutarse (ver `native_ui.md`).

```powershell

[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'MITO - Selecciona la carpeta del host para enlazar'
$dialog.ShowNewFolderButton = $false
$dialog.SelectedPath = [Console]::In.ReadToEnd()
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.SelectedPath -Compress
    }
} finally { $dialog.Dispose() }
```

Salida: JSON de una cadena (`"C:\\Users\\…"`) o vacío si se cancela (entonces `None`).

## Formatos de disco

**Registro** `core/state/host-data-links.json` (`write_json`: UTF-8, `indent=2`, `ensure_ascii=False`, salto final,
escritura atómica por temporal; ver `storage.md`). Clave de primer nivel: identidad de host_apps. Segundo nivel: `data`
o `cache`. Lista de `{relative, target}`:

```json
{
  "3f9c2a10d4e85b67": {
    "data": [
      {
        "relative": "Opera Software",
        "target": "C:\\Users\\usuario\\AppData\\Roaming\\Opera Software"
      }
    ]
  }
}
```

- `relative`: ruta relativa bajo `home/AppData/Roaming` o `Local` (separador `\` en Windows).
- `target`: `str(folder)` de la carpeta del host, ya resuelta (`Path.resolve()`).
- Los enlaces no tienen registro propio: el junction es `home/AppData/Roaming/Opera Software` → `target`.
- Staging de reemplazo: `core/staging/host-data-<uuid4 hex>` (existe solo durante `link` con `replace=True`).
- Lock: `mutation(root)` → `core/state/mutation.lock`.
- Identidad (`3f9c…`) = la que devuelve `host_apps.add` (no es objeto de este contrato).

Ejemplo de la junction: `<root>\home\AppData\Roaming\Opera Software` → `C:\Users\usuario\AppData\Roaming\Opera Software`.

## Invariantes

1. Solo `link` y `unlink` (y `host_apps.remove`) escriben; `linked`, `entries`, `candidates`, `plan`, `choose_folder`
   no escriben nada (test 1: `save` no se llama al consultar).
2. La ruta portable es `root/home/AppData/<Roaming|Local>/<relative>`. `relative` no puede ser vacía, absoluta ni
   contener `..` (validada con `storage.inside` y `portable_path`). `path` no puede ser la raíz de la categoría y su
   padre debe quedar dentro de la base.
3. La base `root/home/AppData/<kind>` debe resolverse dentro de `root`; si no: `AppData de MITO está redirigido fuera de la instancia`.
4. La carpeta del host debe ser absoluta y existente (directorio). No puede ser la propia raíz del AppData del host
   (`Selecciona una carpeta de aplicación, no todo AppData`) ni contener MITO ni estar contenida en MITO (sin enlaces circulares).
5. `relative` = ruta de la carpeta respecto al AppData del host si está dentro; si no, `folder.name`
   (p. ej. `D:\Datos Manuales` → `Datos Manuales`).
6. Ninguna ruta gestionada puede ser igual, ancestro o descendiente de otra ruta gestionada, en cualquier
   aplicación y en cualquier categoría (compara rutas portables). Única excepción idempotente: misma aplicación,
   mismo `kind`, misma ruta, junction que ya apunta a `folder` (no crea nada y no escribe duplicados).
7. `link` nunca reemplaza un junction o symlink ajeno: `No se reemplazan enlaces ajenos a MITO`.
8. Reemplazo (`replace=True`) de una carpeta local existente: se mueve a `core/staging/host-data-<uuid>`, se crea el
   junction; si tiene éxito se borra el staging (`shutil.rmtree`): el contenido local de MITO se pierde. Si falla, se
   borra el junction parcial (solo si `is_junction()`) y se restaura el staging. Test: `(path/'local')` sobrevive a un
   fallo y desaparece tras el éxito.
9. `unlink` solo elimina el junction (`os.rmdir`), tras comprobar que `resolve()` del junction coincide con `target`.
   Nunca recorre ni borra datos del host. Los datos de MITO no se restauran (el junction es el único contenido portable).
10. Si el junction ha cambiado de destino: `El enlace ha cambiado fuera de MITO: {p1}` (no se borra nada). Si en su lugar
    hay una carpeta o archivo real: `Se esperaba una junction gestionada: {p1}`.
11. `entries` devuelve también junctions no registradas que cumplen todo: son junctions en el árbol portable; su
    `relative` no está reclamada (comparación casefold) por ningún registro; alguna parte de `relative` contiene la palabra
    (stem del ejecutable en casefold); `host_folder/relative` es directorio; `destination` valida; el destino resuelto
    coincide con `host_folder/relative.resolve()` y no está dentro de la raíz de MITO. Se devuelve con `target`
    = `expected.resolve()`. No se escribe el registro.
12. Eliminar la aplicación del host (`host_apps.remove`) deshace los enlaces de ambas categorías; si un junction ha
    cambiado, la operación falla antes de quitar la referencia (test 10: la referencia permanece en el inventario).
13. `candidates`: recorre `os.walk` (top-down, `followlinks=False`) desde la base del host, nombres ordenados casefold.
    Omite junctions y symlinks. Una carpeta cuyo nombre contiene el stem del ejecutable (casefold, subcadena) es
    candidata si el ejecutable no está dentro de ella (`executable.is_relative_to(path.resolve())`); en ambos casos
    no se desciende a su subárbol. Las demás carpetas se recorren.
14. Los enlaces no dependen del profile activo: el registro se indexa solo por identidad de aplicación; el aviso de UI dice «Compartido por todos los profiles».

## Casos límite y errores

Mensajes exactos (clave = texto español; `{pN}` = valor, sin traducir):

| Clave | Cuándo |
|---|---|
| `No se pudo localizar AppData del host` | `SHGetFolderPathW` devuelve error |
| `Selecciona una carpeta existente` | `folder` relativa o no es directorio (`plan`) |
| `Selecciona una carpeta de aplicación, no todo AppData` | `folder == host_folder(kind)` |
| `Selecciona datos externos a MITO, sin enlaces circulares` | carpeta dentro de la raíz de MITO o raíz de MITO dentro de la carpeta (`plan` y `host_exposure.plan`) |
| `Destino fuera del AppData de MITO` | `relative` vacía, `path == base`, padre fuera de la base; también en `link` si la carpeta existente no resuelve dentro de `home/AppData` |
| `AppData de MITO está redirigido fuera de la instancia` | base fuera de la raíz |
| `Esta ruta ya está gestionada por una aplicación del host` | conflicto con otro registro (ver invariante 6) |
| `No se reemplazan enlaces ajenos a MITO` | el destino es junction o symlink que no es ya nuestro |
| `La carpeta {p1} ya existe y ha de ser borrada para generar el enlace` | destino existente sin `replace` (`p1` = ruta portable) |
| `El destino existente no es una carpeta` | destino existente, no directorio, con `replace=True` |
| `Copia temporal fuera de staging` | comprobación de seguridad antes de `rmtree` del staging |
| `El enlace ha cambiado fuera de MITO: {p1}` | `unlink_locked`: destino del junction distinto del registrado |
| `Se esperaba una junction gestionada: {p1}` | `unlink_locked`: hay un archivo/carpeta real en la ruta |

Otros casos:
- `CalledProcessError` de `New-Item -ItemType Junction` (PowerShell) se propaga sin traducir; `link` restaura el estado previo (test 7 lo fuerza con `side_effect`).
- `unlink_locked` sin entradas simplemente borra la clave de `identity`/`kind` y guarda el registro.
- `choose_folder` no captura errores de PowerShell (`check=True`).
- Una entrada de registro cuyo junction ya no existe se devuelve igual por `entries` y se elimina en `unlink` sin error.

## Casos de test del original

| Test | Archivo:líneas | Qué comprueba |
|---|---|---|
| `test_manual_junction_detected_without_writing_and_unlinked_safely` | test_host_data.py:12-22 | Un junction sin registro (se borra el JSON) se detecta como `linked` y `entries[0].target`; `save` no se llama al consultar; `unlink` lo quita y los datos del host siguen. |
| `test_manual_junction_reused_idempotently_and_removed_with_application` | :24-34 | Segundo `link` sobre el mismo junction no llama a `subprocess.run`; `host_apps.remove` quita el enlace y conserva los datos. |
| `test_unrelated_manual_junction_not_recognized_or_replaced` | :36-50 | Si `host_folder` cambia, el junction antiguo no cuenta como `linked`; un `link(replace=True)` a otra carpeta con la misma ruta portable es `YitoError`; `unlink` deja intactos los datos. |
| `test_picker_starts_in_category_appdata` | :52-58 | `choose_folder` para `cache` y `data`: stdin = `str(host)`, el script contiene `$dialog.SelectedPath = [Console]::In.ReadToEnd()`; salida vacía → `None`. |
| `test_search_keeps_outer_matches_and_excludes_executable_tree` | :81-86 | `candidates` = `[Opera Software, Vendor/OPERA Data]`: el árbol que contiene el ejecutable (`Programs/Opera`) y el interior de una carpeta candidata no se listan. |
| `test_real_junction_unlink_and_application_removal_preserve_host` | :88-101 | Enlaces reales para `data` y `cache`; `unlink` de `data`; `host_apps.remove` quita `cache`; datos y ejecutable del host intactos. |
| `test_replace_requires_confirmation_and_failure_restores_local_data` | :103-117 | Sin `replace` → error; con `replace` y fallo de PowerShell, los datos locales se conservan; con éxito se pierden (`local` no existe) y la eliminación de la app no toca el host. |
| `test_manual_outside_appdata_and_conflicting_application` | :119-133 | Carpeta fuera del AppData: `relative = folder.name`; otra aplicación con la misma ruta falla aunque `replace=True`; quitar la otra no quita el enlace; quitar la principal deja la carpeta. |
| `test_reject_roots_internal_paths_and_traversal` | :135-140 | `plan` rechaza la raíz del host y la raíz de MITO; `destination` rechaza `../outside`. |
| `test_changed_junction_is_not_removed` | :142-152 | Junction sustituido por carpeta real con `foreign` → `host_apps.remove` falla y no toca `foreign`; la aplicación sigue en el inventario. |
| `test_add_offers_roaming_then_local_and_accepts_independently` (UI) | test_host_data_ui.py:16-65 | `host-add` ofrece primero `Host data:` y luego `Host cache:` (`#host-data-accept`); aceptar uno no enlaza el otro; Esc en el segundo cancela; el botón `host-cache` después ofrece sin selector de carpeta; los seis botones de fila comparten `y` y caben en 150 columnas. |
| `test_manual_link_cancel_replace_unlink_and_buttons_remain` (UI) | :67-120 | Botón `host-data-<id>` con `✗` → confirmación; Esc no toca la carpeta local; aceptar dos veces enlaza (`✓`); sin registro, la etiqueta sigue `Data · ✓` tras `refresh_home`; desenlazar elimina el junction. |

## Dependencias

- `i18n.tr` (todas las claves de la tabla de errores).
- `storage`: `YitoError`, `inside`, `mutation`, `read_json`, `write_json`, `portable_path`.
- `host_apps`: `target(root, identity)`, `inventory`, `add`, `remove` (llama a `unlink_locked`).
- `native_ui.localize_script` (solo `choose_folder`).
- Usado por: `ui.py` (`offer_host_data`, ramas `host-add`, `host-cache`, `host-data`); `host_apps.py`; `host_apps_ui.py` (`HostDataConfirm`, `HostDiscoverDialog`, fuera de este contrato).
- `host_exposure.py` importa `KINDS`, `destination`, `host_folder`.

## Flujos de UI (`ui.md`, ramas de `operation`)

- `host-add` (no exige profile): `discover` → si hay hallazgos `HostDiscoverDialog`, si no `choose_file`; `add`; por cada
  `kind` en `('data','cache')` sin entradas: `candidates` y `offer_host_data` para cada carpeta.
  Logs: `Buscando aplicaciones en el menú Inicio y el escritorio del host…`, `Aplicación del host agregada. Usará el entorno del profile de MITO seleccionado.`, `Buscando carpetas del host: {p1}…` (`p1` = `data`/`cache`, sin traducir), `No se pudieron revisar los accesos del host: {p1}`.
- `offer_host_data(identity, kind, folder)`: `plan`; confirmación `{p0}: ¿crear un enlace?\nHost: {p2}\nMITO: {p4}\nCompartido por todos los profiles. La aplicación podrá modificar los datos del host. Cierra la aplicación antes de enlazar.` (`p0` = `Host data` o `Host cache`, `tr` de ambos). Si el destino existe y no es ya el enlace: `La carpeta {p1} ya existe y ha de ser borrada para generar el enlace.\nSe perderá su contenido local en MITO.` (botón `Borrar y enlazar`). Luego `link(..., replace=...)`.
- `host-cache` / `host-data` (exige profile; el target es el identificador): si ya hay `entries`, confirmación
  `¿Eliminar los enlaces de Host {p1}?\n{p3}\nLos datos del host se conservan.` (`p1` = `cache`/`data` sin traducir, `p3` = los `target` unidos por `\n`; botón `Desenlazar`) → `unlink`. Si no: `Buscando carpetas del host: {p1}…`, `candidates` + `offer_host_data` por cada una; si tras eso no hay enlace: si no hubo candidatas o el usuario acepta `¿Seleccionar otra carpeta del host manualmente?` (botón `Seleccionar`), `choose_folder(kind)` → `offer_host_data` si devuelve carpeta.
- Botones de fila (`ui.py`): `host-cache-<id>` «Cache · ✓/✗», `host-data-<id>` «Data · ✓/✗» (✓ si `linked`).

## Notas para Rust

- Crate `mito-win`. Las llamadas Win32 (`SHGetFolderPathW`, `SHGetKnownFolderPath`, creación y detección de junctions) van con `// SAFETY:` en `mito-win`. El resto del crate usa `#![forbid(unsafe_code)]`.
- Crear el junction: mantener el comando de PowerShell `New-Item -ItemType Junction -Path '<p>' -Value '<f>' -ErrorAction Stop` (mismo literal con comillas simples duplicadas) o un `DeviceIoControl(FSCTL_SET_REPARSE_POINT)` equivalente. Documentar la decisión en ADR si cambia el mecanismo.
- Desenlazar: `std::fs::remove_dir` sobre el junction (no recorre el destino). Detección de junction: atributo `FILE_ATTRIBUTE_REPARSE_POINT` + etiqueta `IO_REPARSE_TAG_MOUNT_POINT`, igual que `Path.is_junction()`.
- `thiserror` en la librería; mapear `CalledProcessError` a un error propio.
- Serialización: `serde_json` con formato idéntico (2 espacios, `\\` escapado, salto final). Escritura atómica como `storage.write_json`.
- Los nombres de error y las claves de i18n se mantienen exactos.

## Dudas

1. `host_folder` usa CSIDL 26/28 (`SHGetFolderPathW`) y el original también respeta variables heredadas; en Rust, `SHGetKnownFolderPath(FOLDERID_RoamingAppData/LocalAppData)` debe dar la misma ruta. Confirmar en un equipo con `APPDATA` redirigido.
2. `destination` valida el padre con `resolve()`; si un padre ya es un junction, `resolve()` sale de la base. ¿Debe rechazarse en lugar de aceptarse? Hoy se acepta si el resultado sigue dentro (no hay test).
3. El reemplazo borra el contenido local de MITO tras el éxito (`shutil.rmtree`) sin copia de seguridad. Confirmar que es intencionado (la UI avisa: «Se perderá su contenido local en MITO»).
4. `unlink_locked` recorre también las junctions descubiertas (no registradas): un enlace creado a mano dentro de MITO se borra al desenlazar. El test 1 lo exige; confirmar.
5. `choose_folder` y `link` capturan `CREATE_NO_WINDOW` como `subprocess.CREATE_NO_WINDOW` (solo existe en Windows): fijar el valor `0x08000000` en Rust.
6. Los `target` del registro se comparan tras `resolve()`; las rutas con mayúsculas/minúsculas distintas en NTFS se consideran iguales por `Path.resolve()`. En Rust, comparar con `normcase` como en `host_exposure.normal`.
7. Mensaje `La carpeta {p1} ya existe …` (en `link`) y `… ya existe y ha de ser borrada…` (en la UI) difieren en el punto final. Unificar en el port (decidir cuál es la clave de i18n).
8. `entries` ignora si el registro apunta a una carpeta que ya no existe (sigue devolviendo la entrada). Confirmar que la etiqueta `✓` debe depender de `linked` y no de `entries`.
9. La identidad de host_apps (`3f9c…`) y el formato de `host-applications.json` no están en este contrato; revisar en `host_apps.md`.
