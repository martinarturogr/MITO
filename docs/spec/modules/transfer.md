# transfer.py → mito_transfer::transfer

Origen: `core/app/mito/transfer.py` (402 líneas), `core/tests/test_transfer.py` (10 tests; 342 líneas).

Nota de nombre: el crate es `mito-transfer` (guion) y en Rust se referencia como `mito_transfer`.

## Responsabilidad

Descargar a disco un archivo por HTTP/HTTPS con redirecciones, reintentos, reanudación y verificación de hash, y extraer ZIP con validación previa de rutas, tamaños y enlaces. Reporta progreso y cancelación a través de `mito.operation` (ver `operation.md`).

## API pública del original

| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `MAX_DOWNLOAD` (línea 19) | constante | `4 * 1024**3` = 4 294 967 296 bytes | n/a |
| `MAX_EXPANDED` (línea 20) | constante | `12 * 1024**3` = 12 884 901 888 bytes | n/a |
| `download_filename(url)` (23) | `url: str` | `str` (nombre local) | `YitoError` si nombre vacío, `.`/`..`, contiene `/` o `\`, o falla `portable_path` |
| `WebRedirect` (34) | subclase de `HTTPRedirectHandler` | handler de `urllib` | `YitoError` si el destino de redirección no es http/https |
| `web_page(url, content_type, head)` (44) | URL pedida, `Content-Type`, primeros bytes | `bool` | nunca |
| `DownloadIntegrityError(message, url='', size=None, host='', portal=False)` (52) | subclase de `YitoError` | excepción con atributos `url`, `size`, `host`, `portal` | n/a |
| `public_url(url)` (62) | URL | URL sin query ni fragmento | nunca |
| `web_page_error(final_url, url='', size=None)` (68) | URL final, URL pedida, tamaño | `DownloadIntegrityError` con `portal=True` | n/a |
| `hash_verifier(expected)` (74) | `str`, `None` o `'skip'` | `(hasher | None, digest_hex | None)` | `YitoError` por tipo no texto, algoritmo no soportado o formato incorrecto |
| `download_hashes(urls, value)` (96) | lista de URL, `value` (str, lista, null) | lista de longitud `len(urls)` con `None` o el valor original | `YitoError` si la cantidad de hashes no coincide con las URL o si un hash es inválido |
| `_build_opener(root, *, insecure=False)` (112) | raíz o `None`, flag TLS | `OpenerDirector` | (privada) |
| `_content_range(headers)` (122) | cabeceras de respuesta | `(start: int, total: int | None)` o `None` | nunca |
| `_validator(headers)` (130) | cabeceras | ETag fuerte, o `Last-Modified`, o `None` | nunca |
| `_Stream(opener, request, timeout=60)` (140) | opener, request | objeto con `next(check)` y `close()` | (privada) |
| `download(url, destination, expected=None, *, root=None, max_bytes=MAX_DOWNLOAD, attempts=5, delay=2)` (193) | URL, ruta destino, hash opcional | `None`; el archivo queda en `destination` | `YitoError`, `DownloadIntegrityError`, `operation.Cancelled`, `urllib.error.HTTPError` (4xx permanente), `urllib.error.URLError`/`OSError` tras agotar reintentos |
| `extract_zip(archive, destination)` (380) | ruta ZIP, carpeta destino | `None` | `YitoError` (límites, duplicados, enlaces, traversal); `NotImplementedError` de `zipfile` (método no soportado, p. ej. Deflate64) se propaga |

### Formatos de hash aceptados

- Prefijo explícito `algoritmo:digest` (línea 82-83): el algoritmo se pasa a `hashlib.new(algorithm, usedforsecurity=False)`. El digest debe tener exactamente `digest_size * 2` caracteres hexadecimales en minúscula tras `strip().lower()` (línea 91).
- Sin prefijo (línea 86): longitud 32 → `md5`, 40 → `sha1`, 64 → `sha256`, 128 → `sha512`. Cualquier otra longitud produce algoritmo `''` (ver Duda 2).

Ejemplos literales de los tests:

```
"md5:" + "a"*32                    # válido (test línea 150)
"0"*64                             # hash sin prefijo, sha256 inferido (test línea 43)
"md5:invalid"                      # Formato de hash incorrecto (test línea 155)
"a"*64 dentro de lista de 2 URL    # cantidad de hashes no coincide (test línea 153)
["skip", "md5:" + "a"*32]          # hash por URL; "skip" = sin verificar esa URL
```

Valores que equivalen a "sin hash" (líneas 75-78, 97-98): `None`, `''`, cadena que tras `strip().lower()` es `'skip'`, o cadena solo con espacios (test línea 151 `hash_verifier('   ') == (None, None)`). Cualquier valor no textual distinto de `None` (p. ej. número, lista dentro de `hash_verifier`) lanza `YitoError('El hash debe ser texto, null o skip')` (línea 80).

### Nombre local derivado de la URL (`download_filename`, líneas 23-31)

- Si el fragmento empieza por `/`, el nombre es el fragmento sin ese `/` (alias Scoop `#/nombre`). En otro caso es el último segmento de la ruta, sin query.
- Se aplica `urllib.parse.unquote` después de separar. Ejemplo del test (test_engine.py:592): `https://example.org/Tool%2EEXE?x=1` → `Tool.EXE`.
- Se valida con `portable_path(name)` (storage.py:24) y se rechaza vacío, `.`, `..`, o con `/` o `\` tras decodificar.
- Ejemplos rechazados (test_engine.py:594-598): `https://example.org/` (sin nombre), `#/../escape`, `#/dir/file`, `#/C%3Aevil`, `#/NUL`, `#/a%5Cb`, `#/bad%3Fname`, `#/..`, `#/file.`.

