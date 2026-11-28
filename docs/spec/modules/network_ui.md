# network_ui.py y botón INTERNET de ui.py → mito_tui (red)

Origen:
- `core/app/mito/network_ui.py` (122 líneas): `source_label`, `NetworkSetupDialog`, `ProxyDialog`.
- `core/app/mito/ui.py`: botón `#network-status` (línea 847), arranque y sondeo (895-917), comprobación (998-1023), diagnóstico (1025-1033), decisión tras el diagnóstico (1035-1085), pulsación (1279-1302), comprobación tras guardar la configuración (1508-1518).
- `core/app/mito/__main__.py`: `intercept` antes de `host-run` (192-195) y de `run` sin `--bin` (307-310). Ver `network.md`, sección 10.
- Test: `core/tests/test_network_setup.py`, clase `NetworkSetupUITests` (líneas 115-145). Es el único test de UI de red.

Contrato de la lógica (`network.py`, `diagnose`, `apply`, `Network.authenticate`) en `network.md`. Este documento cubre la presentación, el flujo entre pantallas y los textos.

Convención: `ui.py:NN` y `network_ui.py:NN` son líneas del original.

## 1. Responsabilidad

Mostrar en la barra de la aplicación el estado de Internet (botón INTERNET), repintarlo periódicamente sin molestar, y, al pulsarlo, guiar al usuario por el diagnóstico, la confirmación de los cambios de `config.properties` y, si el portal lo exige, la solicitud de usuario y contraseña para la sesión.

## 2. API pública

### 2.1 `network_ui.py`

| Elemento | Línea | Entradas | Salida | Notas |
|---|---|---|---|---|
| `source_label(source)` | 8 | `source: str` | `str` | Ver sección 8.1 |
| `NetworkSetupDialog(diagnosis)` | 17 | `Diagnosis` | `ModalScreen` que se cierra con `bool` | `dismiss(True)` = guardar; `dismiss(False)` = cancelar |
| `ProxyDialog(network)` | 54 | `Network` | `ModalScreen` que se cierra con `bool` | `dismiss(True)` = autenticado; `dismiss(False)` = cancelado |

Ambas clases son `ModalScreen` de `localized_ui` (línea 3) y usan `Vertical`, `Button`, `Input` y `Static` de Textual.

### 2.2 `ui.py` (puntos de entrada del botón)

| Método | Línea | Hilo | Grupo de worker | Uso |
|---|---|---|---|---|
| `on_mount` | 886 | principal | — | Llama a `check_network()` (898) y `start_network_poll()` (899) |
| `start_network_poll()` | 903 | principal | — | Crea el temporizador periódico (sección 5) |
| `poll_network()` | 913 | principal | — | Callback del temporizador |
| `check_network(prompt=False, quiet=False)` | 998 | worker | `network`, `exclusive=True` | Comprueba Internet y actualiza el botón |
| `network_result(network, online, local, prompt)` | 1010 | principal | — | Pinta el botón y decide si diagnosticar |
| `diagnose_network()` | 1025 | worker | `network`, `exclusive=True` | Llama a `network.diagnose` |
| `network_diagnosis(diagnosis)` | 1035 | principal | — | Decide qué pantalla mostrar (sección 6.3) |
| `on_button_pressed` | 1279 | principal | — | `network-status` → `check_network(prompt=True)` (1301-1302) |

## 3. Botón INTERNET: texto, color y estados

El botón es un `Button` con `id='network-status'` (`ui.py:847`). Su texto inicial es `Comprobando Internet…` (carácter de elipsis `…`, U+2026), con la variante por defecto de Textual (`default`).

Cada resultado de `network_result` (`ui.py:1010-1023`) fija la etiqueta y la variante:

| Condición (`online`, `local`) | Etiqueta exacta | Variante |
|---|---|---|
| `online` es verdadero | `Internet accesible` | `default` |
| `online` falso y `local` verdadero | `Internet no accesible` | `error` |
| `online` falso y `local` falso | `Sin conexión de red` | `error` |

`local = online or network.local_network()` (línea 1004): si hay Internet, no se consulta la red local.

Reglas de repintado (líneas 1013-1018):

- La etiqueta solo se asigna si cambia (`str(button.label) != label`). Así el sondeo no repinta un botón que no ha cambiado.
- La variante solo se asigna si cambia.

