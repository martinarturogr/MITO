# network.py → mito_transfer::network (interfaz mínima para F2)

Origen: `core/app/mito/network.py` (502 líneas). Este contrato cubre SOLO lo que `transfer.py` usa. El módulo completo (diagnóstico, autenticación de portal, `Network`, `prompt_credentials`, `intercept`) se porta en F9 y tendrá su propio contrato.

Tests relacionados: `core/tests/test_network.py` y `test_network_setup.py` (F9). En F2, `test_transfer.py` lo ejercita indirectamente en el test de reintento sin verificar (línea 121 usa `root=Path(tmp)`, es decir, la rama `network.opener`).

## Responsabilidad (mínima para F2)

Construir el `OpenerDirector` de `urllib` que usa la descarga cuando hay una raíz de instalación: aplica el proxy definido en `config.properties` de la raíz, incluido el proxy vacío que anula el del sistema, y añade los handlers que llegan desde `transfer`.

## API usada por transfer.py

| Función/clase | Entradas | Salida | Errores | Uso en transfer |
|---|---|---|---|---|
| `opener(root, environment=None, *handlers)` (77) | raíz, entorno opcional, handlers extra | `urllib.request.OpenerDirector` | propaga los de `proxy_environment` | `_build_opener` (transfer.py:118-119), con `environment=None` |
| `proxy_environment(root, environment)` (30) | raíz, `dict` de entorno | `dict` con las claves de proxy sustituidas | `YitoError` por URL de proxy inválida | llamado por `opener` cuando `environment is None` (usa `os.environ`) |
| `properties(root)` (18) | raíz | `dict[str, str]` de `root/config.properties` | nunca (archivo ausente → `{}`) | llamado por `proxy_environment` |
| `validate_url(url)` (46) | URL | `urllib.parse.SplitResult` | `YitoError('Configura una URL HTTP/HTTPS sin credenciales')` si no es http/https, no tiene host, o tiene usuario o contraseña | llamado por `proxy_environment` para valores de proxy no vacíos |
| `ExplicitProxyHandler(ProxyHandler)` (70) | mapa `proxies` | handler de `urllib` | — | creado por `opener` |

## Comportamiento

### `properties(root)` (líneas 18-27)

- Lee `root/config.properties` en UTF-8 con BOM opcional (`utf-8-sig`).
- Ignora líneas vacías, las que empiezan por `#` o `!`, y las que no contienen `=`.
- Clave y valor se recortan. Divide en la primera `=`.

Ejemplo de archivo:

```
# comentario
http_proxy=http://proxy.empresa.local:8080
no_proxy=localhost,127.0.0.1
```

### `proxy_environment(root, environment)` (líneas 30-43)

- Copia el entorno recibido.
- Para `http_proxy`, `https_proxy` y `no_proxy` presentes en `config.properties`:
  - Si el valor no está vacío y la clave no es `no_proxy`, valida con `validate_url`.
  - Elimina de la copia cualquier clave equivalente sin distinguir mayúsculas.
  - Escribe la clave en mayúsculas (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`), con el valor (puede ser `''`).
- Un valor vacío **anula** el proxy heredado del sistema (comentario de línea 40).

### `opener(root, environment=None, *handlers)` (líneas 77-83)

- `env = proxy_environment(root, os.environ)` si `environment is None`.
- `proxies = urllib.request.getproxies()` (incluye el registro de Windows en Windows).
- Sobrescribe `proxies` con las claves `http_proxy`, `https_proxy`, `no_proxy` (en minúsculas, sin el sufijo `_proxy`) del entorno calculado.
- Construye `build_opener(ExplicitProxyHandler(proxies), *handlers)`. El `ProxyHandler` por defecto de `urllib` no se añade porque ya hay una subclase.

### `ExplicitProxyHandler.proxy_open` (línea 71-74)

- No usa proxy si el valor es falso o si `urllib.request.proxy_bypass_environment(req.host, self.proxies)` indica bypass (lista `no_proxy`).
- Si no, delega en `ProxyHandler.proxy_open`.

## Invariantes

1. Un `config.properties` con `http_proxy=` (vacío) impide usar el proxy del sistema para HTTP, aunque `HTTP_PROXY` esté en el entorno.
2. Un valor de proxy que no sea `http://` o `https://` con host, sin credenciales embebidas, produce `YitoError` antes de cualquier conexión.
3. `no_proxy` se pasa tal cual a `proxy_bypass_environment` (no se valida con `validate_url`).
4. `transfer.py` nunca pasa un entorno propio: siempre `environment=None`, así que el entorno real se lee en cada `_build_opener` (transfer.py:255).

## Casos límite y errores

- Raíz sin `config.properties`: `properties` devuelve `{}` y el entorno queda igual.
- Archivo con BOM UTF-8: se ignora gracias a `utf-8-sig`.
- Proxy con usuario y contraseña (`http://user:pass@proxy`): rechazado con el mensaje de `validate_url`. Las credenciales no se persisten (docstring de línea 1).
- Lectura de `config.properties` con otra codificación: `UnicodeDecodeError` no capturado (Duda 1).

## Casos de test del original (para F2)

Ninguno de `test_transfer.py` prueba `proxy_environment` ni `ExplicitProxyHandler` directamente. Los casos de proxy están en `test_network.py` y `test_bootstrap_proxy.py` (F9).

## Dependencias

- `mito.i18n.tr` (línea 2; mensaje de `validate_url`).
- `mito.storage.YitoError` (línea 15).
- Stdlib: `os`, `urllib.request`, `urllib.parse`, `urllib.error`.
- Otras partes de `network.py` (`Network`, `Probe`, `diagnose`, `prompt_credentials`, `intercept`, `system_proxy`, `NoRedirect`, `Form`) no las usa `transfer.py` y quedan fuera.

## Notas para Rust

- Crate sugerido: `reqwest` (blocking) con `ClientBuilder::proxy(Proxy::all(url)?.no_proxy(NoProxy::from_string(no_proxy)))`. Cuando `config.properties` tenga una clave vacía, usar `ClientBuilder::no_proxy()` para anular el proxy del sistema, como hace el original.
- Lectura de `config.properties`: `std::fs::read` + decodificación UTF-8 que quite el BOM. Mismo parseo que `properties` (líneas con `#`/`!` ignoradas, split en la primera `=`).
- `getproxies()` en Windows lee el registro (proxy del sistema) y `reqwest` no lo hace por defecto; ver Duda 2.

### Dudas

- Duda 1: `properties` lee el archivo como UTF-8 con `utf-8-sig`. Si el archivo está en otra codificación (p. ej. ANSI de Windows), el original falla con `UnicodeDecodeError` sin mensaje propio. ¿Debe Rust aceptar la codificación del sistema o fallar con mensaje?
- Duda 2: en Windows, `urllib.request.getproxies()` incluye la configuración del registro y del sistema. `reqwest` solo lee `HTTP_PROXY`/`HTTPS_PROXY` del entorno. ¿Debe Rust leer también el proxy del sistema (WinINET) para igualar el comportamiento?
- Duda 3: el original mapea `no_proxy` tal cual; `proxy_bypass_environment` implementa su propia lógica de comodines y sufijos. Confirmar que `reqwest::NoProxy` da el mismo resultado para `.empresa.local` y `*.local`.
- Duda 4: si `root` es `None`, `transfer` no usa `network.opener` sino el `build_opener` por defecto de `urllib` (transfer.py:116-117). Eso ignora `config.properties`. ¿Debe Rust aplicar siempre el mismo cliente? Ver `transfer.md`, Duda 9.
