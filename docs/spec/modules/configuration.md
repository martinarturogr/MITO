# configuration.py → mito-core::configuration

Origen: `core/app/mito/configuration.py` (146 líneas), `core/tests/test_configuration.py` (4 tests). Ejemplo real: `config.properties` de la raíz del original (15 líneas).

## Responsabilidad
Lee y escribe la configuración de MITO: `config.properties` global y el bloque `env` de cada perfil (`profiles/<perfil>.json`). Define las propiedades reconocidas con sus valores válidos, valida antes de escribir y preserva comentarios, líneas vacías y claves desconocidas al guardar.

## API pública del original
| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `PROPERTIES` (dict, `:12-34`) | — | `clave → (valores_permitidos \| None, valor_por_defecto)` | — |
| `PREFIXES` (tupla, `:35`) | — | `('bucket.', 'proxy_authentication_form_field.', 'pocketool.')` | — |
| `parse(text: str) -> dict[str, str]` (`:38-45`) | texto | dict clave→valor | Nunca lanza. Ignora líneas no válidas. |
| `validated(engine, pairs, profile=False) -> dict` (`:48-102`) | pares `(clave, valor)`, modo perfil | dict validado | `YitoError` |
| `class Configuration(engine, profile=None)` (`:105-146`) | motor, perfil opcional | `.values`, `.original`, `.path` | Lectura sin errores propios. |
| `Configuration.save(pairs)` (`:112-146`) | pares `(clave, valor)` | — | `YitoError`, `OSError` |

`Configuration.__init__` (`:106-110`):
- `path` = `engine.root/'profiles'/f'{identifier(profile)}.json'` si hay perfil; si no, `engine.root/'config.properties'`.
- `original` = bytes del fichero, o `None` si no existe (`:109`).
- `values` = `dict(engine.profile(profile)['env'])` en modo perfil; en modo global, `parse(original decodificado con utf-8-sig)` (`:110`).

## Formatos de disco

### `config.properties` (modo global)
- **Codificación de lectura**: UTF-8, BOM tolerado (`utf-8-sig`, `:110`, también en `i18n.py:89`).
- **Codificación de escritura**: UTF-8 sin BOM, `\n` (`:138`). Un BOM de entrada se pierde al reescribir.
- **Separación de líneas**: `str.splitlines()` (`:40`, `:126`). Además de `\r\n`, `\n` y `\r`, divide también en `\v`, `\f`, `\x1c`, `\x1d`, `\x1e`, `\x85`, ` ` y ` `. Duda: en Rust usar `str::lines()` no es equivalente.
- **Espacios**: cada línea se recorta con `strip()` (`:41`). Los valores también (`:44`, y en `validated` `:53`). No se pueden conservar espacios al principio ni al final de un valor.
- **Comentarios y vacías**: una línea vacía, o que empiece por `#` o `!` (tras el recorte), se ignora al leer (`:42`) y se conserva tal cual al guardar (`:127-129`).
- **Líneas sin `=`**: se ignoran al leer y se conservan al guardar.
- **Separador**: solo `=`, el primero de la línea (`split('=', 1)`, `:43`). El resto de la línea es el valor, con `=` incluidos (`env.TOKEN=a=b` → `env.TOKEN` = `a=b`). No se admite `:` ni espacio como separador (a diferencia de Java `.properties`).
- **Sin escapes**: no hay secuencias `\n`, `\t`, `\\`, `\uXXXX` ni comillas. Un `\` es literal. No hay continuación de línea con `\` al final. Un valor no puede tener `\r`, `\n` ni `\0` (`:56`). Una clave no puede contener `=`, `\r`, `\n` ni `\0`, ni empezar por `#` o `!` (`:54`).
- **Claves duplicadas al leer**: gana la última (`dict`, `:44`). Al guardar ver la sección de guardado.
- **Comentarios de fin de línea**: no existen. `valor # texto` guarda `# texto` como parte del valor.

Ejemplo (derivado de `config.properties` del original; el `posthog.api_key` se ha sustituido a propósito):
```
bucket.main=https://github.com/ScoopInstaller/Main
bucket.extras=https://github.com/ScoopInstaller/Extras
bucket.versions=https://github.com/ScoopInstaller/Versions
bucket.java=https://github.com/ScoopInstaller/Java
shortcut.badgeText=MITO
shortcut.badgeColor=naranja
terminal.defaultShell=cmd
mito.language=system
mito.update_url=https://api.github.com/repos/danielgube/mito/releases/latest
mcp.jobs.monitor=false
# Error reports to the public MITO project. Set false to keep only local logs.
posthog.enabled=true
posthog.host=https://eu.i.posthog.com
posthog.api_key=<OMITIDO: clave real en el original, línea 14>
pocketool.local=pocketools/local
```
Nota: el original incluye una clave real `posthog.api_key` en `config.properties` (línea 14 de la raíz). No se reproduce aquí. Duda: ¿debe seguir versionada en el repositorio Rust? Si se versiona, el valor de ejemplo debe ser un placeholder.