## Formatos de disco

- **Archivo destino**: bytes crudos de la respuesta, sin cabecera ni archivo temporal. Se escribe directamente en `destination` (`destination.parent` se crea, línea 205). Modo `'wb'` en una descarga nueva y `'ab'` al reanudar.
- **Cabeceras HTTP enviadas**: `User-Agent: MITO/0.1` (línea 248). Al reanudar: `Range: bytes=<recibidos>-` (línea 250) y, si hay validador, `If-Range: <ETag fuerte o Last-Modified>` (línea 252). Ejemplo del test (línea 300): `{'range': 'bytes=8192-', 'if-range': '"v1"'}` tras recibir 8192 bytes de 16384.
- **Diagnóstico**: si `root` no es `None` y el hash no coincide, `diagnostics.write(root, 'warning', 'download.hash', 'Hash mismatch', host=..., bytes=..., content_type=..., algorithm=...)` (línea 369-372). Test línea 129-131: el log contiene `download.hash` y no contiene el token de la query.
- **Trazas de consola (texto fijo en español, sin i18n, líneas 209-214, 226, 230, 271, 274, 319, 332, 335, 347, 364)**: se imprimen con `print(..., flush=True)` y además se envían con `operation.note`. Formatos:
  - `Aviso: certificado TLS no verificable; reintentando sin verificarlo (integridad garantizada por el hash declarado)`
  - `Aviso: certificado TLS no verificable; reintentando sin verificarlo ni comprobar la integridad (descarga sin hash)`
  - `Aviso: reanudando la descarga desde {received} bytes`
  - `Aviso: el servidor no admite reanudar; la descarga empieza de nuevo`
  - `Aviso: el servidor rechazó reanudar; la descarga empieza de nuevo`
  - `Aviso: HTTP {code}; reintentando la descarga ({n}/{attempts})`
  - `Aviso: error de red ({reason}); reintentando la descarga ({n}/{attempts})`
  - `Aviso: conexión interrumpida ({exc}); reintentando la descarga ({n}/{attempts})`
  - `Aviso: el hash no coincide tras reanudar; la descarga empieza de nuevo`