Estado transitorio: cuando el usuario pulsa el botón, hay Internet caído y la red local está activa (`prompt and not online and local and network`, línea 1019), la etiqueta pasa a `Diagnosticando la red…` (carácter U+2026, línea 1020). La variante sigue siendo `error`. Termina cuando `network_diagnosis` llama a `check_network()` o abre un diálogo.

Estado de excepción: si `Network(...)` o `check` lanzan una excepción, `check_network` registra el error con `capture(..., operation='network.check')` (línea 1007) y llama a `network_result(None, False, True, False)` (línea 1008). Resultado: `Internet no accesible` con variante `error`.

## 4. Sondeo periódico

### 4.1 Arranque (`start_network_poll`, líneas 903-911)

1. `seconds = float(properties(root).get('network.poll_seconds', '20'))` dentro de un `try` (línea 906).
2. Si `ValueError` (cualquier valor que `float` no acepte, y también un `config.properties` con UTF-8 inválido, porque `properties` está dentro del `try`): `seconds = 20.0` (líneas 908-909).
3. Si `seconds > 0` y `seconds != float('inf')`: `self.network_timer = set_interval(max(5.0, seconds), poll_network)` (línea 911).

Tabla de valores de `network.poll_seconds`:

| Valor en config | `seconds` resultante | Sondeo |
|---|---|---|
| (ausente) | 20.0 | cada 20 s |
| `20` | 20.0 | cada 20 s |
| `3` | 3.0 | cada **5 s** (mínimo) |
| `5` | 5.0 | cada 5 s |
| `0` | 0.0 | **desactivado** |
| `-1` | -1.0 | **desactivado** |
| `inf` | inf | **desactivado** (`seconds != inf` falla) |
| `1e400` | inf | **desactivado** (se parsea como infinito) |
| `nan` | nan | **desactivado** (`nan > 0` es falso) |
| `abc` o cadena vacía | 20.0 | cada 20 s |
| `  12  ` | 12.0 | cada 12 s (`float` admite espacios) |
| `12,5` | 20.0 | cada 20 s (la coma no es separador decimal en `float`) |

La configuración no valida `network.poll_seconds` (`configuration.py:31`, sin restricción). Un valor inválido no impide guardar.

El temporizador se crea una sola vez, al montar la aplicación. Cambiar el valor en la configuración no lo reinicia hasta reabrir MITO (Duda D1).

### 4.2 Cada tick (`poll_network`, líneas 913-917)

No hace nada si se cumple cualquiera de estas condiciones:

- `len(self.screen_stack) > 1`: hay un diálogo abierto (cualquier `ModalScreen`, no solo los de red).
- Hay un worker del grupo `network` que no ha terminado: una comprobación manual, un diagnóstico o el sondeo anterior.

En otro caso llama a `check_network(quiet=True)`:

- `Network.check(log=False)` (línea 1003): un fallo de red no escribe `network.check` en el log (ver `network.md`, sección 4.2). Si falla la construcción de `Network` o algo fuera de `check`, sí se registra con `capture` (línea 1007), también en modo `quiet` (Posibles fallos F3).
- `prompt` es falso: nunca abre diálogos ni notifica. Solo repinta el botón.

## 5. Cuándo se comprueba Internet

| Momento | Llamada | Registra en log |
|---|---|---|
| Arranque de la UI (`on_mount`) | `check_network()` | Sí (`log=True`) |
| Cada tick del sondeo | `check_network(quiet=True)` | No |
| Pulsación de INTERNET | `check_network(prompt=True)` | Sí |
| Tras cerrar la configuración global (`ui.py:1508-1517`, solo si no hay perfil seleccionado) | `check_network()` | Sí |
| Tras `diagnose` sin cambios o cancelando un diálogo | `check_network()` | Sí |

## 6. Flujo de pulsación: diagnóstico → diálogos → guardado → login

### 6.1 Pulsación (`ui.py:1301-1302`)

`check_network(prompt=True)`. El resultado depende de `network_result` (sección 3):

- Con Internet: solo repinta el botón. No hay diagnóstico.
- Sin Internet y sin red local (`local` falso): notificación `Conecta el equipo a una red` (warning, ver sección 9). No hay diagnóstico.
- Sin Internet y con red local: etiqueta `Diagnosticando la red…` y `diagnose_network()` (worker).

### 6.2 Diagnóstico (`diagnose_network`, líneas 1025-1033)