### Orden y reescritura (`Configuration.save`, `:112-146`)
Guardar en modo global reescribe el fichero línea a línea (`:124-134`):
1. Cada línea original sin clave (vacía, comentario, o sin `=`) se copia tal cual (`:127-129`).
2. Cada línea con clave, si su clave está en los pares a guardar pendientes, se reemplaza por `clave=valor` normalizado (`:133`). Se pierden espacios alrededor del `=`.
3. Cada línea con clave que **no** está en los pares pendientes (porque ya se usó o porque no se devolvió en `pairs`) se **elimina** (no entra en `lines`, `:131-133`). Así se borran claves: el test `test_profile_only...` y el de global quitan `env.OLD` y la línea desaparece.
4. Las claves nuevas se añaden al final, en el orden de inserción de `values` (`:134`).
5. Se escribe `'\n'.join(lines) + '\n'` (`:140`). Un fichero sin líneas produce `\n`.

**Claves desconocidas**: se conservan solo si el llamador las devuelve en `pairs`. `Configuration.values` incluye todas las claves leídas, así que el flujo normal de la UI sí las devuelve. Una clave desconocida que no se devuelva se borra.

## Propiedades reconocidas

Tabla completa de `PROPERTIES` (`configuration.py:12-34`). «Valores» = lista cerrada (`:68-70`). «Libre» = texto sin lista.

| Clave | Línea | Valores válidos | Por defecto | Validación adicional (línea en `validated`) |
|---|---|---|---|---|
| `terminal.defaultShell` | `:13` | `cmd`, `powershell`, `mito` | `cmd` | Lista cerrada. |
| `posthog.enabled` | `:14` | `true`, `false` | `false` | Lista cerrada, comparación exacta en minúsculas (`error_tracking` lo lee con `lower()`). |
| `posthog.host` | `:15` | libre | `` (vacío) | Si no está vacío, debe pasar `network.validate_url` (`:71-73`). |
| `posthog.api_key` | `:16` | libre | `` | Ninguna en `validated`. Ver `error_tracking`. |
| `mcp.jobs.monitor` | `:17` | `true`, `false` | `false` | Lista cerrada. |
| `mito.language` | `:18` | `system` + 24 idiomas (ver `i18n`) | `system` | Lista cerrada (`LANGUAGE_CHOICES`). |
| `mito.update_url` | `:19` | libre | `https://api.github.com/repos/danielgube/mito/releases/latest` | Siempre se valida con `validate_url`, incluso vacío (`:72`). |
| `enable_proxy_authentication` | `:20` | `true`, `false` | `false` | Lista cerrada. |
| `proxy_authentication_url` | `:21` | libre | `` | Si no está vacío, `validate_url` (`:71`). |
| `proxy_authentication_username_field` | `:22` | libre | `username` | Ninguna. |
| `proxy_authentication_password_field` | `:23` | libre | `password` | Ninguna. |
| `proxy_authentication_require_https` | `:24` | `true`, `false` | `true` | Lista cerrada. |
| `http_proxy` | `:25` | libre | `` | Si no está vacío, `validate_url` (`:71`). El test exige rechazar `user:password@`. |
| `https_proxy` | `:26` | libre | `` | Si no está vacío, `validate_url`. |
| `no_proxy` | `:27` | libre | `` | Ninguna. |
| `network.check_url` | `:28` | libre | `https://www.gstatic.com/generate_204` | Siempre `validate_url` (`:72`). |
| `network.check_status` | `:29` | entero decimal 100-599 | `204` | `value.isdecimal()` y `100 <= int <= 599` (`:82`). |
| `network.timeout_seconds` | `:30` | real finito 0 < x ≤ 60 | `4` | `float(value)`, `isfinite` y `0 < x <= 60` (`:74-80`). |
| `network.poll_seconds` | `:31` | libre | `20` | Ninguna en `validated`. Duda: ¿debería validarse como `timeout_seconds`? |
| `shortcut.badgeText` | `:32` | libre | `MITO` | `shortcuts.badge_text(engine, value)` (`:84-86`). |
| `shortcut.badgeColor` | `:33` | `negro`, `rojo`, `azul`, `naranja`, `rosa`, `verde`, `marron`, `morado` | `naranja` | Lista cerrada. |