- **Mensaje de integridad (línea 374-375)**, con `{p1}` = URL pedida sin query ni fragmento, `{p2}` = bytes recibidos, `{p3}` = host final:
  `Hash incorrecto en {p1}: se recibieron {p2} bytes desde {p3}. Un proxy o antivirus puede estar alterando la descarga; prueba desde otra red o avisa a soporte con esta URL. Se ha descartado la descarga.` (texto literal de la línea 374; `{p1}` URL pública, `{p2}` bytes, `{p3}` host).
- **Mensaje de portal (línea 70-71)**: `La red devolvió una página web en lugar del archivo ({p1}); puede ser el portal de autenticación o un bloqueo del proxy. Pulsa INTERNET y reintenta.` con `{p1}` = host final.
- **ZIP**: formato estándar; la extracción crea la estructura bajo `destination` (líneas 395-402).

### Detección de página web interceptada (`web_page`, líneas 44-49)

- Si la ruta pedida termina en `.htm`, `.html` o `.xhtml` → no es página interceptada.
- Si, tras quitar BOM UTF-8 y espacios/saltos iniciales y pasar a minúsculas, los primeros bytes empiezan por `<!doctype html`, `<html`, `<head`, `<body`, `<script`, `<meta` o `<title` → interceptada.
- O si `Content-Type` es `text/html` y el contenido empieza por `<`.

## Invariantes

1. Ninguna descarga fallida o cancelada deja un archivo parcial en `destination`: todas las ramas de error ejecutan `destination.unlink(missing_ok=True)` antes de propagar (líneas 234, 240, 311, 326, 337, 349, 352, 359, 366, 368). Solo la respuesta de reintento con continuación conserva el archivo para reanudar.
2. Solo `http` y `https` en la URL inicial (línea 202-203) y en cada redirección (`WebRedirect`, línea 36-37). Cualquier otro esquema (p. ej. `file:`) lanza `YitoError`.
3. Un hash inválido se rechaza antes de crear carpetas y antes de tocar la red: `hash_verifier(expected)` se evalúa en la línea 204 y la carpeta se crea en la línea 205.
4. La reanudación solo se intenta si `received > 0`, el archivo en disco mide exactamente `received` y hay validador (ETag fuerte o Last-Modified) o hash declarado (líneas 246-247). Sin ninguna de las dos cosas, un corte reinicia desde cero.
5. Respuesta `206` solo se acepta si la descarga era reanudable y el `Content-Range` empieza exactamente en `received` (líneas 262-267). En otro caso se lanza `HTTPException` y el siguiente intento empieza desde cero. Nunca se concatena un fragmento no verificado.
6. Respuesta `200` a una petición con `Range` reinicia el archivo (`mode='wb'`, `received=0`, hasher nuevo; líneas 273-285).
7. La detección de página web se ejecuta antes de la verificación de hash y también sin hash (línea 358). Un artefacto sin hash nunca publica una página de portal.
8. Los mensajes de error contienen la URL sin query ni fragmento (`public_url`, línea 62-65), nunca el token firmado de la redirección.
9. El hash se calcula sobre todos los bytes recibidos, incluidos los de intentos anteriores reanudados, porque el hasher persiste entre intentos (líneas 220-221, 299-300).
10. Un hash no coincidente tras reanudar provoca una nueva descarga completa dentro del mismo presupuesto de intentos (líneas 362-367), salvo que no queden intentos: entonces lanza `DownloadIntegrityError`.
11. `Cancelled` se propaga sin reintento, y deja el destino sin archivo (líneas 310-312). La cancelación se comprueba en cada `stream.next(check)` (cada 0.25 s, ver límites) y también durante la espera de reintento (`operation.wait`).
12. `extract_zip` valida todas las entradas antes de escribir ninguna: límite de entradas y tamaño (línea 383), duplicados sin mayúsculas y enlaces (líneas 385-394) en el primer bucle; la escritura va en un segundo bucle (líneas 395-402).
13. Cada ruta de entrada ZIP pasa por `inside(destination, entry.filename)` (storage.py:35), que aplica `portable_path` y comprueba que la ruta resuelta queda dentro de la raíz.

## Constantes y límites numéricos

