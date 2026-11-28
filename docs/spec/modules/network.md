# network.py → mito_transfer::network

Origen: `core/app/mito/network.py` (502 líneas), `core/tests/test_network.py` (5 tests), `core/tests/test_network_setup.py` (8 tests, uno de ellos de UI, ver `network_ui.md`), `core/tests/test_bootstrap_proxy.py` (2 tests, portados al bootstrap de Rust, ver `bootstrap.md` sección 5.7).

Consumidor en F2: `transfer.py` (solo usa `opener`, `proxy_environment`, `properties`, `validate_url` y `ExplicitProxyHandler`). El resto del módulo (diagnóstico, portal, `Network`, `prompt_credentials`, `intercept`) se porta en F9 y está en las secciones 3 a 12.

Convención: `network.py:NN` es la línea del original. Las citas a tests usan `test_network.py:NN`, `test_network_setup.py:NN` y `test_bootstrap_proxy.py:NN`.

## Responsabilidad

Enrutar el tráfico HTTP(S) de MITO por el proxy correcto (configurado en `config.properties`, heredado del entorno o del sistema) y ofrecer un diagnóstico de red que detecta portales cautivos, proxies que exigen autenticación y rutas que no funcionan. Si el portal pide usuario y contraseña, los envía por POST sin guardarlos nunca.

## 1. API pública del original

| Función/clase | Línea | Entradas | Salida | Errores |
|---|---|---|---|---|
| `properties(root)` | 18 | `root: Path` | `dict[str, str]` de `root/config.properties` | `UnicodeDecodeError` (es `ValueError`) si el archivo no es UTF-8; `OSError` si `config.properties` es un directorio. Archivo ausente: `{}` |
| `proxy_environment(root, environment)` | 30 | `root`, `environment: Mapping` | copia de `environment` con las claves de proxy sustituidas | `YitoError` por URL de proxy inválida (solo valores de config no vacíos) |
| `validate_url(url)` | 46 | `url: str` | `urllib.parse.SplitResult` | `YitoError('Configura una URL HTTP/HTTPS sin credenciales')` |
| `origin(url)` | 53 | `url: str` | `(scheme, netloc)` en minúsculas | nunca |
| `redirect_location(url, exc)` | 58 | URL de la petición, `HTTPError` | URL absoluta del `Location` si el código está en `REDIRECTS`; si no, `''` | nunca |
| `NoRedirect` | 64 | handler de redirecciones | — | — |
| `ExplicitProxyHandler(ProxyHandler)` | 70 | `proxies: dict` | handler de `urllib` | — |
| `opener(root, environment=None, *handlers)` | 77 | raíz, entorno opcional, handlers extra | `urllib.request.OpenerDirector` | los de `proxy_environment` |
| `Form(password_field)` | 86 | nombre configurado del campo de contraseña | parser HTML con `forms` | nunca |
| `Network(root, environment=None)` | 116 | raíz, entorno opcional | objeto con `url`, `expected`, `timeout`, `auth_url`, `enabled`, `config` | `YitoError` por rango o URL inválida; `ValueError` por número no entero (ver sección 4.1) |
| `Network.check(log=True)` | 132 | `log: bool` | `bool` | nunca (lo captura y devuelve `False`) |
| `Network.local_network()` | 146 | — | `bool` | nunca |
| `Network.log(text, **details)` | 152 | texto, detalles | — | nunca |
| `Network.redirect(client, url)` | 157 | cliente, URL | `str` (URL de redirección o `''`) | nunca |
| `Network.portal_page(client)` | 167 | cliente con cookies | `str` (URL del portal) | nunca |
| `Network.authenticate(username, password)` | 182 | `str`, `str` | `bool` | `YitoError` (HTTPS obligatorio, formulario en otro origen, URL inválida); `OSError`/`ValueError` se registran y dan `False` |
| `SYSTEM_PROXY_SCRIPT` | 246 | — | script PowerShell (constante) | — |
| `REDIRECTS` | 264 | — | `(301, 302, 303, 307, 308)` | — |
| `no_proxy_from_override(override)` | 267 | `str` (ProxyOverride del registro, puede ser `None`) | `str` con los `no_proxy` separados por comas | nunca |
| `system_proxy(target, run=subprocess.run)` | 282 | `target: str` (URL), `run` inyectable | `dict` `{url, source, no_proxy}` o `None` | nunca (devuelve `None`) |
| `Probe` (dataclass) | 301 | `status, location, authenticate, error, url` | `kind(expected)` | — |
| `probe(url, proxy, timeout)` | 319 | URL, proxy (`''` = directo), segundos | `Probe` | nunca (errores de red → `Probe(status=0)` o estado del CONNECT) |
| `Diagnosis` (dataclass) | 337 | `kind, proxy, source, status, authenticate, portal, changes` | — | — |
| `classify(network, proxy)` | 348 | `Network`, proxy | `(Probe, kind)` | nunca |
| `portal_fields(url, proxy, timeout)` | 360 | URL del portal, proxy, segundos | `dict` con nombres de campo no por defecto | nunca (devuelve `{}`) |
| `diagnose(root, network=None, resolve=system_proxy)` | 380 | raíz, `Network` opcional, resolutor inyectable | `Diagnosis` | `YitoError`/`ValueError` si falla la construcción de `Network` (lo captura la UI) |
| `apply(engine, changes)` | 427 | motor, `dict` de claves | — | `OSError`, `YitoError` (lo captura la UI) |
| `prompt_credentials(failed=False, translate=tr)` | 436 | `failed: bool`, `translate` | `dict` `{username, password}` o `None` | nunca (`None` si cancela o PowerShell falla) |
| `intercept(root, environment=None)` | 482 | raíz, entorno opcional | `None` | nunca (silencia `OSError`, `ValueError`, `YitoError`) |

### 1.1 Uso desde `transfer.py` (F2)

- `_build_opener` llama `opener(root, environment=None)` (`transfer.py:118-119`) y, con `environment=None`, `opener` lee `os.environ` en cada construcción (`transfer.py:255`).
- `transfer.py` nunca pasa un entorno propio, así que el proxy efectivo es siempre el calculado aquí.
- Si `root` es `None`, `transfer` usa el `build_opener` por defecto de `urllib` (`transfer.py:116-117`) y se ignora `config.properties` (ver `transfer.md`, Duda 9).

## 2. Formatos de disco

Este módulo solo lee `config.properties`. No escribe nada: el guardado lo hace `apply` a través de `configuration.Configuration` (sección 9).

Ejemplo real de `config.properties` leído por `properties`:

```
# comentario
http_proxy=http://proxy.empresa.local:8080
https_proxy=http://proxy.empresa.local:8080
no_proxy=localhost,127.0.0.1
network.check_url=https://www.gstatic.com/generate_204
network.timeout_seconds=4
enable_proxy_authentication=true
proxy_authentication_url=http://portal.empresa.local/login
proxy_authentication_form_field.origen=mito
```

Parseo (`properties`, líneas 18-27): UTF-8 con BOM opcional (`utf-8-sig`), `splitlines()`, se ignoran las líneas vacías, las que empiezan por `#` o `!` y las que no contienen `=`; la clave y el valor se recortan; se divide en la primera `=`. Si hay claves repetidas, gana la última.

Las claves que usa este módulo están validadas en `configuration.py:12-31` y `configuration.py:65-83` (sección 9).

## 3. Comportamiento de las funciones auxiliares

### 3.1 `properties(root)` (líneas 18-27)

Descrito en la sección 2. Si `root/config.properties` no existe devuelve `{}`.

### 3.2 `proxy_environment(root, environment)` (líneas 30-43)

1. `values = properties(root)`; `result = dict(environment)`.
2. Para cada nombre en `('http_proxy', 'https_proxy', 'no_proxy')` presente en `values`:
   - Si el valor no está vacío y el nombre no es `no_proxy`: `validate_url(value)` (puede lanzar `YitoError`).
   - Elimina de la copia cualquier clave cuyo `lower()` sea el nombre (así borra `HTTP_PROXY`, `http_proxy`, etc.).
   - Escribe `NAME.upper()` con el valor, **aunque sea `''`** (línea 41-42: «Empty values deliberately suppress inherited Windows proxy settings»).
3. Devuelve la copia.

Consecuencias:

- `http_proxy=` (vacío) impide usar el proxy heredado para HTTP: `HTTP_PROXY=''` llega a `opener`, que lo convierte en `proxies['http'] = ''`, y `ExplicitProxyHandler.proxy_open` ignora falsos (línea 72).
- La validación solo se aplica a los valores de `config.properties`. Las variables de entorno del proceso (`HTTP_PROXY` con credenciales, por ejemplo) no se validan aquí, y `urllib` las usa tal cual (con cabecera `Proxy-authorization` Basic si llevan credenciales).

### 3.3 `validate_url(url)` (líneas 46-50)

