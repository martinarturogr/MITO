# terminal_identity.py → mito-terminal::terminal_identity (llamadas Win32/COM en mito-win)

Origen: `core/app/mito/terminal_identity.py` (142 líneas), `core/tests/test_terminal_identity.py` (4 tests).

## Responsabilidad
Asigna a cada ventana de la instancia de Windows Terminal de MITO su identidad en la barra de tareas: `System.AppUserModel.ID` por ruta de instancia y propiedades de relanzamiento (icono `mito.ico`, comando `pythonw` del gestor y nombre visible), mediante `IPropertyStore`. Solo toca ventanas de los dos ejecutables de esa instancia y registra el resultado en el log.

## API pública del original
| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `GUID(ctypes.Structure)` (`:10-11`) | campos `a` DWORD, `b`/`c` WORD, `d` 8 bytes | — | — |
| `PropertyKey(ctypes.Structure)` (`:14-15`) | `fmtid` GUID, `pid` DWORD | — | — |
| `VariantData(ctypes.Union)` (`:18-19`) | `text` puntero, `storage` 2 × uint64 | — | — |
| `PropVariant(ctypes.Structure)` (`:22-23`) | `vt` WORD, `reserved` 3 × WORD, `data` | — | — |
| `guid(value: str)` (`:26-27`) | cadena GUID | `GUID` (de `uuid.bytes_le`) | `ValueError` de `uuid` si la cadena no es válida. |
| `identity_properties(root)` (`:30-36`) | raíz | lista de `(pid, valor)` | — |
| `terminal_windows(root)` (`:39-81`) | raíz | lista de `hwnd` | `OSError` (`WinError`) si `EnumWindows` falla. |
| `set_window_properties(hwnd, properties)` (`:84-110`) | hwnd, lista `(pid, valor)` | — | `OSError` (`WinError`) si `SHGetPropertyStoreForWindow` o `SetValue` devuelven HRESULT negativo. |
| `apply(root)` (`:113-133`) | raíz | número de ventanas (`int`), o `0` si no aplica | `OSError` (`WinError`) de COM o de las funciones anteriores. |
| `apply_safely(root)` (`:136-142`) | raíz | — | No propaga `OSError`; registra `warning`. Otros errores sí se propagan. |

## Formatos de disco
Este módulo no lee ni escribe archivos. Sí genera:

- **Entrada de log** (`:140`, `:142`): `diagnostics.write(root, 'info', 'terminal.identity', 'Taskbar identity applied', windows=<n>)` cuando tiene éxito, y `diagnostics.write(root, 'warning', 'terminal.identity', str(exc))` ante `OSError`. Consultar `diagnostics.md` para el formato del log. Duda: el registro exacto de `windows=<n>` depende del formato de `diagnostics.write`, que no está en este contrato.
- **Icono**: `core/assets/branding/mito.ico` + `,0` como índice de recurso (`:36`).
- **Comando de relanzamiento**: `pythonw.exe` de la instancia (`core/tools/python/pythonw.exe`), con `-B -I -X utf8 -m mito start` (`:33`).

## Propiedades y claves (`PKEY`)
Todas las propiedades usan el mismo formato identificador (`fmtid` `9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3`, `:87`) y un `pid` distinto. Se escriben en este orden (`:35-36`):

| Orden | `pid` | Valor (`:35-36`) | Propiedad (nombre de Windows, no en el código) |
|---|---|---|---|
| 1 | 2 | `list2cmdline([pythonw.exe, -B, -I, -X, utf8, -m, mito, start])` | `RelaunchCommand` |
| 2 | 4 | `MITO` | `RelaunchDisplayNameResource` |
| 3 | 3 | `<raíz>\core\assets\branding\mito.ico,0` | `RelaunchIconResource` |
| 4 | 5 | `MITO.Instance.<tag>` | `AppUserModel.ID` |

- `fmtid` `9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3` y `pid` 2, 3, 4, 5: las asignaciones de nombres son del modelo de propiedades de Windows; el código no las nombra (`:35`). Duda: confirmar contra la tabla de PKEYs de Windows antes de codificarlas.
- Comentario en `:34`: «Relaunch properties must precede the explicit ID». El orden es obligatorio.
- `tag` = los primeros 24 caracteres hexadecimales de SHA-256 de `str(root.resolve()).casefold()` (`:31-32`).
- Tipo de valor: `VT_LPWSTR` (`vt = 31`, `:102`). El valor se copia en `SetValue` (el comentario en `:102` indica que el buffer se copia sincrónicamente).

Ejemplo literal con raíz `C:\MITO` (la lista de propiedades es `[(2, cmd), (4, 'MITO'), (3, icono), (5, id)]`):

```
(2, "C:\\MITO\\core\\tools\\python\\pythonw.exe -B -I -X utf8 -m mito start")
(4, "MITO")
(3, "C:\\MITO\\core\\assets\\branding\\mito.ico,0")
(5, "MITO.Instance.<24 hex>")
```