| Valor | Dónde | Valor |
|---|---|---|
| `MAX_DOWNLOAD` | transfer.py:19 | 4 GiB (4 294 967 296 bytes) |
| `MAX_EXPANDED` | transfer.py:20 | 12 GiB (12 884 901 888 bytes) |
| `attempts` por defecto | transfer.py:194 | 5 (mínimo 1, línea 215) |
| `delay` por defecto | transfer.py:194 | 2 s (`max(0, delay)`, línea 218) |
| Espera entre reintentos | transfer.py:236 | se duplica tras cada reintento: 2, 4, 8, 16 s con valores por defecto |
| Presupuesto renovado | transfer.py:344-345 | si el intento recibió ≥ 1 MiB (1 048 576 bytes), vuelven `attempts` y `delay` |
| Timeout de socket | transfer.py:147 | 60 s, pasado a `opener.open(request, timeout=60)` |
| Cola de eventos | transfer.py:148 | `Queue(maxsize=16)` |
| Espera de put/get de la cola | transfer.py:156, 182 | 0.25 s (sondeo de cancelación) |
| Bloque de lectura | transfer.py:171 | `read1(256 * 1024)` = 262 144 bytes |
| Ventana de sniff de contenido | transfer.py:297 | primeros 512 bytes |
| Intervalo de reporte de progreso | transfer.py:302 | ≥ 0.25 s entre `operation.transfer` |
| Límite de entradas en ZIP | transfer.py:383 | > 150 000 entradas → `YitoError` (mismo límite en TAR, archives.py:40, y 7z, archives.py:97) |
| Límite de tamaño en ZIP | transfer.py:383 | suma de `file_size` declarados > `MAX_EXPANDED` → error |

El test de cancelación anuncia `Content-Length: 100000` (test línea 325); ese valor es del servidor de prueba y no es un límite del código.

## Casos límite y errores

- **Esquema no permitido en la URL inicial**: `YitoError('Las descargas requieren HTTP o HTTPS')` (línea 203), antes de `mkdir`.
- **Hash inválido**: error antes de la red (línea 204).
- **Página de portal**: `DownloadIntegrityError(portal=True)` con host final, aunque no haya hash (línea 358-360).
- **HTTP 5xx, 408, 429**: se reintentan (línea 323) mientras queden intentos. Otros 4xx se propagan como `urllib.error.HTTPError` sin reintento (línea 326-327). Ojo: no es `YitoError`.
- **HTTP 416** con descarga reanudable y quedan intentos: reinicia desde cero (línea 316-320). Sin intentos o sin reanudación: cae en la regla 4xx y se propaga.
- **Error TLS de verificación** (`ssl.SSLCertVerificationError` dentro de `URLError`, línea 330): se reintenta una vez con contexto sin verificar. El flag `insecure` queda activo en intentos siguientes. El reintento TLS no comprueba `remaining` (ver Duda 6).
- **Cortes de red, timeouts, lecturas incompletas** (`OSError`, `HTTPException`, incluida `IncompleteRead` de la línea 307): se reintentan conservando lo recibido.
- **Descarga que supera `max_bytes`** (línea 292-295): con el valor por defecto mensaje `Descarga excede el límite de 4 GiB`; con otro valor `Descarga excede el límite de {max_bytes} bytes`. Se trata como `YitoError` y el archivo se borra (rama `BaseException`).
- **Cancelación durante lectura bloqueada** (test línea 317-342): `Cancelled` en menos de 15 s; el socket no bloquea la cancelación porque la lectura ocurre en un hilo aparte.
- **Cancelación durante espera de reintento**: `operation.wait` lanza `Cancelled` y se borra el destino (línea 233-235).
- **`ZIP` con entradas duplicadas que solo difieren en mayúsculas**: `YitoError('Entrada duplicada: ...')` (línea 389-390).
- **`ZIP` con enlace simbólico** (modo Unix `S_IFLNK` en `external_attr >> 16`): `YitoError('Los enlaces del archivo no están permitidos')` (línea 393-394).
- **Traversal, rutas reservadas, ADS, rutas absolutas o con `:`**: los rechaza `inside` → `portable_path` (storage.py:24-32). Mensajes: `Ruta no portable: ...`, `Ruta reservada: ...`, `Ruta fuera de su raíz: ...`.
- **Hash no coincide tras la descarga completa**: `DownloadIntegrityError(size=total, host=...)` (línea 374-376) y diagnóstico si `root` existe.