`urllib.parse.urlsplit(url)`. Lanza `YitoError('Configura una URL HTTP/HTTPS sin credenciales')` si se cumple **cualquiera** de:

- `scheme` no es `http` ni `https` (comparación tras normalizar a minúsculas por `urlsplit`).
- `hostname` es falsy (`None` o `''`).
- `username is not None` o `password is not None`. Con `http://@host`, `username` es `''` y también se rechaza.

No valida el puerto (`.port` no se lee), así que `http://host:abc` pasa aquí y falla después en la conexión.

Devuelve el `SplitResult`.

### 3.4 `origin(url)` (líneas 53-55)

`(parsed.scheme.lower(), parsed.netloc.lower())`. No normaliza el puerto por defecto: `http://h` y `http://h:80` son orígenes distintos. Lo usan `redirect_location` (indirectamente), `portal_page` (línea 178), `authenticate` (línea 200) y `Network.log` (línea 155).

### 3.5 `redirect_location(url, exc)` (líneas 58-61)

- Si `exc.code in REDIRECTS` y hay cabecera `Location`: devuelve `urllib.parse.urljoin(url, location)`.
- En cualquier otro caso devuelve `''`. Un 400 o un 407 nunca es una redirección, aunque tenga `Location`.

### 3.6 `NoRedirect` (líneas 64-67)

`redirect_request` devuelve `None`. `urllib` entonces lanza `HTTPError` con el código 3xx y sin seguir el `Location`. Motivo (comentario de línea 66): no reenviar un POST con credenciales ni aceptar un portal como si fuera Internet.

Lo usan `check`, `probe`, `portal_fields`, `redirect` (vía `authenticate`) y el `opener` de `authenticate`.

### 3.7 `ExplicitProxyHandler.proxy_open` (líneas 70-74)

```
if not proxy or urllib.request.proxy_bypass_environment(req.host, self.proxies):
    return None        # conexión directa
return super().proxy_open(req, proxy, type)
```

- `proxy` vacío: directo.
- `proxy_bypass_environment(req.host, self.proxies)`: usa solo la clave `no` del mapa `self.proxies`, es decir, el `no_proxy` ya resuelto por `opener`. Verificado en el código fuente de la biblioteca estándar (3.13.14): compara el host en minúsculas sin puerto con cada entrada de `no_proxy.split(',')`, quitando los puntos iniciales (`lstrip('.')`), y coincide si `host == name` o si `host` termina en `'.' + name`. `'*'` solo como valor total.
- `super().proxy_open` (la implementación de `ProxyHandler`) **vuelve a comprobar** `proxy_bypass(req.host)` de la biblioteca (ver sección 3.9). Esa comprobación no mira el `no_proxy` de la config (Posibles fallos F4).

### 3.8 `opener(root, environment=None, *handlers)` (líneas 77-83)

1. `env = proxy_environment(root, os.environ)` si `environment is None`; si no, `env = environment` tal cual (la capa de config ya se aplicó en `engine._environment`, `engine.py:556`).
2. `proxies = urllib.request.getproxies()`.
3. Para cada `key` de `env` cuyo `lower()` sea `http_proxy`, `https_proxy` o `no_proxy`: `proxies[key.lower().removesuffix('_proxy')] = value`. Es decir, `http`, `https` o `no`. Un valor vacío se guarda como `''`.
4. `build_opener(ExplicitProxyHandler(proxies), *handlers)`. No se añade el `ProxyHandler` por defecto de `urllib`.

Consecuencia: el `ProxyHandler` de `urllib` nunca se instala, así que solo corre la lógica de `ExplicitProxyHandler`.

### 3.9 Semántica de `urllib.request.getproxies()` en Windows

Verificada en el código fuente de la biblioteca (`getproxies_environment` y `getproxies` en 3.13.14; `getproxies_registry`, `proxy_bypass` y `proxy_bypass_registry` en 3.14, la versión que va dentro del ZIP de Python de MITO según `bootstrap.lock.json`).

```python
def getproxies():
    return getproxies_environment() or getproxies_registry()
```

1. **Variables de entorno del proceso** (`getproxies_environment`): recorre `os.environ`. Toda variable cuyo nombre termina en `_proxy` (sin distinguir mayúsculas) con valor no vacío se guarda con clave `nombre_sin_sufijo.lower()`. Después, una segunda pasada solo con nombres en minúsculas: valor vacío → **borra** esa clave (`pop`); valor no vacío → la fija. Así, `http_proxy` en minúsculas gana a `HTTP_PROXY`. Si existe `REQUEST_METHOD`, se descarta `http` (CVE-2016-1000110).
2. **Registro** (`getproxies_registry`), solo si el paso 1 devuelve un diccionario vacío. Lee `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings`:
   - Si `ProxyEnable` es distinto de cero, lee `ProxyServer`. Si no contiene `=` ni `;`, usa el mismo valor para `http`, `https` y `ftp` (`http={0};https={0};ftp={0}`).
   - Cada `proto=dir` se guarda; si la dirección no tiene prefijo `esquema://` y el protocolo es `http`, `https` o `ftp`, se le añade `http://`. Para `socks`, se añade `socks://`.
   - Si hay `socks`, `http` y `https` se rellenan con `socks4://` si no estaban.
   - No lee `AutoConfigURL` (PAC) ni WPAD: `urllib` no evalúa scripts. No lee `ProxyOverride` en este paso.
   - Errores de registro (`OSError`, `ValueError`, `TypeError`) dejan el diccionario vacío.

Consecuencias para MITO:

- Si el proceso tiene **cualquier** variable `*_proxy` con valor, el registro de Windows se ignora por completo. Incluida `NO_PROXY` sola: produce `{'no': ...}`, que es truthy.
- `opener` sobrescribe `http`, `https` y `no` con lo que haya en `env` (que ya incluye la config). Las claves `ftp` y `socks` del registro quedan en el diccionario pero no las usa ningún handler de MITO con esquema `ftp`.
- Con `config.properties` sin claves de proxy y sin variables de entorno, el proxy efectivo de `urllib` es el manual del registro (`ProxyEnable=1`). El PAC y WPAD del sistema **no** entran aquí: los resuelve `system_proxy` con PowerShell (sección 6).

## 4. Clase `Network` y comprobación de Internet

### 4.1 `Network.__init__(root, environment=None)` (líneas 117-130)

| Clave (`config.properties`) | Valor por defecto | Validación | Efecto |
|---|---|---|---|
| (parámetro `root`) | — | — | `self.root` |
| (parámetro `environment`) | `None` | — | `self.environment`, se pasa a `opener` |
| `network.timeout_seconds` | `'4'` | `float(...)` (sin error propio si no es número: `ValueError`, línea 121). Luego `0 < t <= 60`, si no `YitoError('network.timeout_seconds debe estar entre 0 y 60')` (líneas 122-123) | `self.timeout` (segundos, `float`) |
| `network.check_url` | `'https://www.gstatic.com/generate_204'` | `validate_url` (línea 125) | `self.url` |
| `network.check_status` | `'204'` | `int(...)` (línea 126): `ValueError` si no es entero. **No** se valida el rango en `Network`; el rango 100-599 lo valida `configuration.validated` | `self.expected` |
| `proxy_authentication_url` | `''` | Si no está vacío: `validate_url` (líneas 127-129) | `self.auth_url` |
| `enable_proxy_authentication` | `'false'` | `.lower() == 'true'` y además `bool(self.auth_url)` (línea 130) | `self.enabled` |

Claves que usa el resto del módulo (no se leen en `__init__`, sino en `authenticate`):

| Clave | Valor por defecto | Uso |
|---|---|---|
| `proxy_authentication_username_field` | `'username'` (línea 187) | Nombre del campo de usuario en el POST |
| `proxy_authentication_password_field` | `'password'` (línea 188) | Nombre del campo de contraseña en el POST |
| `proxy_authentication_require_https` | `'true'` (línea 217) | Solo `.lower() == 'false'` desactiva la exigencia de HTTPS. Cualquier otro valor la mantiene |
| `proxy_authentication_form_field.<nombre>` | — | Campo extra del POST (`<nombre>` = clave sin prefijo), líneas 219-220 |

`self.config` es el `dict` de `properties(root)`. Los `http_proxy`, `https_proxy` y `no_proxy` no se leen aquí: los aplica `opener`.

### 4.2 `Network.check(log=True)` (líneas 132-144)

1. `opener(self.root, self.environment, NoRedirect())` (sin `HTTPCookieProcessor`: no comparte cookies de portal).
2. `open(self.url, timeout=self.timeout)`; devuelve `response.status == self.expected`.
3. `HTTPError`: `exc.close()` y `False`. Un 302 del portal, un 407 o un 500 son `False`. **No se registra nada** (no es excepción de red).
4. `OSError`, `ValueError` o `YitoError`: si `log` es verdadero, `write(root, 'warning', 'network.check', message(root, exc))`, y `False`.

