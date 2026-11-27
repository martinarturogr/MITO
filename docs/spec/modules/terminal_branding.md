# terminal_branding.py → mito-terminal::terminal_branding (llamadas Win32 en mito-win)

Origen: `core/app/mito/terminal_branding.py` (139 líneas). No tiene archivo de test propio; los tests que lo ejercitan están en `core/tests/test_terminal.py` (2 casos, ver tabla).

## Responsabilidad
Deriva `YitoTerminal.exe` a partir del `WindowsTerminal.exe` verificado, sustituyendo sus recursos de icono (`RT_ICON` / `RT_GROUP_ICON`) por los de `core/assets/branding/mito.ico`. Reconstruye ese ejecutable solo cuando cambia el original o el icono. Lo invoca `terminal.brand_window_icons` (`terminal.py:26-27`). Es la única parte del terminal que escribe recursos PE.

## API pública del original
| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `ensure_branded_executable(root)` (`:117-139`) | raíz (`Path`) | `Path` a `YitoTerminal.exe`, o a `WindowsTerminal.exe` si no es Windows o faltan archivos | `YitoError` (ICO inválido, sin recursos de icono); `OSError` (de `ctypes.WinError`, de copia o de renombrado). |
| `_replace_icons(executable, icon)` (`:81-114`) | ruta del ejecutable temporal, ruta del `.ico` | — | `OSError` (`WinError`) en `BeginUpdateResourceW`, `UpdateResourceW` o `EndUpdateResourceW`. |
| `_names_and_languages(path)` (`:40-78`) | ruta de un PE | lista de `(nombre_o_id, idioma)` de `RT_GROUP_ICON` | `OSError` (`WinError`); `YitoError('Windows Terminal no contiene recursos de icono')` (`:75`). |
| `_ico_frames(path)` (`:23-37`) | ruta de `.ico` | lista de `(metadatos_6_bytes, imagen_bytes)` | `YitoError('El ICO de MITO está truncado')` (`:26`), `YitoError('El ICO de MITO no es válido')` (`:29`), `YitoError('El ICO de MITO contiene una imagen inválida')` (`:35`). |
| `_resource(value)` (`:19-20`) | entero o cadena | `c_void_p` (si entero) o `c_wchar_p` | — |

Las funciones con prefijo `_` son internas. Solo `ensure_branded_executable` es el contrato público (la sincronización de `Images/*.ico` está en `terminal.py:13-25`, no aquí).

## Formatos de disco
- **Entrada**: `core/tools/windows-terminal/WindowsTerminal.exe` (original, nunca se modifica, `:119`).
- **Icono**: `core/assets/branding/mito.ico` (`:120`). Se usan sus bytes tal cual.
- **Salida**: `core/tools/windows-terminal/YitoTerminal.exe` (`:121`), creado como copia y luego renombrado.
- **Temporal**: `core/tools/windows-terminal/.mito-terminal-<uuid4 hex>.exe` (`:131`). Se elimina en `finally` (`:137-138`) si quedó.
- **Marcador**: `core/tools/windows-terminal/.mito-branding.json` (`:128`), escrito con `write_json` (`storage.md`). Ejemplo, con hashes ficticios de 64 caracteres:

```json
{
  "source_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "icon_sha256": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
}
```

- **Imágenes externas** (no las escribe este módulo; ver `terminal.py:13-25`): `core/tools/windows-terminal/Images/terminal_contrast-black.ico` y `terminal_contrast-white.ico`, con los bytes de `mito.ico`.

## Derivación de `YitoTerminal.exe`

### Secuencia (`ensure_branded_executable`, `:117-139`)
1. Si `os.name != 'nt'`, o no existe el original, o no existe `mito.ico`: retorna la ruta del original sin hacer nada (`:122-123`).
2. Calcula la identidad: SHA-256 de los bytes del original y de `mito.ico` (`:124-127`).
3. Si existen `YitoTerminal.exe` y el marcador y el marcador es igual a la identidad: retorna `YitoTerminal.exe` (`:128-130`).
4. Copia el original a un temporal (`shutil.copy2`, `:133`).
5. Sustituye los recursos de icono del temporal (`_replace_icons`, `:134`).
6. Renombra el temporal a `YitoTerminal.exe` con `Path.replace` (`:135`).
7. Escribe el marcador con la identidad (`:136`).
8. En `finally`, elimina el temporal si aún existe (`:137-138`) y retorna `YitoTerminal.exe` (`:139`).