Llama a `network.diagnose(root)` (sin `Network` ni resolutor, así que usa `system_proxy`). Si lanza excepción: `capture(..., operation='network.diagnose')` y `result = None`. Después llama a `network_diagnosis(result)` en el hilo principal.

### 6.3 Decisión (`network_diagnosis`, líneas 1035-1085)

Orden de evaluación (la primera condición que se cumple gana):

| # | Condición | Acción |
|---|---|---|
| 1 | `diagnosis is None` | `check_network()`. Sin notificación |
| 2 | `kind == 'ok'` y `changes` vacío | `check_network()`. Sin notificación |
| 3 | `kind == 'offline'` | `check_network()` y notificación `Conecta el equipo a una red` |
| 4 | `changes` no vacío | `push_screen(NetworkSetupDialog(diagnosis), saved)` (sección 6.4) |
| 5 | `kind == 'portal'` (sin cambios) | `login()` (sección 6.5) |
| 6 | Cualquier otro caso sin cambios | `check_network()` y notificación según el `kind` (sección 9) |

Nota sobre la condición 2: en Python, `A or B and C` se evalúa como `A or (B and C)`. Un diagnóstico `kind == 'ok'` **con** `changes` no entra en la condición 2 y pasa a la 4.

Qué puede llegar a la condición 4: `changes` solo se rellena en `ok` (proxy que ya funciona pero hay que guardarlo) y en `portal` (ver `network.md`, sección 7.6).

### 6.4 `NetworkSetupDialog` y `saved` (líneas 1060-1072)

`saved(accepted)`:

- `accepted` falso: `check_network()` y fin.
- `accepted` verdadero: `apply(engine, diagnosis.changes)` (`network.md`, sección 9).
  - `OSError` o `YitoError`: notificación `str(exc)` con severidad `error` y `check_network()`; fin.
  - Éxito: `write_log('Configuración de red guardada en config.properties')` (línea 1071). Después `login()` si `kind == 'portal'`, o `check_network()` en otro caso.

### 6.5 `login()` (líneas 1048-1058)

1. `network = Network(root)`. Si `YitoError`: notificación `str(exc)` con severidad `error`, `check_network()`, fin. (Un `ValueError` no se captura aquí: Posibles fallos F2.)
2. Si `network.enabled`: `push_screen(ProxyDialog(network), lambda _: check_network())`. El valor devuelto por `ProxyDialog` no se usa: al cerrar, siempre se comprueba Internet otra vez.
3. Si no: `check_network()`. Sin notificación.

### 6.6 `ProxyDialog` (sección 8.3)

Ver sección 8.3 para la interacción completa.

Al cerrar, `check_network()` vuelve a probar Internet. Si el login fue correcto, el botón pasa a `Internet accesible`. Si fue cancelado, el botón queda como estaba antes del diálogo (que, tras la comprobación, suele ser `Internet no accesible`).

## 7. Test `NetworkSetupUITests` (`test_network_setup.py:115-145`)

Clase `AsyncTestCase` (de `async_support`). Un único test: `test_detected_portal_is_saved_then_credentials_are_asked_for_the_session`.

Preparación:

- `Engine(Path(tmp))` con un perfil `test`.
- `config.properties` **no existe** al empezar.
- `changes = {https_proxy: 'http://proxy.example:8080', enable_proxy_authentication: 'true', proxy_authentication_url: 'http://portal.test/login'}`.
- `found = Diagnosis('portal', 'http://proxy.example:8080', 'pac', portal='http://portal.test/login', changes=changes)`.
- Parches: `Network.check` → `False`; `Network.local_network` → `True`; `mito.network.diagnose` → `found`.
- Pantalla: `run_test(size=(130, 42))`. Espera a `app.workers.wait_for_complete()`.

Pasos y expectativas (en orden):

1. `app.check_network(prompt=True)`. Espera a que `app.screen` sea `NetworkSetupDialog`.
2. Pulsa `#network-setup-cancel`. Espera a que ya no esté `NetworkSetupDialog`. **Expectativa:** `config.properties` no existe.
3. `app.check_network(prompt=True)`. Espera a `NetworkSetupDialog`. **Expectativa:** el texto de `#network-setup-keys` contiene `proxy_authentication_url=http://portal.test/login`.
4. Pulsa `#network-setup-save`. Espera a que `app.screen` sea `ProxyDialog`. **Expectativa:** las tres claves de `changes` están en el archivo (`parse(...)` las devuelve con esos valores).
5. Pulsa `escape`. Espera a que ya no esté `ProxyDialog`.