Familias de claves (`PREFIXES`, `:35`):
| Patrón | Validación | Ejemplo |
|---|---|---|
| `bucket.<id>` | `<id>` pasa `identifier` (`:88`); valor no vacío (`:89-90`). Duda: el valor no se valida como URL. | `bucket.main=https://github.com/ScoopInstaller/Main` |
| `proxy_authentication_form_field.<nombre>` | `<nombre>` no vacío (`:91-92`). | `proxy_authentication_form_field.user=…` |
| `pocketool.<id>` | `<id>` pasa `identifier`; no repetido tras normalizar a minúsculas (`:93-98`); `pocketools.validate_source(valor)`; si el valor empieza por `pocketools/`, debe ser exactamente `pocketools/<id>` (`:100-101`). | `pocketool.local=pocketools/local` |
| `env.<NOMBRE>` (solo global) | Se extrae el dict con prefijo `env.` (`:61`) y se pasa a `engine.validate_env(env)`. Duda: el test exige rechazar `env.PATH`, y `env.X` junto con `env.x`; esa regla vive en `validate_env` (no leído). | `env.TOKEN=a=b` |

Claves **desconocidas** (que no están en `PROPERTIES` ni tienen prefijo): se aceptan y se guardan sin validar (`:68`, `PROPERTIES.get(key, (None,''))`).

**Modo perfil** (`profile=True`): los pares son nombres de variable de entorno directamente (sin prefijo `env.`), no se recortan (`:52-53`), y solo se valida `engine.validate_env(result)` (`:62-64`). El JSON de perfil guarda `env` como dict.

## Invariantes
1. La construcción de `Configuration` solo lee. Solo `save()` escribe.
2. `save()` valida todo antes de escribir (`:113`). Si falla, el fichero no cambia (test `test_invalid_values_leave_file_unchanged`).
3. Dentro de `mutation(engine.root)` (`:114`), compara los bytes actuales con `original`. Si difieren, lanza `La configuración cambió fuera del diálogo. Ciérralo y vuelve a abrirlo.` y no escribe (`:115-117`).
4. Guardar en modo global conserva byte a byte las líneas sin clave (comentarios, vacías, líneas sin `=`), salvo el salto de línea final, que se normaliza a `\n`.
5. Guardar en modo perfil solo cambia `env` del JSON de perfil y no toca `config.properties` (test `test_profile_only...`).
6. Tras un guardado correcto se actualizan `original` y `values` (`:145-146`).
7. Si la validación pasa, el valor guardado es el mismo que devuelve `parse` del fichero escrito (test `test_global_save...`).

## Casos límite y errores
Mensajes de `tr()` usados (claves del catálogo `español`, `i18n`):
| Caso | Mensaje |
|---|---|
| Clave vacía, con `=`, `\r`, `\n`, `\0` o que empieza por `#`/`!` (`:55`) | `Introduce una clave válida` |
| Valor con `\r`, `\n` o `\0` (`:57`) | `{p0}: el valor debe ocupar una línea` |
| Clave repetida en `pairs` (exacta, sensible a mayúsculas) (`:59`) | `Clave repetida: {p1}` |
| Valor fuera de la lista cerrada (`:70`) | `{p0}: elige uno de los valores permitidos` |
| `network.timeout_seconds` no válido (`:80`) | `{p0}: introduce un número mayor que 0 y hasta 60` |
| `network.check_status` no válido (`:83`) | `{p0}: introduce un código HTTP entre 100 y 599` |
| `bucket.` con valor vacío (`:90`) | `{p0}: introduce una fuente de catálogo` |
| `proxy_authentication_form_field.` sin nombre (`:92`) | `Falta el nombre del campo del formulario` |
| `pocketool.` repetido (`:97`) | `Pocketool repetida: {p1}` |
| `pocketools/` con carpeta distinta del nombre (`:101`) | `La carpeta local debe coincidir con el nombre del paquete.` |
| Concurrencia (`:117`) | `La configuración cambió fuera del diálogo. Ciérralo y vuelve a abrirlo.` |

Otros casos:
- Las validaciones con `identifier`, `validate_url`, `badge_text`, `validate_source` y `validate_env` lanzan sus propios `YitoError` (no leídos).
- `network.timeout_seconds` acepta lo que acepta `float()` de Python: `1_0` y `1e1` son válidos, `inf` y `nan` no. Rust `parse::<f64>` no acepta `_`. Duda: ¿se acepta `1_0`?
- `network.check_status` usa `isdecimal()`, que acepta dígitos Unicode (`٢٠٤`). Duda: ¿se mantiene ese comportamiento o se exige ASCII?
- `posthog.enabled=True` (mayúscula) es inválido al guardar, pero `error_tracking` lo acepta al leer con `lower()`. Son dos reglas distintas.
- Un fichero sin líneas guardado sin pares produce `\n`.
- En Python, `Configuration(engine, profile)` con `profile` inválido lanza `YitoError` de `identifier` en `__init__`.
- El `strip()` de la clave y del valor en modo global hace que `env.X = a ` guarde `a`.