## Casos de test del original

| Test (`core/tests/test_transfer.py`) | Qué comprueba |
|---|---|
| `test_http_download_redirect_and_hash_verification` (16-52) | Redirección 302 `/redirect`→`/payload` en HTTP; verificación con `md5:`, `sha1:`, `sha256:`, `sha512:`; sin hash con `None`, `''` y `'skip'`; hash `'0'*64` sin prefijo y `md5:` de ceros fallan con `YitoError` y no dejan archivo; `max_bytes=len-1` falla y `max_bytes=len` pasa; al final `Catalog.compatibility` devuelve `Compatible` con hash `'0'*64` (dependencia de `catalog.py`) |
| `test_intercepted_portal_page_is_reported_instead_of_a_hash_or_json_error` (60-95) | Servidor de portal redirige 303 a `/fgtauth?magic=1`, responde HTML 200. Con hash `'0'*64`, con `None` y con `/bucket/app.json` lanza `YitoError` cuyo mensaje contiene `página web` y `127.0.0.1`, sin archivo. Una URL `.html` (`/fgtauth/index.html`) sí se guarda |
| `test_hash_mismatch_names_the_url_size_and_host_without_signed_query` (97-135) | Respuesta con cuerpo XML con BOM (`<BlockList>`, simula un proxy que sustituye el artefacto) tras redirección 302 a `/asset?sig=secret-token`; `DownloadIntegrityError` con `Hash incorrecto`, URL sin query, tamaño en el mensaje, sin `token=abc` ni `secret-token` en mensaje ni en `*.log`; log contiene `download.hash` |
| `test_web_page_detection` (137-143) | `web_page` verdadero para BOM+`<!doctype html>` en `.zip` y para `<div>` con `text/html`; falso para JSON con `text/html`, para `PK\x03\x04` con `text/html` y para XML con `application/xml` |
| `test_optional_and_mixed_hashes` (145-160) | `download_hashes` con `None`, `'skip'` y lista mixta `['skip', 'md5:'+'a'*32]`; `hash_verifier('   ')` = `(None, None)`; lista de longitud 1 para 2 URL falla; `md5:invalid` falla; `Catalog.compatibility` acepta `None`, `''`, `'skip'` y lista mixta |
| `test_redirect_allows_http_and_rejects_other_schemes` (162-171) | `WebRedirect` acepta redirección de `https://` a `http://`; rechaza `file:///app.zip`; `download('file:///app.zip', ...)` falla con `YitoError` |
| `test_download_retries_without_certificate_verification_when_hash_declared` (173-206) | Servidor HTTPS con `fixtures/tls_cert.pem` (certificado no verificable). Con hash SHA-256 descarga bien; sin hash también descarga; con hash `'0'*64` falla y no deja archivo |
| `test_download_retries_transient_errors_but_not_permanent` (208-256) | `/gateway` responde 504 dos veces y luego 200: se descarga, 3 peticiones. `/missing` responde 404: `urllib.error.HTTPError`, 1 petición, sin archivo. `/down` responde 504 siempre con `attempts=3`: `HTTPError` tras 3 peticiones, sin archivo |
| `test_interrupted_download_resumes_with_range_from_received_bytes` (269-315) | Servidor corta a la mitad la primera respuesta. Con ETag `"v1"` sin hash: reanuda con `Range: bytes=8192-` e `If-Range: "v1"`. Con hash SHA-256 sin validador: reanuda solo con `Range`. Sin validador ni hash: reinicia (`[None, None]`). Servidor que ignora `Range` (200): reinicia, no concatena (2 peticiones) |
| `test_cancellation_interrupts_a_stalled_read_and_discards_the_partial_file` (317-342) | Servidor envía 1000 de 100 000 bytes y se bloquea. Un hilo cancela cuando hay progreso; `Cancelled` en menos de 15 s y sin archivo (usa `Operation` y `monitored`) |

