# search.py → mito-catalog::search

Origen: `core/app/mito/search.py` (40 líneas), `core/tests/test_search.py` (4 tests).

## Responsabilidad
Ordena las coincidencias del catálogo por palabras de la consulta: primero cobertura total, después aciertos en el identificador `bucket/app` y, por último, en la descripción (el orden exacto está en «Invariantes»). No hay búsqueda difusa: cada palabra coincide por igualdad (números) o por prefijo (letras).

## API pública del original
| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `tokens(text)` (search.py:7-8) | cualquier valor (se convierte con `str()`) | lista de tokens (`str`) | ninguno |
| `_tokens(text)` (search.py:11-14, `@lru_cache(maxsize=32768)`) | `str` | lista de tokens | ninguno. La caché no altera resultados |
| `rank_entries(entries, query, disabled=())` (search.py:17-40) | `entries`: iterable de `(key: str, manifest: dict, status: str)`; `query: str`; `disabled`: conjunto de nombres de bucket | lista de las mismas tuplas `(key, manifest, status)`, ordenada | `ValueError` si una `key` no contiene `/` (desempaquetado de search.py:23, ver «Casos límite») |

## Formatos de disco
Ninguno. El módulo no lee ni escribe archivos. Sus entradas llegan del snapshot de bucket (`core/buckets/<bucket>.json`, ver buckets.md) a través de `Catalog.all()` y de `compatibility()` (catalog.md).

## Invariantes
1. Tokenización (`_tokens`, search.py:13-14): `str(texto)` → `casefold()` → `unicodedata.normalize('NFKD', …)` → se eliminan los caracteres con `unicodedata.combining(c) != 0` → `re.findall(r'[a-z]+|[0-9]+', …)`. Todo lo que no sea ASCII a-z o 0-9 desaparece del token. Letras y dígitos contiguos se separan (`java21` → `java`, `21`). Los separadores `/`, `-`, `.`, espacios no producen tokens.
2. Consulta sin tokens (vacía, o sólo signos): se devuelven todas las filas cuyo bucket no está en `disabled` (search.py:19-20), ordenadas por la cadena `key` (`bucket/app`). No hay puntuación.
3. Con tokens, por cada fila (search.py:22-38):
   a. `bucket, app = key.split('/', 1)`. Si `bucket` está en `disabled`, la fila se descarta (search.py:24-25).
   b. `bucket_words = tokens(bucket)`, `app_words = tokens(app)`.
   c. `description = tokens(manifest.get('description', ''))`.
   d. `extra = tokens(str(manifest.get('version', '')) + ' ' + str(manifest.get('notes', '')))` (search.py:28).
4. Coincidencia de una palabra `w` con una lista de tokens `vs` (`hits`, search.py:29-30): si `w.isdigit()`, es `any(w == v)`; si no, es `any(v.startswith(w))`. Por tanto un número sólo coincide exactamente (`21` no coincide con `210`), y una palabra de letras coincide por prefijo.
5. `key_hits` = número de palabras de la consulta (contando repeticiones) que coinciden con `bucket_words + app_words` (search.py:31).
6. `coverage` = número de palabras de la consulta que coinciden con `bucket_words + app_words + description + extra` (search.py:32).
7. Si `coverage == 0`, la fila se descarta (search.py:33-34). Una coincidencia parcial (`0 < coverage < nº de palabras`) se conserva.
8. `bucket_hits` = número de palabras que coinciden con `bucket_words` (search.py:35).
9. `exact_app` = `1` si `app_words` es igual, como lista y en el mismo orden, a la lista de palabras de la consulta; si no, `0` (search.py:36).
10. Orden final: ascendente por la tupla `(-coverage, -key_hits, -bucket_hits, -exact_app, key)` (search.py:37 y 39). Es decir: más cobertura primero; a igualdad, más aciertos en el identificador; a igualdad, más aciertos en el bucket; a igualdad, app exacta primero; por último `key` en orden de cadena.
11. El campo `status` no interviene en el ranking; sólo se transporta en la tupla de salida.
12. Las tuplas de salida son las de entrada, sin modificar (search.py:38 guarda `(key, manifest, status)`).
13. `rank_entries` no deduplica: si entran dos filas con la misma `key`, ambas aparecen (Duda: el original no lo define explícitamente; sólo se da por hecho que las claves son únicas en el catálogo).