### Cuándo se reconstruye
Se reconstruye (pasos 4-7) si ocurre cualquiera de estos casos:
- Falta `YitoTerminal.exe`.
- Falta el marcador.
- El SHA-256 del original cambió (`source_sha256`).
- El SHA-256 de `mito.ico` cambió (`icon_sha256`).

No se compara la fecha ni el tamaño; solo hashes.

### Sustitución de recursos (`_replace_icons`, `:81-114`)
1. `_names_and_languages(executable)` enumera las entradas `RT_GROUP_ICON` (tipo 14) del PE y sus idiomas (`:82`, `:40-78`). Cada par `(nombre, idioma)` se procesa por separado.
2. `_ico_frames(icon)` lee las imágenes del `.ico` (`:83`).
3. `BeginUpdateResourceW(temporal, False)` (`:89`): el `False` indica que no se borran los recursos existentes.
4. Para cada `(nombre, idioma)` y cada imagen `i`:
   - Recurso `RT_ICON` (tipo 3) con id `0x7000 + i` en ese idioma (`:98`, `:101`).
   - Entrada de grupo `GRPICONDIRENTRY` de 14 bytes: `<BBBBHHIH` (`:103`), con `width`, `height`, `colorcount`, `reserved`, `planes`, `bitcount`, `tamaño` y el id nuevo.
5. Cabecera del grupo: `<HHH` = `0, 1, count` (`:104`), seguida de las entradas. Se escribe como `RT_GROUP_ICON` (tipo 14) con el mismo nombre e idioma (`:107`).
6. `EndUpdateResourceW(handle, False)` confirma los cambios (`:109`). Si falla antes, se llama con `True` para descartar (`:113-114`).

Nota: los `RT_ICON` originales no se eliminan; sus entradas quedan sin referencia desde el grupo. Lo hace así porque `bDeleteExistingResources=False` y no hay llamada de borrado.

### Lectura del ICO (`_ico_frames`, `:23-37`)
- Cabecera `<HHH` (`reserved`, `kind`, `count`): `reserved == 0`, `kind == 1` (ICO), `count >= 1`, y espacio suficiente para `count * 16` bytes de entradas (`:27-28`).
- Entrada `<BBBBHHII` de 16 bytes (`:32`): `size` = campo 6 y `offset` = campo 7. Cada imagen debe cumplir `size >= 1`, `offset >= 6 + count*16` y `offset + size <= len(datos)` (`:34`).
- Los 6 primeros campos de la entrada (`entry[:6]`) se reutilizan como metadatos del grupo.

### Lectura de recursos (`_names_and_languages`, `:40-78`)
- `LoadLibraryExW(ruta, None, LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)` (`:47`). Sin `FreeLibrary` en error de carga; `FreeLibrary` en `finally` (`:78`).
- `EnumResourceNamesW(module, RT_GROUP_ICON, callback, 0)` (`:59`). Los nombres mayores de `0xFFFF` se leen como cadena (`:55`).
- `EnumResourceLanguagesW(module, RT_GROUP_ICON, nombre, callback, 0)` (`:71`).