Helper de test: `serve(handler)` (258-267) levanta un `ThreadingHTTPServer` en `127.0.0.1:0` y lo cierra al terminar.

Gaps del original (comportamientos sin test en `test_transfer.py`; el contrato Rust debe cubrirlos igualmente):

- `download_filename` solo aparece en `test_engine.py:591-598`, no en test_transfer.
- `extract_zip` solo se prueba de forma directa en `test_engine.py:93-101` (traversal) y de forma indirecta en `test_activation.py` (ver `archives.md`).
- Sin test: reset del presupuesto al recibir ≥ 1 MiB (línea 344-345); espera exponencial; HTTP 408/429 reintentados; HTTP 416; `Content-Range` malformado; `max_bytes` por defecto de 4 GiB; flag `insecure` persistente entre intentos; redirección real HTTPS→HTTP (solo test unitario de `WebRedirect`); `web_page` sobre `.htm`; límites `150000` entradas y `MAX_EXPANDED` en `extract_zip`.

## Dependencias

- `mito.i18n.tr` (líneas 1, 30, 37, 70, 80, 90, 92, 101, 203, 294, 374): mensajes de error traducibles. Las claves son el texto español.
- `mito.storage`: `YitoError` (storage.py:12), `inside` (35), `portable_path` (24).
- `mito.network.opener` (importación perezosa, línea 118). Ver `network.md`.
- `mito.operation` (importación perezosa, línea 201): `current`, `note`, `transfer`, `wait`, `Cancelled`, y el monitor devuelto por `current()` con `.cancelled` y `.check()`. Ver `operation.md`.
- `mito.diagnostics.write` (importación perezosa, línea 370). Solo si `root` no es `None`.
- Consumidores (fuera del alcance de este contrato): `engine.py:333`, `catalog.py:85,165,169`, `bootstrap_tools.py:32`, `bucket_resources.py:53`, `self_update.py:246,254`, `versions.py:44` (`attempts=2, delay=1`), `pocketools.py:122`, `burn.py:13,124,179` (`hash_verifier`, `MAX_EXPANDED`), `hooks.py:165`, `engine.py:338-339` (`download_filename`), `catalog.py:227,231` (`download_hashes`, `download_filename`).
- `archives.py` importa `MAX_EXPANDED` y `extract_zip` (ver `archives.md`).
- Tests: `catalog.Catalog.compatibility` (usado en 2 tests del original).

## Notas para Rust

### Crates sugeridas

- HTTP: `reqwest` con feature `blocking` y `native-tls` (SChannel en Windows, usa el almacén de certificados del sistema como Python). El modo sin verificación TLS se obtiene con `ClientBuilder::danger_accept_invalid_certs(true)`; equivale a `CERT_NONE` + `check_hostname=False` (transfer.py:105-109). Cuidado: `danger_accept_invalid_certs` también desactiva la validación del hostname, igual que el original.
- Redirecciones: `redirect::Policy::custom` que rechace destinos distintos de `http`/`https` y limite a 10 (urllib usa `max_redirections = 10` por defecto; Duda 7).
- ZIP: `zip`. Validación propia antes de escribir (no basta `enclosed_name`). Detección de enlace con `unix_mode()` y `S_IFLNK`.
- TAR: `tar`, con `flate2` (gzip), `xz2` (xz), `bzip2` (bz2). La detección de compresión por firma: gzip `1f 8b`, xz `FD 37 7A 58 5A 00`, bz2 `42 5A 68` (`BZh`).
- Hashes: `md-5`, `sha1`, `sha2`. Para algoritmos adicionales con prefijo explícito (`sha3_*`, `blake2*`) usar `digest` con crates específicas; ver Duda 2.
- Hilos y cancelación: replicar `_Stream` con `std::thread` + `std::sync::mpsc::sync_channel(16)`, `recv_timeout(250 ms)` y `AtomicBool` de cancelación. Lectura con bloque de 256 KiB. Nunca tocar el progreso desde el hilo de red.