Qué demuestra: cancelar no escribe nada; guardar escribe las claves; y el login se pide después de guardar, no antes.

## 8. Textos, estilos y comportamiento de los diálogos

### 8.1 `source_label(source)` (líneas 8-14)

| Entrada | Salida |
|---|---|
| `config.properties` | `config.properties` (sin traducir) |
| `manual` | `tr('configuración manual de Windows')` |
| `pac` | `tr('script PAC')` |
| `wpad` | `tr('autodetección WPAD')` |
| cualquier otro | el mismo valor, sin cambios |

### 8.2 `NetworkSetupDialog` (líneas 17-51)

`BINDINGS = [('escape', 'cancel', 'Cancelar')]`. `action_cancel` → `dismiss(False)`.

Constructor: guarda `diagnosis`.

`compose` (líneas 31-44), en este orden dentro de `Vertical(id='network-setup')`:

| # | Widget | Texto exacto | Notas |
|---|---|---|---|
| 1 | `Static` | `CONFIGURACIÓN DE RED` | Título. Sin `id` |
| 2 | `Static` | Si `proxy` no vacío: `Proxy: {p0} ({p1})` con `p0 = proxy` y `p1 = source_label(source)`. Si no: `Conexión directa, sin proxy` | Sin `markup=False` (Posibles fallos F4) |
| 3 | `Static` | Si `kind == 'ok'`: `El proxy da acceso a Internet.`. Si no: `La red redirige a un portal de autenticación.` | Sin `markup=False` |
| 4 | `Static` | `Se guardarán estas claves en config.properties:` | |
| 5 | `Static` (`id='network-setup-keys'`, `markup=False`) | Una línea por entrada de `changes`: `clave=valor`, unidas con `\n` | Estilo `margin: 1 0; color: #b5dc64` |
| 6 | `Static` | Solo si `kind == 'portal'`: `Tu usuario y contraseña nunca se guardan: MITO los pedirá en cada sesión que el portal lo requiera.` | |
| 7 | `Button` (`id='network-setup-save'`, `variant='primary'`) | `Guardar y conectar` | `dismiss(True)` |
| 8 | `Button` (`id='network-setup-cancel'`) | `Cancelar` | `dismiss(False)` |

`on_button_pressed` (líneas 49-51): `event.stop()` y `dismiss(event.button.id == 'network-setup-save')`.

Ancho: 80 columnas (`width: 80`). Alto automático. Borde redondeado de color `#b5dc64`, fondo `#153429`, relleno `1 2`. Los botones ocupan el 100 % del ancho del diálogo y tienen `margin-top: 1`. Fondo de la pantalla modal `#00000088`, centrado.

CSS literal (`network_ui.py:20-25`):

```css
NetworkSetupDialog { align: center middle; background: #00000088; }
#network-setup { width: 80; height: auto; padding: 1 2; border: round #b5dc64; background: #153429; }
#network-setup-keys { margin: 1 0; color: #b5dc64; }
#network-setup Button { margin-top: 1; width: 100%; }
```

### 8.3 `ProxyDialog` (líneas 54-122)

`BINDINGS = [('escape', 'cancel', 'Cancelar')]`.

Constructor: guarda `network` y `credentials = None`.

`compose` (líneas 68-76), en `Vertical(id='proxy-dialog')`:

| # | Widget | Texto o placeholder exacto | `id` | Otras propiedades |
|---|---|---|---|---|
| 1 | `Static` | `AUTENTICACIÓN DE RED` | — | |
| 2 | `Static` | `Las credenciales no se guardan. Escape cancela.` | — | |
| 3 | `Input` | placeholder `Usuario` | `proxy-user` | |
| 4 | `Input` | placeholder `Contraseña` | `proxy-password` | `password=True` (enmascarado) |
| 5 | `Static` | (vacío; luego recibe `Conectando…` o el mensaje de error) | `proxy-result` | |
| 6 | `Button` | `Conectar` | `proxy-submit` | `variant='primary'` |
| 7 | `Button` | `Cancelar` | `proxy-cancel` | |

Ancho: 66 columnas. Los `Input` tienen `margin-top: 1`. Los botones ocupan el 100 % con `margin-top: 1`. Resto de estilos iguales a `NetworkSetupDialog`.

CSS literal (`network_ui.py:57-60`):