## Selección de ventanas (`terminal_windows`, `:39-81`)
1. Carga `user32` y `kernel32` con `use_last_error=True` (`:41-42`).
2. Conjunto de rutas permitidas: `{<raíz>/core/tools/windows-terminal/YitoTerminal.exe, <raíz>/core/tools/windows-terminal/WindowsTerminal.exe}` en minúsculas (`:56-57`). Compara con `casefold()`.
3. Para cada ventana top-level (`EnumWindows`, `:79`):
   - La clase debe ser exactamente `CASCADIA_HOSTING_WINDOW_CLASS` (`:63`). El título no se consulta.
   - `GetWindowThreadProcessId` obtiene el PID (`:66`).
   - `OpenProcess(0x1000, False, pid)` con `PROCESS_QUERY_LIMITED_INFORMATION` (`:67`). Si falla, se omite.
   - `QueryFullProcessImageNameW(proceso, 0, buffer, tamaño)` obtiene la ruta completa (`:73`). Si está en el conjunto, se añade el `hwnd`.
   - `CloseHandle` se llama siempre en `finally` (`:76`).
4. Devuelve la lista de `hwnd` coincidentes (`:81`).

## Invariantes
1. `identity_properties` devuelve siempre exactamente 4 pares, en el orden de la tabla anterior (test 1).
2. El `AppUserModel.ID` depende solo de la ruta resuelta de la instancia, sin distinguir mayúsculas. `C:/mito` y `c:/mito` producen el mismo ID; `C:/Yito` y `C:/other-mito` producen IDs distintos (test 1).
3. El icono es siempre `mito.ico`; nunca el icono de otra aplicación (`:36`, test 1).
4. `terminal_windows` nunca devuelve ventanas de otra instancia, de otra clase de ventana ni el `WindowsTerminal.exe` del host (test 2).
5. Cada `IPropertyStore` obtenido se libera con `Release` una vez (`:110`, test 3).
6. Un fallo de identidad no aborta el gestor: `apply_safely` solo registra `warning` (test 4).
7. Se llama `CoUninitialize` solo si este módulo llamó a `CoInitializeEx` con éxito (`:132-133`).