`message(root, exc)` es `diagnostics.message` (`diagnostics.py:50-55`): para `CalledProcessError` devuelve `Child process failed with exit code N`, y en otro caso el texto saneado.

`log=False` lo usa el sondeo periódico de la UI (`ui.py:1003`), para no llenar el log cada pocos segundos.

### 4.3 `Network.local_network()` (líneas 146-150)

- En sistemas que no son Windows (`os.name != 'nt'`): `True`.
- En Windows: `ctypes.windll.wininet.InternetGetConnectedState(byref(flags), 0)`. Devuelve si hay **algún** adaptador con conexión (LAN, Wi-Fi, móvil). No dice nada de si hay Internet.

### 4.4 `Network.log(text, **details)` (líneas 152-155)

`write(root, 'warning', 'network.authenticate', text, portal=origin(self.auth_url), **details)`. El campo `portal` se serializa como array JSON (`origin` devuelve tupla): `"portal":["http","portal.empresa.local"]`. Nunca lleva la URL completa, porque los portales meten tokens de sesión en la query (comentario de línea 153).

### 4.5 `Network.redirect(client, url)` (líneas 157-165)

- `client.open(url, timeout)` con éxito → `''`.
- `HTTPError` → `exc.close()` y `redirect_location(url, exc)`. Un 400 devuelve `''`.
- `OSError` o `ValueError` → `''`.

### 4.6 `Network.portal_page(client)` (líneas 167-180)

Objetivo: obtener la URL **actual** del portal. Los portales meten un token por sesión en la redirección, así que la URL guardada en `proxy_authentication_url` puede estar caducada (comentario de línea 168; test `test_network.py:76-111`).

1. `url` = la URL de `check_url` con esquema forzado a `http` (mismo `netloc`, `path` y `query`, sin fragmento). Es la petición que el navegador haría primero, y que el portal intercepta.
2. Hasta 5 veces: `url = self.redirect(client, url)`. Si no hay redirección (`''`), se sale del bucle. Si la redirección apunta al mismo origen que `auth_url` (`origin(url) == origin(self.auth_url)`), se devuelve esa URL.
3. Si no se encontró nada, devuelve `self.auth_url`.

Nota: el primer `redirect` se hace sobre `http`, aunque `check_url` sea `https`. Así se detecta un portal sin negociar TLS.

### 4.7 `Network.authenticate(username, password)` (líneas 182-242)

Paso a paso:

1. Si `not self.enabled`, o `username` vacío, o `password` vacío: `False` sin log (línea 183).
2. `client = opener(root, self.environment, HTTPCookieProcessor(CookieJar()), NoRedirect())` (línea 186). Cookies y credenciales viven solo durante esta secuencia. `user_field` y `pass_field` se leen de la config (líneas 187-188).
3. `url = self.portal_page(client)` (línea 190).
4. Bucle de hasta 5 peticiones GET (línea 191):
   - `client.open(url, timeout)`. Si funciona: `parser = Form(pass_field)`; `parser.feed(page.read(1 MiB).decode('utf-8', errors='replace'))`; `break`.
   - Si `HTTPError`: `exc.close()`; `target = redirect_location(url, exc)`. Si no hay `target` o `origin(target) != origin(self.auth_url)`: `log('Portal page unavailable', status=exc.code)` y `return False`. Si no, `url = target`.
   - Si el bucle termina sin `break` (`for…else`, línea 204): `log('Portal page redirects too many times')` y `return False`.
5. Destino del POST (líneas 207-212):
   - Si `parser.forms` no está vacío: `action = urljoin(url, forms[0].action)`; `fields = forms[0].fields`.
   - Si no hay formularios con contraseña: `action = url`, `fields = {}`, y `log('Portal page has no password form; posting to the page itself')`.
6. `parsed = validate_url(action)` y `original = validate_url(self.auth_url)` (líneas 213-214). Pueden lanzar `YitoError`.
7. Si `(scheme, netloc)` del destino no coincide con el de `auth_url` (sin normalizar, línea 215): `YitoError('El formulario apunta a otro origen; configura su URL explícitamente')`.
8. Si `parsed.scheme != 'https'` y `proxy_authentication_require_https.lower() != 'false'`: `YitoError('El POST requiere HTTPS; configura proxy_authentication_require_https=false para un portal HTTP')` (líneas 217-218).
9. `fields.update({clave_sin_prefijo: valor})` para cada `proxy_authentication_form_field.*` (líneas 219-220). Estos valores **sobrescriben** los campos ocultos del formulario.
10. `fields.update({user_field: username, pass_field: password})` (línea 221). Las credenciales ganan siempre.
11. `body = urlencode(fields).encode()`; `Request(action, data=body, headers={'Content-Type': 'application/x-www-form-urlencoded'})` (líneas 222-223).
12. `status = 200`. `client.open(request)`:
    - Éxito: `status = response.status`.
    - `HTTPError`: `exc.close()`; `status = exc.code`; si el código **no** está en `(301, 302, 303)`: `log('Portal rejected the login POST', status=status)` y `return False`.
    - Un 307 o 308 también se considera rechazo, a diferencia de los 301, 302 y 303 (comportamiento literal del original).
13. `if self.check(): return True` (línea 235). `check()` usa un cliente **nuevo** sin cookies: prueba que la sesión funciona sin depender de las cookies del portal (comentario de línea 234). Si falla: `log('Internet still unavailable after the login POST', status=status)` y `return False`.
14. `except (OSError, ValueError) as exc`: `log(message(root, exc))` y `return False` (líneas 239-242). Sin `status`.
15. `YitoError` de los pasos 6 a 8 **no** se captura aquí: se propaga al llamador (ver tests `test_network.py:62-63`).

Mensajes de log de `authenticate` (en inglés, literales, con `portal=<origen>` en todos):

| Línea | Mensaje | Detalles extra |
|---|---|---|
| 201 | `Portal page unavailable` | `status` |
| 205 | `Portal page redirects too many times` | — |
| 212 | `Portal page has no password form; posting to the page itself` | — |
| 232 | `Portal rejected the login POST` | `status` |
| 237 | `Internet still unavailable after the login POST` | `status` |
| 241 | texto de `message(root, exc)` | — |

`check()` de la línea 235 puede escribir además su propio `network.check` si hay excepción de red.

Los `YitoError` de los pasos 6 a 8 y el resultado `False` no escriben en el log del portal: el llamador decide qué mostrar (ver sección 10 y Posibles fallos F1).

## 5. Formularios HTML

### 5.1 `Form(password_field)` (líneas 86-113)

Subclase de `html.parser.HTMLParser` (`convert_charrefs=True` por defecto, así que las entidades de los atributos se decodifican: `&amp;` → `&`, `&#x41;` → `A`).

Estructura guardada en `self.forms`: lista de `[action, fields, has_password, user_field, password_field]`.

Reglas (líneas 94-113):

- `<form>`: abre `current = [attrs.get('action', ''), {}, False, '', '']`. Si ya había un formulario abierto, se pierde (no hay pila).
- `<input>` dentro de un formulario abierto (no se procesan `select`, `textarea` ni `button`):
  - Sin `name` (o `name=""`): se ignora.
  - `kind = (type or 'text').lower()`.
  - `fields[name] = attrs.get('value', '')`. Se guarda **cualquier** input, incluidos `password`, `checkbox` y `submit`, sin mirar `checked`.
  - Si `name == password_field` **o** `kind == 'password'`: `has_password = True`; `current[4]` se fija al primer nombre de contraseña encontrado.
  - Si no, si todavía no hay usuario y `kind in ('text', 'email')`: `current[3] = name`. Es decir, el **primer** campo de texto o email del formulario. Los `hidden`, `tel`, `number`, `search` y los que no tienen `type` (cuando sí tienen `type` distinto) no son candidatos (el valor por defecto de `type` es `text`, así que los que no lo tienen sí).
- `</form>`: si `has_password` es verdadero, se añade a `self.forms`; luego `current = None`.

Formularios sin contraseña no se guardan. Un formulario con contraseña, pero sin campo de usuario, tiene `user_field = ''`.

Ejemplo de la prueba (`test_network.py:76-93`): `<form action="/"><input type="hidden" name="magic" value="1"><input name="username"><input name="password" type="password"></form>` produce `['/', {'magic': '1', 'username': '', 'password': ''}, True, 'username', 'password']`.

## 6. Proxy del sistema (PowerShell)

### 6.1 `SYSTEM_PROXY_SCRIPT` (líneas 246-263)

Python no evalúa PAC ni WPAD (comentario de línea 245). El script lo ejecuta PowerShell. Se copia literal de `network.py:247-262`:

```powershell
$ErrorActionPreference = 'Stop'
$target = [Uri]$env:MITO_PROXY_TARGET
$registry = @{}
try { $registry = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' } catch { }
$result = @{ url = ''; source = 'wpad'; override = [string]$registry.ProxyOverride }
if ($registry.ProxyEnable -eq 1 -and $registry.ProxyServer) { $result.source = 'manual' }
elseif ($registry.AutoConfigURL) { $result.source = 'pac' }
$resolver = [Net.WebRequest]::GetSystemWebProxy()
if (-not $resolver.IsBypassed($target)) {
    $proxy = $resolver.GetProxy($target)
    if ($proxy -and $proxy.AbsoluteUri -ne $target.AbsoluteUri -and $proxy.Scheme -in @('http', 'https')) {
        $result.url = '{0}://{1}:{2}' -f $proxy.Scheme, $proxy.Host, $proxy.Port
    }
}
[Console]::OutputEncoding = New-Object Text.UTF8Encoding
$result | ConvertTo-Json -Compress
```

La URL de destino llega por la variable de entorno `MITO_PROXY_TARGET`, no en la línea de comandos (`system_proxy`, línea 289).

### 6.2 `no_proxy_from_override(override)` (líneas 267-279)

Convierte el `ProxyOverride` de Windows (separado por `;`, `,` o espacios) en `no_proxy`:

1. Lista inicial fija: `['localhost', '127.0.0.1', '::1']`.
2. Para cada entrada: se recorta y se pasa a minúsculas. Se descartan las vacías y `<local>`.
3. Si empieza por `*.`, se quita el `*` (queda `.dominio`).
4. Se descartan las que contienen `*` o `/` (por ejemplo `10.*`, `a/b`) y las ya presentes en la lista inicial.
5. Se añade el resto (sin deduplicar entre sí).
6. Devuelve `','.join(items)`.

Ejemplo (`test_network_setup.py:62-63`): entrada `'<local>;*.corp.example;intranet;10.*;a/b'` → `'localhost,127.0.0.1,::1,.corp.example,intranet'`.

Consecuencia: `<local>` no se traduce a nada útil aquí. El `<local>` de Windows (hosts sin punto) sí lo aplica `urllib` después (Posibles fallos F4).

### 6.3 `system_proxy(target, run=subprocess.run)` (líneas 282-298)

1. Si no es Windows (`os.name != 'nt'`): `None`.
2. `run(['powershell.exe', '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', '-'], input=SYSTEM_PROXY_SCRIPT, capture_output=True, encoding='utf-8', timeout=30, env=dict(os.environ, MITO_PROXY_TARGET=target), creationflags=CREATE_NO_WINDOW)`. Si `returncode == 0` y hay salida, `json.loads`. Si no, `{}`.
3. `OSError`, `ValueError` o `SubprocessError` (incluido el `timeout`): `None`.
4. `url = data.get('url') or ''`. Si `validate_url(url)` falla (incluido el caso `''`): `None`.
5. Devuelve `{'url': url, 'source': data.get('source', 'wpad'), 'no_proxy': no_proxy_from_override(data.get('override', ''))}`.

Campo `source`, decidido en PowerShell (líneas 251-253):

- `manual`: `ProxyEnable -eq 1` y `ProxyServer` no vacío.
- `pac`: si no, `AutoConfigURL` no vacío.
- `wpad`: en cualquier otro caso (valor por defecto).

Este valor **no** refleja la ruta real que usó .NET: `GetProxy` puede venir de WPAD aunque haya `AutoConfigURL`. Ver Dudas D5.

## 7. Diagnóstico: `Probe`, `classify`, `diagnose`

### 7.1 `Probe` y `Probe.kind(expected)` (líneas 301-316)

Campos: `status: int = 0`, `location: str = ''`, `authenticate: str = ''`, `error: str = ''`, `url: str = ''`. `error` se guarda pero no se usa en ninguna decisión.

`kind(expected)`, en este orden:

| Condición | Resultado |
|---|---|
| `status == expected` | `'ok'` |
| `status == 407` | `'auth'` |
| `status in REDIRECTS` y `location` no vacío | `'portal'` |
| `status == 0` | `'unreachable'` |
| cualquier otra cosa | `'blocked'` |

### 7.2 `probe(url, proxy, timeout)` (líneas 319-334)

Una sola petición a través de `proxy` (`''` = directo) sin seguir redirecciones:

1. `route = ProxyHandler({'http': proxy, 'https': proxy} if proxy else {})`. Con `''` se usa un diccionario vacío, así que no hay proxy, ni siquiera el del entorno.
2. `client = build_opener(route, NoRedirect())`. Nota: aquí se usa `ProxyHandler` de la biblioteca, no `ExplicitProxyHandler` (ver Posibles fallos F7).
3. Éxito: `Probe(response.status, url=url)`.
4. `HTTPError`: `Probe(exc.code, exc.headers.get('Location', ''), authenticate, url=url)`. `authenticate` = los esquemas de `Proxy-Authenticate` (primer token de cada cabecera), sin repetidos, ordenados alfabéticamente y unidos con `', '`. Ejemplo: cabeceras `Negotiate` y `NTLM` → `'NTLM, Negotiate'` (orden alfabético: `NTLM` antes que `Negotiate`). `exc.close()`.
5. `OSError` o `ValueError`: `reason = str(getattr(exc, 'reason', exc))`. Si coincide con `Tunnel connection failed: (\d{3})` (regex de línea 333), `Probe(status=int(NNN))`. Si no: `Probe(status=0)`. En los dos casos `error = type(exc).__name__`.

**Caso «Tunnel connection failed: NNN».** Con `https://` a través de proxy, `urllib` hace un `CONNECT`. Si el proxy responde a `CONNECT` con un código distinto de 200, `http.client` solo conserva el texto del estado (no las cabeceras). Por eso:

- `Tunnel connection failed: 407 Proxy Authentication Required` → `Probe(407)`, `authenticate=''` (no se conocen los esquemas). `kind` = `'auth'`. Si luego `classify` reintenta por HTTP, obtiene los esquemas reales (sección 7.3).
- `Tunnel connection failed: 403 Forbidden` → `Probe(403)`, `kind` = `'blocked'`.
- Sin ese texto (por ejemplo, `ConnectionRefusedError`, `TimeoutError`) → `Probe(0)`, `kind` = `'unreachable'`.

### 7.3 `Diagnosis` (líneas 337-345)

| Campo | Tipo | Significado |
|---|---|---|
| `kind` | `str` | `'ok'`, `'offline'`, `'portal'`, `'auth'`, `'blocked'`, `'unreachable'` |
| `proxy` | `str` | Proxy probado (`''` = directo) |
| `source` | `str` | `'config.properties'`, `'manual'`, `'pac'`, `'wpad'`, `'direct'` o `''` |
| `status` | `int` | Código HTTP o 0 |
| `authenticate` | `str` | Esquemas de `Proxy-Authenticate` (solo si `auth`) |
| `portal` | `str` | URL absoluta de la redirección (solo si `portal`) |
| `changes` | `dict` | Claves `config.properties` propuestas |

### 7.4 `classify(network, proxy)` (líneas 348-357)

1. `result = probe(network.url, proxy, network.timeout)`; `kind = result.kind(network.expected)`.
2. Si `kind != 'ok'` y el esquema de `network.url` es `https`: `plain = probe(url con esquema http, …)`. Si `plain.kind` es `'auth'` o `'portal'`, se devuelven `plain` y ese kind. Si no, se mantiene el resultado HTTPS.
3. Devuelve `(result, kind)`.

Motivo: por HTTPS no se ven cabeceras de `407` ni redirecciones de portal (el CONNECT falla o el túnel no revela nada). Por HTTP sí.

### 7.5 `portal_fields(url, proxy, timeout)` (líneas 360-377)

1. Abre `url` a través de `proxy` con la misma ruta que `probe` (`ProxyHandler` + `NoRedirect`) y lee hasta 1 MiB.
2. `Form('password')`. Si `OSError` o `ValueError`, o no hay formularios: `{}`.
3. Usa el primer formulario con contraseña: `user = forms[0][3]`, `password = forms[0][4]`.
4. Solo devuelve nombres **distintos** de los por defecto: si `user` existe y no es `'username'`, `proxy_authentication_username_field = user`; si `password` existe y no es `'password'`, `proxy_authentication_password_field = password`.

Nunca devuelve credenciales: solo nombres de campo (comentario de línea 361). No sigue redirecciones: si la URL es una redirección, devuelve `{}`.

### 7.6 `diagnose(root, network=None, resolve=system_proxy)` (líneas 380-424)

Objetivo: cuando la comprobación falla, encontrar una ruta que funcione y proponer cambios en `config.properties`. **No guarda nada** (comentario de línea 383). Lo guarda `apply` tras confirmación del usuario.

Orden de ejecución:

1. `network = network or Network(root)`. Puede lanzar `YitoError` o `ValueError`.
2. `if network.check(): return Diagnosis('ok')` (sin cambios). Nota: `check()` con `log=True` por defecto.
3. `if not network.local_network(): return Diagnosis('offline')`.
4. `configured = 'https_proxy' in config or 'http_proxy' in config` (presencia de la clave, aunque esté vacía).
5. Lista de **candidatos** `(proxy, source, no_proxy)`, en este orden:
   1. Si `configured`: `(config.get('https_proxy', config.get('http_proxy', '')), 'config.properties', None)`.
   2. `found = resolve(network.url)`. Si `found` y su URL no coincide con ningún proxy ya en la lista: `(found['url'], found['source'], found['no_proxy'])`.
   3. Si ningún candidato tiene `proxy == ''` y (`configured` o no hay `found`): `('', 'direct', '' if configured else None)`.
6. Para cada candidato, en orden: `result, kind = classify(network, proxy)`; se guarda `(kind, proxy, source, no_proxy, result)`. Si `kind == 'ok'`: se corta el bucle.
7. Ranking (línea 407): `ok` 0, `portal` 1, `auth` 2, `blocked` 3, `unreachable` 4. Se elige el mínimo; en empate gana el primero de la lista.
8. `Diagnosis(kind, proxy, source, result.status, result.authenticate)`.
9. Cambios de proxy (líneas 410-411): solo si `no_proxy is not None` **y** `kind` es `'ok'` o `'portal'`: `changes = {http_proxy: proxy, https_proxy: proxy, no_proxy: no_proxy}`.
   - Para el candidato `'direct'` con `configured`, `no_proxy = ''`, así que el cambio es `http_proxy=`, `https_proxy=`, `no_proxy=`: quita el proxy configurado.
   - Para el candidato de config, `no_proxy` es `None`: no se proponen cambios de proxy (ya estaban).
10. Si `kind == 'portal'` (líneas 412-423):
    - `diagnosis.portal = validate_url(urljoin(result.url, result.location)).geturl()`. Si `YitoError` (por ejemplo, la redirección lleva credenciales): `kind = 'blocked'`, se vacían `changes` y se devuelve.
    - Si `config` **no** tiene `proxy_authentication_url`: `changes['proxy_authentication_url'] = portal` y `changes.update(portal_fields(portal, proxy, timeout))`.
    - Si `enable_proxy_authentication` no es `'true'` (sin distinguir mayúsculas): `changes['enable_proxy_authentication'] = 'true'`.
    - Si `proxy_authentication_url` ya existe, no se toca ni se detectan nombres de campo (Duda D6).

El orden de inserción de `changes` se refleja en la pantalla de `NetworkSetupDialog`: `http_proxy`, `https_proxy`, `no_proxy`, `proxy_authentication_url`, `proxy_authentication_username_field`, `proxy_authentication_password_field`, `enable_proxy_authentication`.

Resultado en `kind == 'auth'`: no hay cambios (test `test_network_setup.py:91-96`).

## 8. Formulario de contraseñas: `prompt_credentials` (líneas 436-479)

Muestra un diálogo WinForms en un proceso `powershell.exe` separado y devuelve las credenciales. Es el único lugar donde el usuario escribe la contraseña del portal.

Comportamiento:

1. `script` = el script de la línea 437 a 469 (copia literal abajo). Si `failed` es verdadero, se sustituye la etiqueta `Usuario y contraseña del portal. Cancelar continúa sin conexión.` por `No se pudo verificar Internet. Reintenta o cancela para continuar.` (línea 472).
2. Localización (líneas 473-477): `localize_script(script, ...)` sustituye cada **literal de PowerShell completo** (entre comillas simples, con `''` escapado) por `translate(texto)`. Las cadenas son:

| Literal original (español) | Dónde aparece |
|---|---|
| `MITO - Autenticación de red` | Título de la ventana (`$f.Text`, línea 440) |
| `Usuario y contraseña del portal. Cancelar continúa sin conexión.` | Etiqueta (`$l.Text`, línea 446) |
| `No se pudo verificar Internet. Reintenta o cancela para continuar.` | Etiqueta cuando `failed` (línea 472) |
| `Conectar` | Botón de aceptar (línea 454) |
| `Cancelar` | Botón de cancelar (línea 458) |

   No hay etiquetas para los campos de usuario ni de contraseña. El `TextBox` de la contraseña usa `UseSystemPasswordChar` (línea 452).
3. `subprocess.run(['powershell.exe', '-NoLogo', '-NoProfile', '-STA', '-Command', script], capture_output=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)`. **Sin `timeout`.**
4. Devuelve `json.loads(stdout)` si `returncode == 0` y hay salida; si no, `None`. Se devuelve `None` al cancelar (`ShowDialog()` distinto de `'OK'`, el script no imprime nada).
5. La contraseña se pone a vacío en el control (`$p.Clear()`, línea 468) y el formulario se libera (`Dispose`).

Script literal (`network.py:438-469`, sin cambios):

```powershell
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object Windows.Forms.Form
$f.Text = 'MITO - Autenticación de red'
$f.ClientSize = New-Object Drawing.Size(420,190)
$f.StartPosition = 'CenterScreen'
$f.FormBorderStyle = 'FixedDialog'
$f.MaximizeBox = $false
$l = New-Object Windows.Forms.Label
$l.Text = 'Usuario y contraseña del portal. Cancelar continúa sin conexión.'
$l.SetBounds(15,10,390,35)
$u = New-Object Windows.Forms.TextBox
$u.SetBounds(15,50,390,25)
$p = New-Object Windows.Forms.TextBox
$p.SetBounds(15,85,390,25)
$p.UseSystemPasswordChar = $true
$ok = New-Object Windows.Forms.Button
$ok.Text = 'Conectar'
$ok.SetBounds(195,135,100,30)
$ok.DialogResult = 'OK'
$cancel = New-Object Windows.Forms.Button
$cancel.Text = 'Cancelar'
$cancel.SetBounds(305,135,100,30)
$cancel.DialogResult = 'Cancel'
$f.Controls.AddRange(@($l,$u,$p,$ok,$cancel))
$f.AcceptButton = $ok
$f.CancelButton = $cancel
[Console]::OutputEncoding = New-Object Text.UTF8Encoding
if ($f.ShowDialog() -eq 'OK') {
    @{username=$u.Text;password=$p.Text} | ConvertTo-Json -Compress
}
$p.Clear()
$f.Dispose()
```

El script se pasa como argumento de `-Command`. No contiene credenciales: estas viajan solo por la salida estándar en JSON.

## 9. Guardado de la configuración: `apply` y validación

`apply(engine, changes)` (líneas 427-433):

```python
configuration = Configuration(engine)
values = dict(configuration.values)
values.update(changes)
configuration.save(values.items())
```

Escribe a través de `configuration.py` (no escribe el archivo a mano). Conserva comentarios y claves ajenas (test `test_network_setup.py:106-112`). Puede lanzar `OSError` y `YitoError`.

Validación en `configuration.py` para las claves de red (líneas 12-31 y 65-83):

| Clave | Valor por defecto | Restricción |
|---|---|---|
| `enable_proxy_authentication` | `false` | `true` o `false` |
| `proxy_authentication_url` | `''` | URL HTTP(S) sin credenciales, solo si no está vacía |
| `proxy_authentication_username_field` | `username` | Texto libre |
| `proxy_authentication_password_field` | `password` | Texto libre |
| `proxy_authentication_require_https` | `true` | `true` o `false` |
| `proxy_authentication_form_field.<n>` | — | Nombre no vacío (`Falta el nombre del campo del formulario`) |
| `http_proxy`, `https_proxy` | `''` | URL HTTP(S) sin credenciales, solo si no está vacía |
| `no_proxy` | `''` | Texto libre, **sin validar** |
| `network.check_url` | `https://www.gstatic.com/generate_204` | URL HTTP(S) sin credenciales, siempre |
| `network.check_status` | `204` | Solo dígitos (`isdecimal`, que en el original acepta también dígitos Unicode; Rust exige ASCII, ver `docs/PARITY.md`) y entre 100 y 599 |
| `network.timeout_seconds` | `4` | Número finito, mayor que 0 y hasta 60 |
| `network.poll_seconds` | `20` | **Sin validar**; el sondeo lo interpreta la UI (ver `network_ui.md`, Duda D9) |

## 10. Interceptor de arranque: `intercept(root, environment=None)` (líneas 482-502)

Se llama desde `__main__.py`:

- `host-run`: antes de lanzar, siempre (`__main__.py:194-195`).
- `run` sin `--bin`: antes de lanzar (`__main__.py:309-310`).

Algoritmo:

1. `network = Network(root, environment)`.
2. Si `network.check()` es verdadero, o `not network.enabled`, o `not network.local_network()`: `return` (sin preguntar).
3. `failed = False`; `translate = Translator.from_root(root)`.
4. Bucle:
   - `credentials = prompt_credentials(failed, translate=translate)`. Si `None`: `return` (cancelar = continuar sin conexión).
   - `try: if network.authenticate(user, pass): return` `finally: credentials.clear()` (borra el dict, no las cadenas).
   - `failed = True` y se repite.
5. `except (OSError, ValueError, YitoError): return`. Nunca impide el arranque (comentario de línea 483).