```css
ProxyDialog { align: center middle; background: #00000088; }
#proxy-dialog { width: 66; height: auto; padding: 1 2; border: round #b5dc64; background: #153429; }
#proxy-dialog Input { margin-top: 1; }
#proxy-dialog Button { margin-top: 1; width: 100%; }
```

Interacción:

- `action_cancel` (líneas 78-81): vacía `#proxy-password`, pone `credentials = None` y `dismiss(False)`.
- `on_button_pressed` (líneas 83-94):
  - `event.stop()`.
  - `proxy-cancel` → `action_cancel()`.
  - `proxy-submit`:
    1. Lee `username` de `#proxy-user` y `password` de `#proxy-password`.
    2. Vacía `#proxy-password` (`value = ''`) antes de cualquier otra cosa.
    3. Deshabilita `#proxy-submit`.
    4. Pone en `#proxy-result` el texto `Conectando…` (U+2026).
    5. `self.credentials = (username, password)`; `self.authenticate()` (worker).
- `authenticate` (worker `thread=True`, `exit_on_error=False`, sin grupo; líneas 96-113):
  1. Toma `credentials` y pone `self.credentials = None`. Si era `None`: sale sin llamar a `completed`. Con la UI actual no ocurre, porque el worker solo se lanza tras rellenar `credentials`.
  2. `network.authenticate(username, password)`.
  3. `except Exception as exc`: `write(network.root, 'error', 'network.authenticate', type(exc).__name__)` y `success = False`. Solo se registra el nombre de la excepción, nunca el mensaje.
  4. `finally`: pone `username` y `password` a `None`.
  5. `app.call_from_thread(self.completed, success)`.
- `completed(success)` (líneas 115-122):
  - Si el diálogo ya no está montado: sale.
  - Éxito: `dismiss(True)`.
  - Fallo: `#proxy-result` pasa a `No se pudo verificar Internet. Puedes reintentar o cancelar.` y `#proxy-submit` vuelve a estar habilitado. La contraseña ya está vacía: hay que volver a escribirla.

Qué no hace:

- Enter en el campo de contraseña no envía (no hay `on_input_submitted`). Ver Dudas D3.
- No hay límite de reintentos en el diálogo.
- El usuario no ve el motivo de un `YitoError` de `authenticate` (Posibles fallos F1).

## 9. Notificaciones y registros

Todas las notificaciones usan `self.notify(texto, severity=...)`. Si no se indica `timeout`, se usa el valor por defecto de Textual (el código no lo fija).

| Línea | Cuándo | Texto exacto | Severidad | Timeout |
|---|---|---|---|---|
| 1023 | Pulsación, sin Internet y sin red local | `Conecta el equipo a una red` | `warning` | por defecto |
| 1045 | Diagnóstico `offline` | `Conecta el equipo a una red` | `warning` | por defecto |
| 1052 | `Network(...)` lanza `YitoError` en `login()` | `str(exc)` | `error` | por defecto |
| 1068 | `apply` lanza `OSError` o `YitoError` | `str(exc)` | `error` | por defecto |
| 1081 | Diagnóstico `auth` sin cambios | `El proxy {p0} exige autenticación HTTP 407 ({p1}), que MITO no admite.` (`p0` = proxy, `p1` = esquemas o `Basic`) | `warning` | 10 s |
| 1083 | Diagnóstico `blocked` sin cambios | `La comprobación de Internet respondió HTTP {p0}.` (`p0` = código) | `warning` | 10 s |
| 1085 | Diagnóstico `unreachable` (o cualquier otro sin cambios) | `No se pudo llegar a Internet ni con el proxy de Windows ni directamente.` | `warning` | 10 s |
| 1510 | Configuración global guardada | `Configuración guardada. Abre nuevas pestañas o aplicaciones para usar el nuevo entorno.` | (por defecto) | por defecto |

Texto que remite al botón. Está en `DownloadIntegrityDialog` (`ui.py:563-587`), que se muestra cuando la descarga llega alterada por la red y `error.portal` es verdadero (si es falso, el texto habla de proxy o antivirus y no menciona el botón):

| Línea | Texto exacto |
|---|---|
| 585 | `Causa más probable: el portal de autenticación de la red. Pulsa INTERNET, conéctate y vuelve a intentarlo.` |

Registros en el log de la UI (`write_log`, no notificaciones):

| Línea | Texto exacto |
|---|---|
| 1071 | `Configuración de red guardada en config.properties` |

Registros de diagnóstico (`capture`, sin texto para el usuario):