## Casos límite y errores
- `"python/"` y `"/python"` producen los mismos tokens que `"python"` (la barra no genera token).
- `"java 21 lts"`: `lts` no aparece en ningún campo; la fila `java/temurin21-jdk` conserva cobertura 2 (de 3) y sigue siendo la primera (test_search:23).
- `description` o `notes` con valor JSON `null` (presentes pero nulos): `dict.get` devuelve `None` y `str(None)` produce el token `none`. Con la clave ausente el resultado es cadena vacía. Duda: el original no distingue estos dos casos; conviene replicarlo tal cual o corregirlo de forma explícita, pero la decisión no está tomada en el original.
- `description` o `notes` no textuales (listas en Scoop): `str()` devuelve la representación de Python (`['a', 'b']`), y `tokens` extrae `a`, `b` de ella. Duda: si la Rust debe concatenar los elementos de la lista en lugar de usar su representación.
- `version` numérico en JSON (`21.0`): `str()` produce `21.0`, tokens `21` y `0`. Duda: `str()` de Python sobre un `float` puede diferir de otras representaciones (p. ej. `1e+20`).
- `key` sin `/`: `key.split('/', 1)` produce una lista de un elemento y el desempaquetado lanza `ValueError`. No hay captura; con una `key` así, toda la búsqueda falla. (En la rama de consulta vacía no ocurre, porque sólo se usa `row[0].split('/', 1)[0]`.)
- Caracteres no ASCII que no se descomponen con NFKD (p. ej. `ø`) se pierden en la tokenización; una consulta con ellos puede quedar sin tokens y devolver la lista ordenada de la rama vacía.
- Empates completos: la clave `key` los resuelve; sólo quedan empates si hay claves repetidas (ver invariante 13).

## Casos de test del original
| Test (archivo:línea) | Qué comprueba |
|---|---|
| `test_identifier_precedes_description` (test_search.py:16-18) | `'eclipse'` pone primero `extras/eclipse` (coincidencia en identificador) frente a `extras/anypoint-studio` (coincidencia sólo en descripción). `'java'` pone primero una clave que empieza por `java/`. |
| `test_slashes_and_multiple_words` (test_search.py:20-24) | `'python'`, `'python/'` y `'/python'` devuelven `main/python` primero (las barras se ignoran). `'java 21 lts'` y `'java21'` devuelven `java/temurin21-jdk` primero. |
| `test_disabled_buckets` (test_search.py:26-29) | Con `disabled={'main'}`, `'python'` devuelve primero `versions/python313`. Con `{'main','versions'}` no devuelve ninguna fila. |
| `test_empty_query_and_changed_description` (test_search.py:31-36) | Consulta vacía con `disabled={'main'}` devuelve las claves ordenadas, sin `main/`. `'uniquephrase'` sin coincidencias devuelve lista vacía; tras cambiar la descripción de `main/activemq` en el manifiesto, esa clave aparece primera. |

## Dependencias
- Sólo biblioteca estándar de Python: `functools.lru_cache`, `re`, `unicodedata`. No usa otros módulos de MITO.
- Consumidores (no se documentan aquí): `ui.py:1219` llama `rank_entries(self.entries, term, self.disabled_buckets)`; `__main__.py:243-247` (comando `search`) llama `rank_entries(entries, args.query)` sin `disabled` y después filtra por `status == 'Compatible'` si se pasa `--compatible`.
- Los buckets deshabilitados en la UI vienen de `core/state/catalog-filters.json` (ver buckets.md).

## Notas para Rust
- Sin crate obligatorio. Para la normalización NFKD, la crate `unicode-normalization` (`nfkd()`). Hay que comprobar que su API permite identificar marcas combinantes con el mismo criterio que `unicodedata.combining` (que devuelve la clase de combinación canónica distinta de 0). Duda: confirmar la API exacta de esa crate para ese criterio.
- `casefold()` de Python no equivale a `to_lowercase()` de Rust. Ejemplo: `ß` pasa a `ss` en `casefold` pero no en `to_lowercase`. Hace falta una tabla de casefold completa (p. ej. crate `caseless`) o aceptar la diferencia de forma explícita. Duda: decidir si la diferencia es aceptable para catálogos reales.
- Orden de cadenas: `String` en Rust ordena por bytes UTF-8, que coincide con el orden por punto de código de Python. El criterio final `key` se puede reproducir con `String::cmp`.
- La tupla de orden se puede expresar como clave de ordenación con `Reverse` para los valores que van de mayor a menor, y `key` como último elemento. Debe ser una ordenación estable para mantener el comportamiento ante claves repetidas (Duda, ver invariante 13).
- La caché `lru_cache` no afecta al resultado. En Rust basta con calcular los tokens una vez por fila; cualquier caché es opcional.
- Tipos sugeridos: una fila `(key, manifest, status)` como estructura con `manifest` en `serde_json::Value` (crate `serde_json`). Los campos `description`, `version` y `notes` se leen con los mismos valores por defecto que el original (cadena vacía si faltan, `None` tratado como `none` si son `null`). Duda: ver «Casos límite».
- No hay E/S. El módulo puede ser una función pura de `mito-catalog::search` sin dependencias de `mito-transfer`.