El bucle no tiene límite de intentos: el usuario puede seguir reintentando o cancelar.

Nota: en el paso 2, `check()` escribe `network.check` en el log cuando hay excepción de red, así que cada arranque sin Internet deja una línea de log.

## 11. Invariantes

1. Usuario y contraseña del portal **nunca** se escriben en `config.properties`, en el log ni en los pendientes de telemetría. Solo viven en la secuencia de `authenticate` (diccionario de campos y cuerpo del POST) y en el diálogo (`prompt_credentials` o `ProxyDialog`). Lo comprueban `test_network.py:72-74`, `:109-111`, `:125-128` y `test_bootstrap_proxy.py:159`.
2. Los logs de portal (`network.authenticate`) solo llevan el origen `[scheme, netloc]` y el código de estado. Nunca la URL completa (que puede llevar `magic=` o tokens). Lo comprueba `test_network.py:126-128`.
3. `check()` usa un cliente sin cookies. Una sesión de portal no es prueba de Internet por sí sola: se verifica con un cliente nuevo (línea 234-235).
4. Ninguna petición de red de este módulo sigue redirecciones (`NoRedirect` en `check`, `probe`, `portal_fields`, `redirect` y `authenticate`).
5. El POST solo va al mismo esquema y netloc que `proxy_authentication_url` (línea 215), y por defecto solo por HTTPS (línea 217).
6. `diagnose` no escribe nada: solo propone `changes`. Lo guarda `apply` tras la confirmación del usuario en `NetworkSetupDialog`.
7. `portal_fields` devuelve solo nombres de campo, nunca valores (comentario de línea 361).
8. Un proxy de `config.properties` con credenciales embebidas o con un esquema que no sea HTTP(S) produce `YitoError` antes de cualquier conexión.
9. Un valor vacío de `http_proxy` o `https_proxy` en la config anula el proxy heredado (línea 41).
10. `intercept` nunca lanza excepciones esperadas al llamador (líneas 501-502).
11. `Form` no guarda ningún valor de contraseña del formulario (guarda el atributo `value` del input, que en un `type=password` normalmente está vacío; si el portal publica una contraseña precargada, se guarda en el dict en memoria). Ver Posibles fallos F9.

## 12. Casos límite y errores

- `config.properties` ausente: `properties` devuelve `{}`. `Network` usa los valores por defecto. `diagnose` no ve claves de proxy (`configured = False`).
- `config.properties` con BOM UTF-8: se ignora gracias a `utf-8-sig`.
- `config.properties` en otra codificación (por ejemplo ANSI): `UnicodeDecodeError` (es `ValueError`). `Network(...)` lo deja pasar; `intercept` lo silencia; la UI (`ui.py:1006`) lo registra como `network.check` con `capture`. Ver Duda D1.
- `config.properties` que es un directorio: `OSError` sin capturar en `properties`.
- `network.timeout_seconds` no numérico (`abc`): `ValueError` desde `float()` (línea 121), no `YitoError`. Ver Posibles fallos F2.
- `network.timeout_seconds` = `0`, `-1`, `nan`, `inf` o `61`: `YitoError` (`not 0 < t <= 60`).
- `network.check_status` no entero (`abc`): `ValueError` desde `int()` (línea 126).
- `network.check_status` de `200` con un `check_url` que devuelve `204`: válido para `configuration.py`, pero `check()` compara `status == expected` y siempre da `False`. No hay aviso.
- `network.check_url` con credenciales o sin host: `YitoError` en `__init__`.
- `proxy_authentication_url` vacío y `enable_proxy_authentication=true`: `enabled` es `False` (línea 130). `intercept` no pregunta.
- `proxy_authentication_url` inválido: `YitoError` en `__init__`, incluso con `enable_proxy_authentication=false`.
- Redirección del portal a otro host que no es el del `auth_url`: `portal_page` la sigue hasta 5 saltos sin confiar en ella. Si no se llega al origen del portal, se devuelve la URL guardada.
- Portal que devuelve 400 (token caducado): `Portal page unavailable`, `status=400`, `False` (test `test_network.py:113-128`).
- Formulario sin campo de contraseña: se hace POST a la propia página con `fields = {}` (línea 212). Puede que el portal no acepte la petición.
- `action` del formulario relativo: `urljoin` con la URL de la página (línea 209).
- `action` con `&amp;` en el atributo: decodificado a `&` (test de bootstrap, `test_bootstrap_proxy.py:112-113`).
- Input sin valor (`<input name="x" value>`): `attrs.get('value', '')` devuelve `None`, no `''`. Se envía el texto `None` en el POST. Ver Posibles fallos F8.
- Redirección 307 o 308 en el POST de login: se trata como rechazo (`Portal rejected the login POST`), no como éxito.
- Un `proxy` que responde 407 en HTTP plano: `auth`. Sin cabecera `Proxy-Authenticate`: `authenticate = ''` y la UI usa `Basic` por defecto (`ui.py:1081`).
- `probe` a través de un proxy que no responde: `timeout` del `OSError` → `Probe(0)`, `unreachable`.
- `system_proxy` con PowerShell colgado: `timeout=30` → `None`, tras 30 segundos de espera sin mensaje de progreso.
- Un `ProxyOverride` con `<local>` y sin entradas de dominio: `no_proxy` tiene solo `localhost,127.0.0.1,::1`.

## 13. Casos de test del original

Los 15 tests de los tres archivos. Los datos son exactos.

### 13.1 `test_network.py`

| Test | Línea | Qué comprueba | Entrada exacta | Salida esperada |
|---|---|---|---|---|
| `test_proxy_routing_and_explicit_empty` | 23 | Mapeo de claves, vacío que anula, y validación de credenciales | Config: `http_proxy=http://proxy.example:8080`, `https_proxy=`, `no_proxy=localhost`. Entorno: `{'http_proxy':'old','HTTPS_PROXY':'old'}` | `env['HTTP_PROXY'] == 'http://proxy.example:8080'`; `env['HTTPS_PROXY'] == ''`; `'http_proxy' not in env`; `proxies['http'] == 'http://proxy.example:8080'`; `proxies['no'] == 'localhost'`. Con config `http_proxy=http://user:secret@proxy.example`: `YitoError` |
| `test_portal_post_hidden_fields_and_independent_connectivity` | 37 | Login completo contra servidor local: campos ocultos, redirección, HTTPS obligatorio y secretos fuera de disco | Servidor local: `/login` (form con `token=abc` oculto, `password`), `/redirect` (302 a `/login`), resto 204 si autenticado, 200 si no. POST válido solo con `{token:abc, username:test-user, password:test-password, extra:value}`. Config: `http_proxy=`, `https_proxy=`, `no_proxy=127.0.0.1`, `check_url=…/check`, `timeout_seconds=1`, `enable_proxy_authentication=true`, `proxy_authentication_url=…/login`, `proxy_authentication_form_field.extra=value` | `check()` → `False`. `authenticate` sin `require_https=false` → `YitoError`, POST = 0. Con `require_https=false` → `True`, POST = 1. `url=/redirect` → `check()` `False`. Ningún archivo del directorio contiene `test-password` |
| `test_portal_url_is_rediscovered_because_saved_redirects_expire` | 76 | La URL del portal se redescubre aunque la guardada tenga un token caducado | Servidor: `/check` → 302 a `/portal?magic=N` (N incrementa por sesión); `/portal?magic=N` → 302 a `/fgtauth?magic=N`; `/fgtauth?magic=N` → 200 con form `action="/"`, `magic` oculto, `username`, `password`. POST válido solo con `{magic:[N], username:[test-user], password:[test-password]}`. Otro caso → 400. Config: `proxy_authentication_url=…/portal?magic=expired`, `require_https=false` | `authenticate` → `True`; `len(posts) == 1`; ningún archivo contiene `test-password` |
| `test_failed_portal_login_is_logged_without_secrets` | 113 | Fallo registrado sin token ni contraseña | Servidor: cualquier GET → 400. Config: `proxy_authentication_url=…/portal?magic=secret-token`, `require_https=false` | `authenticate` → `False`. Los logs (todos los archivos salvo `config.properties`) contienen `Portal page unavailable`, no contienen `secret-token` ni `test-password` |
| `test_interceptor_cancellation_offline_and_success` | 130 | Decisión de `intercept`: no preguntar, cancelar, o autenticar con éxito | `Network` y `prompt_credentials` mockeados. Casos: (a) `check=False`, `local=True`, `enabled=True`, prompt `None`; (b) `check=True`; (c) `check=False`, `local=False`; (d) prompt `{username:'test', password:'secret'}` | (a) devuelve `None`, prompt una vez, `authenticate` no llamado; (b) prompt no llamado; (c) prompt no llamado; (d) `authenticate('test','secret')` una vez, y el dict queda `{}` |

### 13.2 `test_network_setup.py`