## Casos límite y errores
- `os.name != 'nt'` o falta `mito.ico`: `apply` retorna `0` sin tocar nada (`:115-116`). `apply_safely` igualmente registra `info` con `windows=0`.
- **`RPC_E_CHANGED_MODE`** (`-2147417850`, `:123`): el hilo ya tiene COM inicializado con otro modo. Se continúa sin `CoUninitialize` (`:132`).
- Otros HRESULT negativos de `CoInitializeEx` (`:124`): `WinError` (`OSError`), y `apply_safely` lo registra como `warning`.
- Ventana sin propiedad store (HRESULT negativo en `SHGetPropertyStoreForWindow`): `OSError`, registrado como `warning`. Las ventanas anteriores ya tienen sus propiedades escritas; no hay rollback (`set_window_properties` procesa ventana a ventana en `:128-129`).
- `OpenProcess` falla (proceso de otro usuario o ya terminado): la ventana se omite sin error (`:68-69`).
- `QueryFullProcessImageNameW` falla: la ventana no se incluye, sin error (`:73`).
- `EnumWindows` falla: `WinError` (`:79-80`).
- **Ventana aún no creada**: `apply` se llama desde el hijo lanzado en la pestaña (docstring `:114`), así que la ventana debe existir. Si Terminal aún no la ha creado, `terminal_windows` devuelve lista vacía y se registra `windows=0`. Duda: ¿se reintenta o se acepta el `0`?
- **Varias ventanas de la misma instancia**: todas reciben las mismas propiedades (`:128-129`); no hay distinción por pestaña.
- `apply_safely` solo captura `OSError`. Un `TypeError` o `ValueError` (por ejemplo, de `uuid`) sí se propaga.
- El valor de `hwnd` se pasa como `W.HWND`; un valor nulo o inválido produce HRESULT negativo.

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_taskbar_identity_is_per_instance_and_uses_only_mito_icon` | `identity_properties(C:/MITO)` devuelve los pid `[2, 4, 3, 5]`; el icono termina en `core\assets\branding\mito.ico,0`; el nombre es `MITO`; el comando contiene `pythonw.exe` y termina en `-m mito start`, sin `pocketool`; el ID es igual para `c:/mito` y distinto para `C:/Yito` y `C:/other-mito`. |
| `test_enumeration_excludes_other_instances_and_unrelated_windows` | Con `user32`/`kernel32` simulados, de cinco ventanas solo se devuelven `[1, 4]`: la 2 es de otra instalación, la 3 es el `WindowsTerminal.exe` del host, y la 5 tiene otra clase. Se cierran 4 handles (las ventanas 1 a 4 se abren; la 5 se descarta antes). |
| `test_property_store_receives_strings_and_releases_interface` | Tamaños de estructuras: `GUID` 16, `PropertyKey` 20, `PropVariant` 24 bytes. Con una tabla de vtable simulada, `SetValue` recibe las cuatro propiedades en orden, con `vt` 31 y cadenas correctas; `Release` se llama una vez. |
| `test_failure_to_set_identity_does_not_abort_manager` | Si `apply` lanza `OSError('denied')`, `apply_safely` no propaga y registra `('warning', 'terminal.identity')` mediante `diagnostics.write`. |

## Dependencias
- `diagnostics.write(root, nivel, componente, mensaje, **campos)` (`diagnostics.md`): log `terminal.identity`.
- `terminal.py` (`command`, `child_command`): el mismo `pythonw` y la misma raíz. El `relaunch` apunta al gestor, no al terminal.
- `core/tools/python/pythonw.exe` (ejecutable de Python privado).
- Biblioteca estándar: `ctypes`, `ctypes.wintypes`, `hashlib`, `os`, `subprocess`, `uuid`.
- Win32: `user32` (`EnumWindows`, `GetClassNameW`, `GetWindowThreadProcessId`), `kernel32` (`OpenProcess`, `QueryFullProcessImageNameW`, `CloseHandle`), `shell32` (`SHGetPropertyStoreForWindow`), `ole32` (`CoInitializeEx`, `CoUninitialize`).

## Notas para Rust
- **Crate**: `windows` con las características `Win32_Foundation`, `Win32_UI_WindowsAndMessaging` (`EnumWindows`, `GetClassNameW`, `GetWindowThreadProcessId`), `Win32_System_Threading` (`OpenProcess`, `QueryFullProcessImageNameW`), `Win32_System_Com` (`CoInitializeEx`, `CoUninitialize`), `Win32_UI_Shell` (`SHGetPropertyStoreForWindow`) y `Win32_UI_Shell_PropertiesSystem` (`IPropertyStore`, `PROPERTYKEY`, `PROPVARIANT`). Las llamadas van en `mito-win`. Los nombres exactos de los módulos dependen de la versión de `windows`; confirmarlos al implementar.
- **`IPropertyStore`**: en Rust usar `IPropertyStore::SetValue(&PROPERTYKEY, &PROPVARIANT)` y `Release` mediante `Drop` del `IPropertyStore` (el `windows` crate lo hace). Esto sustituye la vtable manual de `:94-97`.
- **PROPVARIANT con `VT_LPWSTR`**: se puede construir con `InitPropVariantFromStringAsVector`/`PropVariantFromStringAlloc` del crate `windows`, lo que evita apuntar a un buffer local. Duda: el original apunta a un buffer temporal que sobrevive a la llamada (`:98-103`); en Rust se elige la variante con copia propia.
- **GUID**: `uuid::Uuid::parse_str` y `to_bytes_le()` (equivale a `bytes_le`, `:27`). Para `fmtid` basta `windows::core::GUID::from_u128` o similar.
- **Ruta y tag**: `std::fs::canonicalize` devuelve `\\?\` en Windows; usar `dunce::canonicalize` antes de `to_lowercase()`. SHA-256 con `sha2`, primeros 24 hex. Duda: `str(root.resolve())` de Python no es igual a `canonicalize` cuando la ruta contiene enlaces; confirmar.
- **Comparación de ruta**: el original usa `casefold()`; en Rust `to_lowercase()`. Duda: diferencias con caracteres no ASCII en rutas de usuario.
- **Firmas sugeridas**:
  - `pub fn identity_properties(root: &Path) -> Vec<(u32, String)>`
  - `pub fn terminal_windows(root: &Path) -> Result<Vec<HWND>, IdentityError>`
  - `pub fn set_window_properties(hwnd: HWND, props: &[(u32, String)]) -> Result<(), IdentityError>`
  - `pub fn apply(root: &Path) -> Result<usize, IdentityError>`
  - `pub fn apply_safely(root: &Path)` (registra `info` o `warning` con `diagnostics`; no propaga E/S).
- **Errores**: `IdentityError` con variante `Io(std::io::Error)` (el equivalente de `OSError`/`WinError`) y `Domain(String)` si hace falta. `apply_safely` solo debe capturar la variante de E/S, igual que el original.
- **COM**: `CoInitializeEx(COINIT_APARTMENTTHREADED)` con tratamiento de `RPC_E_CHANGED_MODE` (`0x80010106`); la guarda de `CoUninitialize` debe replicarse.
- **Duda sobre el momento de `apply`**: el docstring dice que se llama una vez por hijo tras crear la ventana. No se ve el caller en estos archivos. Duda: ¿dónde se llama `apply_safely`? Sin ese punto no se puede fijar cuándo se mide `windows=n`.
- **Duda sobre `windows=0`**: si el hijo se ejecuta antes de que Terminal cree la ventana, el resultado es `0` y no hay reintento. Confirmar si es aceptable.
- **Duda sobre los nombres de los PID**: `2`, `3`, `4` y `5` se corresponden con las propiedades de relanzamiento e ID en la tabla de Windows, pero el original no los nombra. Confirmar antes de codificar constantes.