| Línea | Operación |
|---|---|
| 1007 | `network.check` |
| 1031 | `network.diagnose` |

Los mensajes `str(exc)` de 1052 y 1068 dependen de `configuration.py` o `network.py` (en español).

## 10. Invariantes

1. El botón INTERNET nunca muestra un estado que no esté en la tabla de la sección 3. Los únicos textos son `Comprobando Internet…`, `Internet accesible`, `Internet no accesible`, `Sin conexión de red` y `Diagnosticando la red…`.
2. El sondeo periódico nunca abre diálogos, nunca notifica y nunca escribe `network.check` en el log cuando hay un fallo de red (`check(log=False)`).
3. El sondeo no se ejecuta mientras hay un diálogo abierto (`screen_stack > 1`) ni mientras hay un worker de red activo.
4. `diagnose` no guarda nada. Solo `apply`, tras confirmación en `NetworkSetupDialog`, escribe `config.properties`.
5. `NetworkSetupDialog` no muestra ni guarda credenciales: solo `changes` (claves de proxy, URL de portal y nombres de campo).
6. La contraseña del portal se vacía del `Input` al enviar, antes de empezar el worker, y el diálogo nunca la guarda en disco ni en el log. El log de error solo registra el nombre de la excepción.
7. Cancelar `NetworkSetupDialog` no modifica `config.properties` (test, sección 7, paso 2).
8. Tras cerrar `ProxyDialog` (con éxito o sin él), se vuelve a comprobar Internet.

## 11. Casos límite

- Pulsar INTERNET dos veces seguidas: el segundo worker de `check_network` (grupo `network`, `exclusive=True`) cancela al primero. Solo queda un resultado.
- Pulsar INTERNET durante un sondeo: el sondeo se cancela por el mismo motivo.
- Con un diálogo modal abierto, el sondeo no se ejecuta (sección 4.2). La pulsación del botón no está disponible hasta cerrar el diálogo, porque la pantalla modal cubre la barra.
- `diagnosis.kind == 'ok'` con `changes` (proxy de Windows que funciona y hay que guardar): se muestra `NetworkSetupDialog` con el texto `El proxy da acceso a Internet.`.
- `diagnosis.kind == 'blocked'` por URL de portal inválida (`network.md`, sección 7.6): el mensaje dice `respondió HTTP 302` (o el código real), aunque la causa sea la URL del portal (Posibles fallos F5).
- Portal con `enable_proxy_authentication=true` ya configurado, pero `proxy_authentication_url` vacío: `Network.enabled` es falso y `login()` no muestra `ProxyDialog`; `check_network()` se ejecuta sin notificación.
- Guardar desde `NetworkSetupDialog` y después fallar `apply`: notificación `error` con el mensaje de `configuration.py`, y no se llama a `login()`.
- Cerrar la aplicación con un worker de `ProxyDialog` activo: `completed` no hace nada si el diálogo ya no está montado.
- Terminal más estrecha que el ancho del diálogo (80 o 66 columnas): el original no define el comportamiento.

## 12. Dependencias

- `mito.localized_ui.ModalScreen`: pantalla modal con traducción de bindings.
- `mito.i18n.tr`: todos los textos de esta sección. `source_label` traduce sus tres etiquetas.
- `mito.network`: `Network`, `diagnose`, `apply`, `properties` y `Diagnosis` (import dentro de las funciones). Ver `network.md`.
- `mito.diagnostics`: `write` (dentro de `ProxyDialog.authenticate`, línea 108) y `capture` (`ui.py`). Ver `diagnostics.md`.
- `mito.storage.YitoError`.
- Textual: `work`, `Vertical`, `Button`, `Input`, `Static`, `set_interval`, `call_from_thread`, `notify`, `push_screen`, `dismiss`.

## 13. Notas para Rust

- Mismos estados y textos del botón (sección 3). En `mito-tui`, el sondeo es un temporizador de `tokio` o un hilo que envía un mensaje al bucle principal (no `set_interval`).
- La regla de sondeo (mínimo 5 s, 0 o inf o nan desactiva, valor inválido → 20 s) debe ser una función pura con tests de tabla (ver la tabla de la sección 4.1 como casos de prueba).
- Textual `ModalScreen` → pantalla o modal propio de `ratatui`. El orden de los elementos y los textos de la sección 8 son el contrato visual; los colores y anchos son los CSS de la sección 8.
- `ProxyDialog` debe llamar al autenticador en un hilo aparte y mostrar `Conectando…` mientras tanto. La contraseña se guarda en `SecretString` (`zeroize`) y nunca pasa al log.
- El test `NetworkSetupUITests` se porta como test de integración de `mito-tui` con `ratatui` y un backend de pruebas, usando los mismos pasos de la sección 7.