Todos los tests de `DiagnoseTests` parchean `Network.check` a `False` y `Network.local_network` a `True` (líneas 54-57). La config base es `# comentario\nnetwork.check_url=http://check.test/generate_204\nnetwork.timeout_seconds=2\n`.

| Test | Línea | Qué comprueba | Entrada exacta | Salida esperada |
|---|---|---|---|---|
| `test_no_proxy_from_windows_override` | 62 | Conversión de `ProxyOverride` | `no_proxy_from_override('<local>;*.corp.example;intranet;10.*;a/b')` | `'localhost,127.0.0.1,::1,.corp.example,intranet'` |
| `test_stale_configured_proxy_is_replaced_by_the_windows_route` | 65 | Un proxy configurado caducado se sustituye por la ruta del sistema | Config: `http_proxy=http://127.0.0.1:1`, `https_proxy=http://127.0.0.1:1`. Proxy local que responde 204. `resolve` devuelve `{url: <proxy>, source:'pac', no_proxy:'localhost'}` | `kind='ok'`, `proxy=<url del proxy local>`, `source='pac'`; `changes == {http_proxy:<url>, https_proxy:<url>, no_proxy:'localhost'}` |
| `test_portal_behind_detected_proxy_proposes_portal_keys_without_credentials` | 73 | Portal detrás del proxy detectado: propone claves de portal sin credenciales | Config base sin claves de proxy. Proxy local: cualquier ruta distinta de `/login` → 302 a `http://portal.test/login`; `/login` → 200 con form `token` (oculto), `user` (texto), `pass` (password). `resolve` devuelve `{url: <proxy>, source:'wpad', no_proxy:'localhost'}` | `kind='portal'`, `portal='http://portal.test/login'`; `changes == {http_proxy:<url>, https_proxy:<url>, no_proxy:'localhost', proxy_authentication_url:'http://portal.test/login', proxy_authentication_username_field:'user', proxy_authentication_password_field:'pass', enable_proxy_authentication:'true'}` |
| `test_configured_portal_needs_no_changes` | 85 | Portal ya configurado: no se propone nada | Config: `http_proxy=<proxy>`, `https_proxy=<proxy>`, `enable_proxy_authentication=true`, `proxy_authentication_url=http://portal.test/login`. `resolve` devuelve `None` | `(kind, changes) == ('portal', {})` |
| `test_proxy_407_is_reported_and_not_saved` | 91 | 407 se informa y no se guarda | Proxy local que responde 407 con `Proxy-Authenticate: Negotiate` y `Proxy-Authenticate: NTLM`. `resolve` devuelve `{url:<proxy>, source:'manual', no_proxy:''}` | `(kind, status, authenticate) == ('auth', 407, 'NTLM, Negotiate')`; `changes == {}` |
| `test_offline_and_online_short_circuit` | 98 | Sin red o con Internet, no se consulta el sistema | (a) `local_network` → `False`; (b) `check` → `True` | (a) `kind='offline'`; (b) `kind='ok'`; `resolve` no llamado |
| `test_apply_keeps_comments_and_other_keys` | 106 | Guardado conserva comentarios y claves ajenas | Config base + `terminal.defaultShell=powershell`. `apply` con `{https_proxy: 'http://proxy.example:8080', no_proxy: 'localhost'}` | El texto conserva `# comentario`; `terminal.defaultShell == 'powershell'`; `https_proxy == 'http://proxy.example:8080'` |
| `NetworkSetupUITests.test_detected_portal_is_saved_then_credentials_are_asked_for_the_session` | 116 | Flujo completo de la UI: cancelar no guarda; guardar escribe y pide credenciales | Diagnóstico `portal` mockeado, cambios de la sección 7.6. Pantalla `(130, 42)` | Ver `network_ui.md`, sección 7. Resumen: cancelar no crea `config.properties`; guardar escribe las claves y abre `ProxyDialog`; escape cierra `ProxyDialog` |

### 13.3 `test_bootstrap_proxy.py`

Estos dos tests ejecutan PowerShell real contra un proxy local. No se portan a Python: su comportamiento se porta en el bootstrap de Rust (`bootstrap.md`, sección 5.7). Aquí se resumen sus datos esperados:

| Test | Línea | Qué comprueba | Datos esperados |
|---|---|---|---|
| `test_system_proxy_no_proxy_form_and_properties_helpers` | 81 | `Get-YitoSystemProxy` (PAC, manual, puerto 80, directo), `ConvertTo-YitoNoProxy`, `Get-YitoPortalForm`, `Set-YitoProperties`, `Get-YitoProxyKind` | `noProxy` = `localhost,127.0.0.1,::1,.corp.example,intranet,corp.example`; `pacUrl` = `http://10.1.2.3:8080`; `pacSource` = `script PAC http://pac.example/proxy.pac`; `pacNoProxy` = `localhost,127.0.0.1,::1,.corp.example`; `manualSource` = `configuración manual de Windows`; `port80` = `http://10.1.2.3:80`; `direct` = `true`; formulario: `action` = `/a?x=1&y=2`, `user` = `login`, `password` = `pwd`, `csrf` = `t&k`; `kinds` = `['ok','auth','portal','unreachable','blocked']`; el archivo queda `b'# comment\nhttp_proxy=http://p:80\nbucket.main=x\nhttps_proxy=http://p:80\n'` |
| `test_probe_and_portal_login_through_local_proxy` | 119 | Sonda, login de portal con campos ocultos, rechazo de HTTP sin permiso, origen distinto y contraseña incorrecta | `ok`, `auth` (cabeceras `NTLM, Negotiate`), `portal` (`Location: http://captive.invalid/login?from=check`); login por HTTPS: rechazado (`HTTPS`); origen `foreign.invalid`: `otro origen`; contraseña `wrong`: `False`; contraseña `secret-pass`: `True`. Solo llegan 2 POST válidos: el último es `{csrf:['tok&en'], user:['ana'], pass:['secret-pass']}` | Igual que la columna de la izquierda; el log no contiene `secret-pass` |

## 14. Dependencias

Otros módulos del original:

- `mito.i18n.tr` (línea 2): mensajes de `YitoError` en español.
- `mito.i18n.Translator` (línea 489): traduce las etiquetas de `prompt_credentials`.
- `mito.storage.YitoError` (línea 15).
- `mito.diagnostics`: `write`, `message` y `scrub` (importados dentro de las funciones: líneas 142, 154, 240). Ver `diagnostics.md`.
- `mito.configuration.Configuration` (línea 429, dentro de `apply`). Ver `configuration.md`.
- `mito.native_ui.localize_script` (línea 473). Ver `native_ui.md`.
- `mito.engine` llama a `proxy_environment` (`engine.py:548` y `:556`).
- Biblioteca estándar: `ctypes`, `http.cookiejar`, `html.parser`, `json`, `os`, `re`, `subprocess`, `urllib.error`, `urllib.parse`, `urllib.request`, `dataclasses`.

## 15. Notas para Rust

- Cliente HTTP: `reqwest` (blocking o async) con `Proxy::all` para la ruta explícita y `no_proxy`. Para una clave vacía en config, usar `ClientBuilder::no_proxy()` para anular el proxy del sistema, igual que el original.
- `reqwest` no lee el registro de Windows ni PAC. El equivalente de `system_proxy` (PowerShell) puede sustituirse por una llamada nativa a WinHTTP (`WinHttpGetProxyForUrl`) o mantenerse como subproceso. Ver Duda D2.
- Sin redirecciones automáticas: `reqwest::redirect::Policy::none()` en todos los clientes de este módulo.
- Cookies del portal: `reqwest::cookie::Jar` en un cliente nuevo por `authenticate`, descartado al terminar.
- Parseo de formularios: `scraper` o `html5ever` con las mismas reglas de la sección 5. Las entidades HTML se decodifican igual.
- `no_proxy`: `reqwest::NoProxy::from_string` no implementa `*.dominio` ni `10.*`. Ver Duda D4 y Posibles fallos F5.
- Secretos: `secrecy::SecretString` para la contraseña, `zeroize` para la copia del diálogo. Ninguna credencial pasa por `Debug` ni por el log.
- Diálogo de credenciales: en Rust se hace con la UI propia (`network_ui.md`), no con PowerShell. La lista de etiquetas localizadas de la sección 8 pasa a ser el catálogo de traducción de `mito-tui`.

## Dudas