## Constantes Win32 e IDs de recurso
| Constante | Valor | Línea | Uso |
|---|---|---|---|
| `RT_ICON` | 3 | `:13` | Tipo de cada imagen de icono. |
| `RT_GROUP_ICON` | 14 | `:14` | Tipo del grupo que referencia las imágenes. |
| `LOAD_LIBRARY_AS_DATAFILE` | `0x00000002` | `:15` | Carga del PE como datos. |
| `LOAD_LIBRARY_AS_IMAGE_RESOURCE` | `0x00000020` | `:16` | Carga para leer recursos. |
| Base de id de imagen | `0x7000` | `:98` | Id de `RT_ICON` nuevo: `0x7000 + índice`. |
| `BeginUpdateResourceW(path, bDeleteExisting)` | `False` | `:89` | Abre el PE para actualizar. |
| `UpdateResourceW(handle, tipo, nombre, idioma, datos, tamaño)` | — | `:101`, `:107` | Escribe un recurso. |
| `EndUpdateResourceW(handle, bDiscard)` | `False` confirma, `True` descarta | `:109`, `:114` | Cierra la actualización. |

Las funciones se configuran con `argtypes` y `restype` explícitos (`:42-46`, `:85-88`). Esta configuración no es necesaria en Rust.

## Invariantes
1. El original `WindowsTerminal.exe` no se modifica nunca: solo se copia (`:133`).
2. `YitoTerminal.exe` nunca queda a medio escribir: se escribe en un temporal del mismo directorio y se renombra (`:131-135`).
3. El marcador se escribe después del ejecutable; si falla el marcador, la siguiente ejecución reconstruye (`:136`).
4. Los IDs de `RT_ICON` nuevos empiezan en `0x7000` y son consecutivos por imagen (`:98`).
5. Cada `(nombre, idioma)` del grupo original recibe el mismo conjunto de imágenes (`:95-108`).
6. El ICO de MITO se usa tal cual, sin composición ni etiquetas (`mito.ico`).
7. En Windows con `mito.ico` y `WindowsTerminal.exe` presentes, el resultado es `YitoTerminal.exe` o una excepción.

## Casos límite y errores
- No es Windows (`os.name != 'nt'`): retorna el original, sin error (`:122`).
- Falta `WindowsTerminal.exe` o `mito.ico`: retorna el original (`:122`). Nota: `brand_window_icons` ya comprueba la existencia antes (`terminal.py:17-18`), por lo que solo ocurre si `ensure_branded_executable` se llama directamente.
- `mito.ico` truncado (menos de 6 bytes), cabecera inválida o imagen fuera de rango: `YitoError` con el mensaje de `_ico_frames`.
- PE sin recursos `RT_GROUP_ICON`: `YitoError('Windows Terminal no contiene recursos de icono')` (`:75`).
- Fallo de `LoadLibraryExW` o `BeginUpdateResourceW`: `OSError` (`WinError`). El temporal se elimina en `finally`.
- Fallo de `UpdateResourceW` en mitad de la sustitución: se lanza la excepción; `EndUpdateResourceW(handle, True)` descarta los cambios (`:113-114`). No se deja un `YitoTerminal.exe` parcial.
- **`YitoTerminal.exe` en uso**: `temporary.replace(branded)` (`:135`) falla si el ejecutable está abierto (Windows no reemplaza un ejecutable en ejecución). Python lanza `OSError`, sin captura. Duda: ¿se espera que el usuario cierre el terminal antes de reconstruir?
- **Sin captura en `prepare`**: `brand_window_icons` no captura errores de `ensure_branded_executable` (`terminal.py:26-27`), así que un fallo aquí aborta `prepare` y, con ello, `launch`. Duda: ¿debe degradar a `WindowsTerminal.exe` como hace `command`?
- Un `.ico` con `count` de imágenes con `size` cero: rechazado (`:34`).

## Casos de test del original
Ninguno de los tests de `test_terminal.py` que aparecen abajo prueba directamente `ensure_branded_executable`: `test_prepare_brands_portable_terminal_window_icons` parchea la función, y `test_bootstrap_tools_brand_before_publishing_first_run_marker` ejercita el camino real (no hay `patch`) a través de `bootstrap_tools.ensure_tools`.