## Dudas

- **D1. Cambio de intervalo sin reinicio.** `start_network_poll` solo corre al montar la UI (`ui.py:899`). Si el usuario cambia `network.poll_seconds` en la configuración, el nuevo valor no se aplica hasta reabrir MITO. ¿Rust debe reaplicarlo al guardar la configuración?
- **D2. Notificaciones de error sin timeout.** Los `notify` de `str(exc)` (líneas 1052 y 1068) y de `Conecta el equipo a una red` (1023 y 1045) no fijan `timeout`, así que usan el valor por defecto de Textual. ¿Debe Rust usar un tiempo fijo o mantenerlas hasta que el usuario las cierre?
- **D3. Enter en el campo de contraseña.** En `ProxyDialog` pulsar Enter en `Usuario` o `Contraseña` no envía el formulario. ¿Debe Rust enviar con Enter, como hace `AcceptButton` en WinForms (`network.md`, sección 8)?
- **D4. Mensaje de error de login.** El texto `No se pudo verificar Internet. Puedes reintentar o cancelar.` es el mismo tanto si la contraseña es incorrecta como si el portal rechazó la petición o hay un error de HTTPS. ¿Debe distinguirse? Ver Posibles fallos F1.
- **D5. Pantalla de diagnóstico sin feedback.** Mientras `diagnose` ejecuta `system_proxy` (PowerShell, hasta 30 s según `network.md`), la etiqueta es `Diagnosticando la red…` y no hay indicador de progreso. ¿Es suficiente?
- **D6. Diálogo de cambios para `kind == 'ok'`.** Si el diagnóstico encuentra un proxy que ya funciona pero hay que guardarlo, el diálogo dice `El proxy da acceso a Internet.` y pide guardar. ¿Debe Rust preguntar o guardar en silencio cuando el proxy ya funciona?

## Posibles fallos del original

- **F1. Mensaje del `YitoError` de `authenticate` no visible.** `ProxyDialog.authenticate` captura `Exception` (`network_ui.py:106`) y muestra el texto genérico de la línea 121. Los errores de HTTPS obligatorio y de origen distinto (`network.md`, sección 4.7, pasos 7 y 8) nunca llegan al usuario, que queda sin saber qué configurar.
- **F2. `ValueError` sin capturar en `login()`.** `network_diagnosis.login()` solo captura `YitoError` (`ui.py:1051`). Un `network.timeout_seconds` o `network.check_status` no numéricos (`network.md`, sección 4.1) lanzan `ValueError` y la excepción sale de un callback de Textual.
- **F3. Sondeo que repite excepciones.** En modo `quiet`, `check_network` captura cualquier excepción y llama a `capture` (`ui.py:1006-1007`). Si la configuración es inválida, cada tick del sondeo (cada 5 a 20 s) vuelve a fallar. Depende de si `capture` escribe en el log o en los pendientes; ver `diagnostics.md`.
- **F4. Texto de ruta sin `markup=False`.** La `Static` de la ruta (`network_ui.py:37`) no usa `markup=False`, aunque incluye la URL del proxy. Una URL con corchetes (por ejemplo IPv6 `http://[::1]:8080`) se interpreta como marcado Rich y puede no mostrarse bien o lanzar error de marcado. La `Static` de las claves sí usa `markup=False` (línea 40).
- **F5. Mensaje de fallo con etiqueta engañosa.** Con `kind == 'blocked'` causado por una URL de portal inválida (`network.md`, sección 7.6), el diálogo dice `La comprobación de Internet respondió HTTP {status}` con el código de la redirección (`ui.py:1083`). El usuario busca un problema HTTP que no existe.
## Decisiones del port (revisión de Opus)

- F1 se corrige: el diálogo muestra el texto del error de configuración de `authenticate`.
- F2/F3 se corrigen como en `network.md`. F4 no aplica (ratatui no interpreta marcado).
- D1–D6 y F5: igual que el original. En D3, Enter sigue la convención de los diálogos de
  la TUI de F6 (envía el botón principal), equivalente al `AcceptButton` de WinForms.