- **D1. Codificación de `config.properties`.** El original exige UTF-8 con BOM opcional. Si el archivo está en ANSI (habitual en Windows con editores antiguos), falla con `UnicodeDecodeError` y `intercept` lo silencia (líneas 22, 501-502). ¿Rust debe aceptar la codificación del sistema, o fallar con un mensaje claro en la UI?
- **D2. Fuente de proxy en Windows.** El original usa dos mecanismos distintos: `urllib` con el registro manual (`ProxyEnable` y `ProxyServer`, sin PAC ni WPAD) para las descargas, y PowerShell con .NET (PAC, WPAD y manual) para la sonda de `diagnose`. ¿Rust debe usar una única fuente (WinHTTP, por ejemplo) para ambos casos, o mantener la separación?
- **D3. Prioridad de la lista de exclusión.** Hoy `no_proxy` de la config no es la lista exclusiva: se suman `ProxyOverride` del registro (y `NO_PROXY` del entorno del proceso). ¿Debe Rust usar solo la lista de la config (más predecible), o mantener la suma (igual que el original)? Ver Posibles fallos F4.
- **D4. Comodines en `no_proxy` escritos a mano.** La biblioteca estándar no reconoce `*.dominio` ni `10.*` en `no_proxy` (solo `.dominio`). Un usuario que escriba `*.empresa.local` en la config no obtendrá la exclusión y no recibirá aviso. ¿Rust debe aceptar `*.dominio` (equivalente a `.dominio`), rechazarlo con un mensaje, o replicar el comportamiento silencioso?
- **D5. Etiqueta de origen `pac`.** `SYSTEM_PROXY_SCRIPT` marca `pac` si existe `AutoConfigURL`, aunque la ruta real venga de WPAD o de un manual sin `ProxyEnable`. ¿La etiqueta debe reflejar la configuración leída (como ahora) o la ruta usada?
- **D6. Detección de campos de portal cuando la URL ya está configurada.** `diagnose` no busca nombres de campo (líneas 419-421) si `proxy_authentication_url` ya existe. Si el portal cambia sus campos, el usuario debe corregir `proxy_authentication_*_field` a mano. ¿Debe Rust volver a detectarlos en cada diagnóstico?
- **D7. Mensaje del `YitoError` de `authenticate` en la UI.** Los errores de HTTPS obligatorio y de origen distinto son `YitoError` con texto en español (líneas 216 y 218), pero `ProxyDialog` los sustituye por un mensaje genérico (`ui`: `network_ui.py:105-110`). ¿Deben mostrarse en el diálogo para que el usuario sepa qué configurar? Ver Posibles fallos F1.
- **D8. Solicitud de credenciales en cada arranque.** Con `enable_proxy_authentication=true` y sin Internet, `host-run` y `run` sin `--bin` preguntan cada vez que se lanza una aplicación gestionada, sin límite de frecuencia. ¿Es el comportamiento deseado, o debe preguntarse una vez por sesión de MITO?
- **D9. Intervalo de sondeo.** `network.poll_seconds` se lee una sola vez, al arrancar la UI (`ui.py:899-911`). Cambiarlo en la configuración no reinicia el temporizador. ¿Debe Rust reaplicarlo al guardar?
- **D10. Cancelar sin pistas.** `prompt_credentials` devuelve `None` tanto si el usuario cancela como si PowerShell falla (línea 479). ¿Debe Rust distinguir ambos casos para mostrar un error?

## Posibles fallos del original

- **F1. Errores de `authenticate` que no llegan al usuario.** Los `YitoError` de HTTPS obligatorio y de origen distinto (líneas 216 y 218) se propagan. `ProxyDialog.authenticate` los captura con `except Exception` (`network_ui.py:106-110`) y muestra solo el texto genérico de la línea 121. `intercept` los silencia (línea 501) y el usuario no sabe por qué no se conectó. El test `test_network.py:62-63` confirma que se lanzan.
- **F2. Excepciones no esperadas al guardar valores numéricos inválidos.** `Network.__init__` lanza `ValueError` (no `YitoError`) si `network.timeout_seconds` o `network.check_status` no son números (líneas 121 y 126). `ui.network_diagnosis.login()` solo captura `YitoError` (`ui.py:1051`), así que la excepción sale de un callback de Textual.
- **F3. Repetición del error de configuración en el sondeo.** `Network.__init__` lanza el `ValueError` de F2 en cada construcción. El sondeo periódico de la UI construye `Network` en cada tick y registra el error cada vez. El detalle está en `network_ui.md`, Posibles fallos F3.
- **F4. `no_proxy` de la config no es la lista exclusiva.** `ExplicitProxyHandler.proxy_open` (línea 74) llama a `super().proxy_open`, que a su vez consulta `proxy_bypass(req.host)` de la biblioteca estándar. Esa función usa `getproxies_environment()` (variables `*_proxy` del proceso) si existen, o `ProxyOverride` del registro si `ProxyEnable=1`. Con `no_proxy=` vacío en la config, un host de `ProxyOverride` (o de `NO_PROXY` del entorno) sigue yendo directo. Verificado en el código de la biblioteca estándar.
- **F5. Comodines de `no_proxy` escritos a mano.** `proxy_bypass_environment` no reconoce `*.dominio` ni `10.*` (verificado en la biblioteca estándar: se comparan como texto literal). `configuration.py` no valida `no_proxy` (línea 27). El fallo es silencioso. Solo `no_proxy_from_override` quita el `*.` (línea 275), así que el problema aparece al escribir la clave a mano.
- **F6. Comparación de origen distinta en dos sitios.** `origin()` pasa a minúsculas (línea 55), pero la comprobación de la línea 215 compara `parsed.scheme` y `parsed.netloc` sin normalizar. `http://Portal.empresa.local/login` configurado y un formulario `http://portal.empresa.local/submit` dan «otro origen» aunque sean el mismo servidor. Lo mismo ocurre con `:80` explícito frente a implícito.
- **F7. La sonda ignora la exclusión explícita del proxy.** `probe` usa `ProxyHandler` de la biblioteca estándar (línea 321), no `ExplicitProxyHandler`. Su `proxy_open` también consulta `proxy_bypass(req.host)` (sección 3.7). Si el host de comprobación está en `ProxyOverride`, la sonda va directa aunque se esté probando un candidato proxy. El candidato puede marcarse `ok` sin haberse usado, y `diagnose` propondría `http_proxy` con un proxy no probado.
- **F8. Valor `None` en el POST.** `Form` guarda `attrs.get('value', '')` (línea 102). Un atributo `value` sin valor (`<input name="x" value>`) da `None`, y `urlencode` lo envía como el texto `None`.
- **F9. Contraseña precargada en el formulario.** `Form` guarda el `value` de los inputs `type=password` (línea 102). Si el portal publica una contraseña en el HTML, se queda en `fields`. Se sobrescribe con la del usuario (línea 221), pero si el campo tiene otro nombre (`proxy_authentication_password_field` distinto), el valor publicado se envía igualmente.
- **F10. `CREATE_NO_WINDOW` sin comprobar la plataforma.** `prompt_credentials` usa `subprocess.CREATE_NO_WINDOW` directamente (línea 478). En sistemas que no son Windows lanza `AttributeError`, que `intercept` no captura (líneas 501-502). `system_proxy` sí usa `getattr` (línea 289).
- **F11. Sin límite de tiempo en el diálogo de credenciales.** `prompt_credentials` ejecuta PowerShell sin `timeout` (línea 478). Un diálogo que no aparezca (por ejemplo, detrás de otra ventana en otro escritorio) bloquea el arranque de la aplicación indefinidamente.
- **F12. `system_proxy` solo lee HKCU.** `SYSTEM_PROXY_SCRIPT` lee `HKCU:` (línea 250). Las políticas de grupo que fijan el proxy en `HKLM` no se ven en la lista de exclusión (`ProxyOverride`), aunque `GetSystemWebProxy` sí las use para elegir el proxy.

## Decisiones del port (revisión de Opus)

Se corrigen (política del proyecto: corregir y documentar en `docs/PARITY.md`):

- **F1**: el texto de los errores de configuración de `authenticate` (HTTPS obligatorio,
  otro origen) se muestra en `ProxyDialog` en lugar del mensaje genérico, y `intercept`
  lo registra en `logs/mito.log` (operación `network.authenticate`) en vez de callarlo.
- **F2/F3**: la configuración de red inválida es un error de dominio con mensaje; la
  interfaz lo captura siempre. El sondeo silencioso (`quiet`) no registra el mismo error
  en cada intervalo.
- **F5**: en `no_proxy`, `*.dominio` equivale a `.dominio` (como `no_proxy_from_override`).
- **F6**: la comprobación de origen del formulario usa `origin()` (esquema y host en
  minúsculas, puerto por defecto implícito), igual que el resto del módulo.
- **F7**: `probe` usa siempre el proxy candidato indicado; no aplica las exclusiones del
  sistema, para que un candidato no se dé por bueno sin haberse probado.
- **F8**: un atributo `value` sin valor cuenta como `""`.
- **F9**: el `value` publicado en un input de contraseña no se envía nunca (se toma `""`).

Se mantiene el comportamiento del original:

- D1 (codificación: la de `mito-core`), D2 (descargas con `getproxies`; `diagnose` con
  PowerShell/.NET para PAC y WPAD), D3/F4 (las exclusiones del sistema se suman a
  `no_proxy`), D5, D6, D8, D9, D10 (se registra el fallo de PowerShell), F11 (sin límite de
  tiempo: el usuario puede tardar), F12. F10 no aplica.