| Test | Qué comprueba |
|---|---|
| `test_prepare_brands_portable_terminal_window_icons` (`test_terminal.py`) | Con `ensure_branded_executable` parcheado: los dos `Images/terminal_contrast-*.ico` son idénticos a `mito.ico`; el `icon` de las entradas visibles de `settings.json` es la ruta a `mito.ico`. Verifica la sincronización de `Images`, no el PE. |
| `test_bootstrap_tools_brand_before_publishing_first_run_marker` (`test_terminal.py`) | Tras `ensure_tools`: existen `core/state/bootstrap-tools.json` y `YitoTerminal.exe`; `YitoTerminal.exe` difiere en bytes de `WindowsTerminal.exe`; `command(engine)[0]` es `YitoTerminal.exe`; `Images` iguales a `mito.ico`. Este es el único test que comprueba la derivación del PE. Duda: el test solo valida que los bytes son distintos, no que los recursos de icono sean los de MITO. |

Cobertura faltante a escribir en Rust (no existe en el original): reconstrucción con marcador igual (no reconstruye), con hash cambiado (sí reconstruye), y ICO inválido (`YitoError`).

## Dependencias
- `storage` (`YitoError`, `read_json`, `write_json`): `storage.md`.
- `terminal.py` (`brand_window_icons`, `:13-27`): quien llama, y que sincroniza `Images/`.
- `bootstrap_tools.ensure_tools` (solo en test): primera ejecución del bootstrap.
- Biblioteca estándar: `ctypes`, `hashlib`, `os`, `pathlib`, `shutil`, `struct`, `uuid`.

## Notas para Rust
- **Win32**: crate `windows` con las características de `Win32_System_LibraryLoader` (`LoadLibraryExW`, `EnumResourceNamesW`, `EnumResourceLanguagesW`, `FreeLibrary`, `BeginUpdateResourceW`, `UpdateResourceW`, `EndUpdateResourceW`), o `windows-sys` si se quiere sin wrappers. Las llamadas van en `mito-win`.
- **Errores**: `std::io::Error::last_os_error()` equivale a `ctypes.WinError(get_last_error())`. Duda: `ctypes.get_last_error()` requiere `use_last_error=True`; en Rust basta llamar a `last_os_error` justo después de la llamada.
- **Callbacks**: `EnumResourceNamesW` recibe un `extern "system" fn` con contexto; en Rust usar un puntero a un `Vec` como `LPARAM`. Los nombres mayores de `0xFFFF` son `PCWSTR`; los menores son `MAKEINTRESOURCE`.
- **ICO**: parser manual, con `struct`-equivalentes sobre `&[u8]` y `from_le_bytes`. Replicar las mismas comprobaciones de rango. El crate `ico` es alternativa, pero las reglas de error deben coincidir.
- **Hash**: crate `sha2`, SHA-256 de los bytes completos. Como el original se lee entero en memoria (`:125`), en Rust basta `std::fs::read`.
- **Temporal y renombrado**: `tempfile::NamedTempFile::new_in(dir)` o un nombre `.mito-terminal-<uuid>.exe` con `std::fs::copy`; `std::fs::rename` sustituye el destino en Windows (`MoveFileEx` con `MOVEFILE_REPLACE_EXISTING`). Duda: confirmar el comportamiento con el ejecutable en uso.
- **Marcador**: mismo formato que `write_json` (`storage.md`): `serde_json` con `PrettyFormatter` de 2 espacios.
- **Firma sugerida**: `pub fn ensure_branded_executable(root: &Path) -> Result<PathBuf, BrandingError>`, con `BrandingError` que distingue `Domain(String)` (el equivalente a `YitoError`) y `Io(std::io::Error)`.
- **Duda sobre los recursos viejos**: el original deja los `RT_ICON` previos sin referencias. Replicarlo mantiene el tamaño del ejecutable; eliminarlos (poniendo los datos a `NULL`) lo reduciría. No se decide aquí.
- **Duda sobre la firma**: el original indica que la copia queda sin firma (`ARCHITECTURE.md:502-503`). En Rust se espera el mismo resultado; no se intenta reparar la firma.
- **Duda sobre `os.name != 'nt'`**: en Rust el módulo solo se compila para Windows (`#[cfg(windows)]`); conservar el retorno del original para otros sistemas solo si el crate sirve también en otras plataformas.