## Casos de test del original
| Test | Qué comprueba |
|---|---|
| `test_global_save_preserves_comments_unknown_properties_and_equals` | Guardar en global conserva el comentario inicial `# My configuration` y la línea vacía, mantiene la clave desconocida `unknown.setting` (porque se devuelve en `values`), escribe `env.TOKEN=a=b` y `enable_proxy_authentication=true`, borra `env.OLD`, y `env.TOKEN` llega a `engine.global_env()`. |
| `test_profile_only_changes_env_and_overrides_global` | En modo perfil solo cambia `env` del JSON de perfil (`{'CUSTOM': 'profile'}`), `engine.environment('test')` da el valor del perfil sobre el global, y `config.properties` queda igual (`env.CUSTOM=global`). |
| `test_invalid_values_leave_file_unchanged` | Nueve entradas inválidas (`enable_proxy_authentication=maybe`, `env.PATH`, `env.X` repetido en mayúsculas/minúsculas, valor con salto de línea, `network.timeout_seconds=nan`, `network.check_status=999`, `http_proxy` con credenciales, `shortcut.badgeColor=blanco`, `terminal.defaultShell=bash`) lanzan `YitoError` y el fichero no cambia. |
| `test_concurrent_edit_is_not_overwritten` | Si el fichero cambia tras la construcción, `save` lanza `cambió fuera` y el contenido externo se conserva. |

## Dependencias
- `i18n`: `tr`, `DEFAULT_SETTING` (`'system'`), `LANGUAGE_CHOICES` (`('system', *LANGUAGES)`, `i18n.py:24`).
- `storage`: `YitoError`, `identifier`, `mutation` (`storage.py:12, :16, :73`).
- `engine` (no leído, uso observado): `engine.root` (Path), `engine.profile(nombre) -> dict` con clave `env` (`'workspace'`, `'apps'`, `'env'` en los tests), `engine.save_profile(nombre, dict)`, `engine.validate_env(dict)` (lanza `YitoError`), `engine.environment(perfil) -> dict`, `engine.global_env() -> dict`.
- `network.validate_url(url) -> ParseResult` (lanza `YitoError`; el test exige rechazar credenciales en la URL). No leído. Duda: ¿es `validate_url` quien rechaza `user:password@` o esa regla está en otro sitio?
- `shortcuts.badge_text(engine, value)` (no leído, valida el texto del badge).
- `pocketools.validate_source(value)` (no leído).
- Dependientes: `i18n.Translator.from_root` usa `configuration.parse` (`i18n.py:86-90`). Ver Notas.

## Notas para Rust
- **Parser**: implementar `parse` a mano (no crate). Crates de `.properties` existentes no son compatibles (usan `:`, escapes y continuaciones). Para el split, reproducir `splitlines()` con una función propia.
- **Escritura**: reproducir el algoritmo de `save` exactamente (conservar líneas sin clave, reemplazar, eliminar las no devueltas, añadir al final). Usar `tempfile::NamedTempFile::new_in` + `persist` (la versión Python de `save` no hace `fsync`; Duda: ¿añadirlo en Rust?).
- **Perfiles**: JSON con `serde_json` (`env` como mapa). Mantener el orden de claves con `preserve_order`.
- **Validación de URL**: depende de `network`. Definir el tipo de URL en el crate de red (`url::Url`) y reproducir las reglas `validate_url` (no leído).
- **Mensajes**: `Configuration::save` devuelve `Result<(), ConfigError>` con el texto traducido (`i18n::tr`).
- **Claves con prefijo**: modelar `PROPERTIES` como un `enum` o tabla estática `(clave, Choices, default, validator)`. Las familias `bucket.`, `proxy_authentication_form_field.`, `pocketool.`, `env.` se detectan por prefijo.
- **Estado**: `Configuration` guarda `original: Option<Vec<u8>>` y compara bytes antes de escribir (equivale al `mutation` de `storage`).
- **Duda abierta**: `network.poll_seconds` no tiene validación en Python. ¿Se mantiene así en Rust?
- **Duda abierta**: `posthog.api_key` en el `config.properties` versionado del original (ver ejemplo).