### Mapa de comportamientos

- Reintentos: `attempts=5`, `delay=2` con duplicación. En Rust, los valores por defecto deben ser los del original para que `versions.py` (`attempts=2`, `delay=1`) siga igual.
- El progreso se reporta cada 0.25 s y en la primera vez (línea 289, 302).
- Permisos de Windows y rutas largas: los límites de longitud de ruta están en `path_limit.py`, fuera de este contrato.
- Enlaces en extracción TAR (sólo `isfile` o `isdir` permitidos, línea 45) y ZIP (`S_IFLNK`). En Windows, detectar reparse points con `symlink_metadata` y el atributo `FILE_ATTRIBUTE_REPARSE_POINT`. `Path.is_junction()` de Python requiere 3.12 (ver `archives.md`).
- Errores: distinguir en Rust una variante para `HTTPError` permanente (no `YitoError`), otra para `DownloadIntegrityError` con los campos `url`, `size`, `host`, `portal`, y `Cancelled`.

### Dudas

- Duda 1: la reanudación solo ocurre dentro de una misma llamada a `download` (línea 220 reinicia `received=0`). Un archivo parcial dejado por un proceso que murió se sobrescribe en el siguiente intento (modo `'wb'`). ¿Es intencionado o debe reanudarse entre ejecuciones?
- Duda 2: sin prefijo solo se infieren 32/40/64/128 caracteres. Con prefijo se acepta cualquier algoritmo de `hashlib` con `digest_size > 0` (p. ej. `blake2b` con 128 hex, `sha3_256`). ¿Qué algoritmos debe aceptar la versión Rust? Un digest de longitud no estándar sin prefijo produce el mensaje `Algoritmo de hash no soportado: ` con el nombre vacío.
- Duda 3: `download_hashes` devuelve el valor original sin normalizar (línea 102). ¿Debe normalizarse en Rust?
- Duda 4: `extract_zip` valida tamaños con `file_size` declarado, pero copia sin límite (línea 401-402). Un ZIP con `file_size` falso puede escribir más de `MAX_EXPANDED`. ¿Rust debe limitar la lectura real (`take`)?
- Duda 5: el ZIP se detecta por contenido con `zipfile.is_zipfile` (archives.py:29) antes que por extensión, así que un `.exe` cuyo contenido es ZIP se extrae en lugar de copiarse. ¿Es el comportamiento deseado?
- Duda 6: el reintento TLS sin verificar (línea 330-333) no mira `remaining`; con `attempts=1` igualmente reintenta. Además el flag persiste en los intentos siguientes. ¿Es intencionado?
- Duda 7: el límite de redirecciones es `urllib` (10) y `reqwest` por defecto también 10. Confirmar que no hay otra expectativa.
- Duda 8: `web_page` mira la extensión de la URL pedida, no la final (línea 358, `request.full_url`), y solo los 512 primeros bytes. ¿Debe usar la URL final?
- Duda 9: con raíz `None` se usa el `build_opener` por defecto de `urllib` (lee proxy del entorno); con raíz se usa `network.opener` (lee `config.properties` y además el registro de Windows vía `getproxies`). ¿Qué fuente de proxy debe seguir Rust?
- Duda 10: el 1 MiB de progreso reinicia el presupuesto solo en la rama `OSError` (línea 344), no en `HTTPError` ni `URLError`. ¿Es intencionado?
- Duda 11: `ARCHITECTURE.md` dice que el extractor acepta gzip/xz/bzip2, pero el código solo los acepta dentro de TAR (`tarfile`). Un `.gz` suelto produce `Formato de archivo todavía no soportado`. Ver `archives.md`.
- Duda 12: `download` escribe en el destino sin temporal. ¿Quién garantiza la publicación atómica (engine)?
