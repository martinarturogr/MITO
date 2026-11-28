# protocols.py + protocols_ui.py → mito-win::protocols y mito-tui::protocols_ui

Origen: `core/app/mito/protocols.py` (356 líneas), `core/app/mito/protocols_ui.py` (171 líneas),
`core/tests/test_protocols.py` (12 tests: 11 de registro y 1 de diálogo). Botón `protocol-<n>` «Protocolos» en
`ui.py` (líneas 1123 y 1420-1435).

## Responsabilidad

Reclama esquemas de URL (`claude://`, `vscode://`…) para un componente de MITO. Un navegador entrega a Windows los
enlaces de esquema personalizado (por ejemplo el retorno de un inicio de sesión), y Windows ejecuta el comando de
`HKCU|HKLM\Software\Classes\<esquema>`. La copia del host de la aplicación suele ser la dueña de esa clave, así que
el enlace llega al host y no a la instancia de MITO. Reclamar escribe `HKCU\Software\Classes\<esquema>` con el mismo
lanzador que «Abrir con» (`mito run --profile --app -- "%1"`). La reclamación vive solo mientras el diálogo está
abierto: el diálogo la vuelve a afirmar cada 5 s y la libera al cerrar. El registro anterior se guarda en
`core/state/protocols.json` y se restaura al liberar, solo si la clave aún tiene el comando de MITO.

## API pública del original (`protocols.py`)

| Función / clase | Entradas | Salida | Errores |
|---|---|---|---|
| `REGISTRY` (26), `CLASSES` (27), `MACHINE_CLASSES` (29) | — | `'core/state/protocols.json'`, `r'Software\Classes'` (ambas) | — |
| `SCHEME` (30), `RESERVED` (32), `STATUSES` (34) | — | regex, conjunto de reservados, estados | — |
| `registry(root)` (42) | raíz | `dict` (o `{}` si el JSON no es objeto) | `read_json` |
| `save(root, records)` (48) | raíz, dict | `None` | E/S |
| `scheme_name(text)` (52) | texto | esquema normalizado | `YitoError` |
| `command(root, profile, app, shortcut=None)` (61) | raíz, strings | línea de comando (`open_with.command`) | — |
| `snapshot(hive, path)` (69) | hive, ruta | árbol JSON de la clave o `None` si no existe | `OSError` sin FileNotFound se propaga |
| `write_tree(path, tree)` (101) | ruta HKCU, árbol | `None` | `OSError` |
| `delete_key(scheme)` (112) | esquema | `None` (borra `CLASSES\<esquema>` entera) | — |
| `read_command(hive, base, scheme)` (119) | hive, base, esquema | comando (`str`), `''` si no es texto, o `None` | captura `OSError` → `None` |
| `current(scheme)` (133) | esquema | `(comando, 'user'\|'machine')` o `(None, None)` | — |
| `executable(command_line)` (146) | comando | primer token sin comillas o `''` | — |
| `inside_root(root, path)` (155) | raíz, ruta | `bool` | — |
| `status(root, scheme, profile, installation, records=None)` (164) | … | `(estado, comando)` | — |
| `label(state)` (183) | estado | texto traducido | — |
| `schemes(hive, base)` (189) | hive, base | generador de esquemas en minúsculas | `OSError` captura, termina |
| `candidates(engine, lock)` (208) | motor, lock | lista ordenada de esquemas | — |
| `describe(engine, profile, installation, scheme)` (227) | … | `{'scheme','status','command'}` | `YitoError` |
| `rows(engine, profile, installation)` (234) | motor, perfil, instalación | lista de `describe` de los candidatos | `KeyError` si no está instalado |
| `class Session(engine, profile, installation, log=print)` (240) | … | `describe`, `claim`, `check`, `release` | — |
| `claim(engine, profile, installation, scheme, shortcut=None, log=print, keep_backup=False)` (276) | … | `record` | `YitoError` |
| `release_record(root, record, log=print)` (326) | raíz, registro | `None` | — |
| `unregister(root, select, log=print)` (339) | raíz, predicado, log | lista de registros liberados | captura y registra |

`Session` (240-271):
- `describe(scheme)` → `describe(engine, profile, installation, scheme)`.
- `claim(scheme, shortcut=None)` → `claim(...)`, guarda `schemes[scheme] = shortcut`, devuelve `describe(...)`.
- `check(scheme)` → `False` si el esquema no está reclamado o si su estado es `mine`; si no, vuelve a reclamar con
  `keep_backup=True` (sin log) y devuelve `True`.
- `release(scheme=None)` → `unregister` de los registros del esquema (o de todos los reclamados) para ese perfil e instalación,
  y los quita de `schemes`.

## Claves y valores de registro

Hives: `HKCU` (escritura) y `HKLM` (solo lectura, para mostrar y detectar). Todas las claves de esquema son:

```
HKCU\Software\Classes\<esquema>
    (Predeterminado)      REG_SZ  URL:<esquema>
    URL Protocol          REG_SZ  (vacío)
    DefaultIcon\
        (Predeterminado)  REG_SZ  <icono>,0
    shell\open\command\
        (Predeterminado)  REG_SZ  <raíz>\core\tools\python\pythonw.exe -B -I -X utf8 -m mito run --profile default --app main/claude --shortcut Claude -- "%1"
```

- `URL Protocol` es imprescindible: `read_command` ignora claves sin él (`FileNotFoundError` → `None`).
- `<icono>` viene de `open_with.icon_for`. `command` es el de `open_with.command`.
- Al reclamar se borra primero la clave entera `HKCU\Software\Classes\<esquema>` (incluidas subclaves y valores binarios
  del host) y se escribe la nueva. Por eso el respaldo es imprescindible.
- Los tipos de valor se guardan tal cual (`REG_SZ` = 1, `REG_BINARY` = 3, `REG_NONE` = 0, `REG_EXPAND_SZ` = 2, `REG_DWORD` = 4).
  Los binarios se guardan como `{"hex": "…"}`.

**Respaldo (`snapshot`)**: árbol `{"values": [[nombre, tipo, datos], …], "keys": {hijo: árbol, …}}`. Clave ausente =
`None`. Todos los valores y subclaves de la clave, en orden de enumeración de Windows.

## Formatos de disco

**Registro** `core/state/protocols.json` (`write_json`, mismo formato que `open-with.json`). Clave: el esquema
(en minúsculas). Valor: registro con el perfil, instalación, aplicación, atajo y respaldo:

```json
{
  "claude": {
    "scheme": "claude",
    "profile": "default",
    "installation": "main/claude",
    "app": "main/claude",
    "shortcut": "Claude",
    "backup": {
      "values": [
        ["", 1, "URL:claude"],
        ["URL Protocol", 1, ""],
        ["Blob", 3, {"hex": "0001"}]
      ],
      "keys": {
        "shell": {
          "keys": {
            "open": {
              "keys": {
                "command": {
                  "values": [["", 1, "\"C:\\Users\\usuario\\AppData\\Local\\AnthropicClaude\\app-1\\claude.exe\" -- \"%1\""]]
                }
              }
            }
          }
        }
      }
    }
  }
}
```

- `backup` es `null` cuando la clave no existía antes de la reclamación.
- `shortcut` es `null` si la entrada es principal.
- Estas claves de registro no se crean al escribir el JSON; lo que cambia es el registro del sistema.

## Estados (`STATUSES`) y etiquetas

`status()` devuelve, por este orden:

1. `absent`: no hay valor en HKCU ni, si está activo, en HKLM.
2. `mine`: el registro del esquema coincide con el perfil y la instalación indicados, y el comando actual es exactamente
   el que generaría `command(record)`.
3. `profile`: el ejecutable es `<raíz>\core\tools\python\pythonw.exe` y el comando contiene ` -m mito run `, sea otro perfil.
4. `direct`: el ejecutable está dentro de la raíz de MITO pero no es el lanzador (p. ej. `core/apps/main/claude/claude.exe`).
5. `other`: cualquier otra cosa (la aplicación del host, otra app).

Etiquetas (`label`): `mine` → `De este profile`; `profile` → `Otro profile de MITO`; `direct` → `Ejecutable de MITO sin entorno`;
`other` → `Otra app`; `absent` → `No registrado`.

## Candidatos

`candidates(engine, lock)`: nombres de `engine.application_entries(lock)` (el segundo elemento, el ejecutable) reducidos a
su nombre de archivo en casefold (`claude.exe`). Recorre HKCU y HKLM (si `MACHINE_CLASSES` no es nulo). Un esquema es
candidato si:
- cumple `SCHEME` (en minúsculas), no es reservado ni empieza por `ms-`;
- `read_command` devuelve un comando cuyo ejecutable (`executable`) tiene el mismo nombre de archivo (casefold).

Devuelve la lista ordenada y sin repetir (HKCU gana; HKLM solo aporta esquemas no vistos).

Nota: este emparejamiento compara el **nombre completo** del ejecutable (`claude.exe`). `open_with` usa el stem y
subcadenas (ver `open_with.md`); son reglas distintas.

## Invariantes

1. `scheme_name` normaliza: quita espacios, pasa a minúsculas, elimina un sufijo `://` o `:`. Valida `^[a-z][a-z0-9+.-]{1,31}$`
   (2 a 32 caracteres). Rechaza `RESERVED` (`http`, `https`, `file`, `ftp`, `mailto`, `javascript`, `data`, `about`,
   `blob`, `res`, `shell`, `ms-settings`, `search-ms`, `microsoft-edge`, `mito`) y cualquier cosa que empiece por `ms-`.
2. `claim` exige que el componente esté en `profile['apps']` y que `application_target` resuelva (como en «Abrir con»).
3. `claim` ejecuta bajo `mutation(root)`. Antes de escribir toma el respaldo: si ya había un registro y el estado es
   `mine` o `profile` (o `keep_backup`), **reutiliza el respaldo anterior** (el del host original); si no, toma uno nuevo.
4. Escritura: `delete_key` + `write_tree`. Si algo falla: `delete_key` y, si había respaldo, `write_tree(respaldo)`; luego relanza.
   El registro no se guarda. `notify()` se llama en `finally`.
5. `release_record` restaura solo si `read_command(HKCU)` es exactamente el comando de MITO del registro: `delete_key` y, si hay
   respaldo, `write_tree(respaldo)`. Si no coincide (la clave la retomó otra aplicación o no existe), no toca la clave.
   En ambos casos el registro se elimina del JSON.
6. `unregister` guarda y notifica solo si liberó algo. Un error por registro (`OSError`, `KeyError`, `ValueError`, `YitoError`)
   se registra y el registro se conserva.
7. `Session.check` vuelve a afirmar la reclamación solo si el estado no es `mine` (p. ej. el host la sobrescribió).
   Si la afirma, lo hace con `keep_backup=True` y así conserva el respaldo original.
8. `Session.release()` libera todos los esquemas reclamados de la sesión y no toca los de otras sesiones o perfiles.
9. El diálogo, al abrirse, llama `protocols.unregister(root, lambda record: True, log)`: libera reclamaciones huérfanas
   de un cierre de MITO con el diálogo abierto.
10. `inside_root` compara con `commonpath` sobre rutas normalizadas en minúsculas.

## Casos límite y errores

| Clave (texto español) | Cuándo |
|---|---|
| `Esquema no válido: {p1}` | `scheme_name` (`p1` = texto original) |
| `Esquema reservado del sistema: {p1}` | `scheme_name` (`p1` = esquema normalizado) |
| `El componente debe estar activo en el profile` | `claim` con instalación no activa |
| `Protocolo {p1} reclamado para {p3}.` | log de `claim` (`p3` = `{profile} - {app}`) |
| `Protocolo {p1} liberado; registro anterior restaurado.` | log de `release_record` con restauración |
| `Protocolo {p1} liberado; ya apuntaba a otra aplicación y no se ha modificado.` | log de `release_record` sin restauración |
| `No se pudo liberar el protocolo {p1}: {p3}` | `unregister`: error por registro |
| `Recuperado a las {p1}: otra app lo había sobrescrito.` | nota del diálogo tras `check` (`p1` = `HH:MM:SS`) |
| `Reclamado: se vigila cada 5 s mientras este diálogo siga abierto.` | nota por defecto de una fila reclamada |
| `No se ha detectado ningún esquema; escríbelo abajo.` | diálogo sin candidatos |

## Casos de test del original

| Test | Archivo:líneas | Qué comprueba |
|---|---|---|
| `test_scheme_names_are_validated_and_reserved_ones_rejected` | test_protocols.py:67-71 | ` Claude:// ` → `claude`; `''`, `x`, `1abc`, `a b`, `http`, `HTTPS`, `file`, `ms-settings`, `ms-teams`, `mito` → `YitoError`. |
| `test_candidates_match_declared_executable_names` | :73-76 | `claude` aparece como candidato (ejecutable `claude.exe`), `other` (`other.exe`) no. |
| `test_claim_backs_up_and_release_restores_exactly` | :78-91 | Reclamar escribe el comando de MITO con `--shortcut Claude`; `mine` para el perfil, `profile` para otro; liberar restaura comando y árbol exactos (incluido binario) y deja el registro vacío. |
| `test_release_leaves_a_key_another_application_took_back` | :93-99 | Si otra app vuelve a escribir la clave, liberar no la toca. |
| `test_session_reasserts_overwritten_claim_and_restores_the_first_backup` | :101-115 | `Session.claim` → `mine`; `check` no hace nada mientras sea `mine`; cuando la app del host la sobrescribe, `check` devuelve `True` y restaura `mine` con el mismo respaldo; `release` restaura el host original. |
| `test_session_release_leaves_a_key_taken_after_the_last_check` | :117-123 | Si la clave cambia después del último `check`, `release` no la toca. |
| `test_claim_of_absent_scheme_is_removed_on_release` | :125-129 | Sin clave previa, liberar borra la clave (respaldo `None`). |
| `test_direct_mito_executable_is_reported` | :131-133 | Ejecutable en `core/apps/main/claude/claude.exe` → `direct`. |
| `test_another_profile_takes_over_but_keeps_the_host_backup` | :135-141 | Reclamar con `work` sustituye el registro, pero conserva el respaldo del host; liberar `work` restaura el original. |
| `test_inactive_component_is_rejected` | :143-146 | Instalación no activa → `YitoError`; registro vacío. |
| `test_rows_list_candidates_with_their_state` | :148-151 | `rows` → `[('claude', 'other')]`. |
| `test_claim_watch_and_close_release` (UI) | test_protocols.py:182-211 | `ProtocolDialog`: añadir `http` (reservado) muestra error; reclamar una fila llama `claim('claude','Claude')`; tras el intervalo, la nota `Recuperado` aparece; `Cerrar` llama `release()` y cierra; `schemes` queda vacío. Usa `FakeSession`. |

## Diálogo `ProtocolDialog` (protocols_ui.py)

Constructor: `ProtocolDialog(name, entries=(), rows=(), session=None, watch=WATCH_SECONDS)`.
`WATCH_SECONDS = 5`. `BINDINGS = [('escape','close','Cerrar')]`. CSS: ancho 90, borde `#b5dc64`, fondo `#153429`,
filas `.protocol-row`, botones de alternancia de ancho 14 (`.protocol-toggle`), notas `#ff9090` en `#protocol-error`.

Widgets, en orden:

| Id / tipo | Texto o valor | Notas |
|---|---|---|
| `Label.title` | `Reclamar protocolo` | `tr` |
| `Static` (markup off) | nombre de la aplicación | |
| `Select#protocol-entry` | opciones = entradas; sin vacío | solo si hay más de una entrada; la entrada elegida se usa en `claim(scheme, self.entry)` |
| `VerticalScroll#protocol-rows` | una fila por esquema (ordenados) | ver abajo |
| `Input#protocol-scheme` | placeholder `Esquema (por ejemplo: claude)` | `tr`; enfoque inicial |
| `Button#protocol-add` | `Añadir` | `tr` |
| `Static#protocol-help` (markup off) | `Pulsa Reclamar: mientras este diálogo esté abierto, los enlaces del esquema, como el retorno de un inicio de sesión en el navegador, abrirán el componente con el entorno del profile. MITO lo comprueba cada 5 s y lo recupera si otra app lo sobrescribe. Al cerrar se restaura el registro anterior.` | `tr` |
| `Static#protocol-error` | vacío o error | color `#ff9090` |
| `Button#protocol-close` | `Cerrar` (variant primary) | `tr` |

Fila por esquema:
- `Static` (markup off) `<esquema>://  ·  <etiqueta del estado>` (p. ej. `claude://  ·  Otra app`).
- Botón de alternancia: id `protocol-toggle-<esquema con no alfanuméricos → _>`, `name=<esquema>`, clase `protocol-toggle`.
  Texto `Liberar` si está reclamado (variant success), `Reclamar` si no (variant default).
- `Static` con el comando del estado o `—` (clase `protocol-target`).
- Si está reclamado: `Static` con la nota (`notes[esquema]` o el texto por defecto) (clase `protocol-claimed`).

Comportamiento:
- Montaje: refresca filas, programa `watch` cada `watch_seconds`, enfoca el campo.
- `Añadir` / Enter: `session.describe(texto)`; si falla, error y no añade; si no, añade la fila y limpia el campo. Vacío no hace nada.
- Alternancia: si está reclamado → `session.release(scheme)`, quita la nota y redescribe; si no → `rows[scheme] = session.claim(scheme, entry)`.
  Errores `YitoError`, `OSError`, `ValueError` → mensaje en `#protocol-error`.
- `watch` (cada 5 s): para cada esquema reclamado, `session.check`; si devuelve `True`, nota `Recuperado a las HH:MM:SS: otra app lo había sobrescrito.` y repinta.
- `Cerrar`, `Esc`: `close()` → `session.release()` (libera todas) y `dismiss(None)`.
- Botón cuyo `name` está en `rows`: alterna ese esquema.

Uso desde `ui.py` (`protocol-<n>`): `unregister(root, all, write_log)`; `rows = protocols.rows(engine, profile, target)`; `session = protocols.Session(engine, profile, target, write_log)`; `ProtocolDialog(lock['key'], engine.application_entries(lock), rows, session)`. Si la liberación previa o `rows` falla: `capture(…, operation='protocols.open')` y notify con el mensaje.

## Dependencias

- `open_with`: `command` (como `launcher`), `icon_for`, `notify`, `delete_tree`.
- `i18n.tr`; `storage`: `YitoError`, `identifier`, `mutation`, `read_json`, `write_json`.
- `engine`: `profile(name)`, `selected(data)`, `application_target`, `application_entries(lock)`, `installed()`, `root`.
- `winreg`, `shlex`, `re`, `os`.
- Tests: `async_support` (Textual), `unittest` sobre HKCU real con `Software\MITO-tests-<uuid>` (ver `open_with.md`).
- `ui.py`: `protocol-<n>`, `Protocolos` (botón si la aplicación es lanzable, `protocol-button`).

## Notas para Rust

- `mito-win::protocols`: crate `winreg`. `HKLM` solo lectura. Mismo orden de búsqueda: HKCU primero (Windows usa el mismo orden).
- `snapshot` y `write_tree` deben preservar tipos y orden; los binarios van como `{"hex"}`.
- `unsafe` solo en `mito-win` si se llama a `SHChangeNotify` (ver `open_with.md`).
- `mito-tui::protocols_ui`: `ratatui` según ADR 0011. Los textos y el orden de filas se mantienen; el temporizador de 5 s pasa al bucle de eventos.
- La sesión (`Session`) es un objeto con estado; se libera siempre al cerrar el diálogo (equivalente a `Drop` seguro).

## Dudas

1. **Respaldo perdido con estado `other`.** `claim` reutiliza el respaldo solo si el estado es `mine` o `profile` (o `keep_backup`). Si la otra aplicación tomó la clave (estado `other`) y el usuario reclama de nuevo desde el diálogo (`Session.claim`, sin `keep_backup`), se toma un respaldo nuevo, que es el de la otra aplicación, y se pierde el respaldo original del host. No hay test. ¿Es intencionado?
2. **Estado `direct` con registro previo.** Mismo caso: el respaldo se reemplaza por la clave actual (que puede ser la de MITO directo). Confirmar.
3. **Mensaje de liberación sin clave.** Si la clave ya no existe, `release_record` escribe `ya apuntaba a otra aplicación`. Es engañoso; ¿otro texto?
4. **URL Protocol obligatorio.** Un host que registra el esquema sin `URL Protocol` no aparece como candidato ni como `other`. Confirmar.
5. **Comando sin `--shortcut`.** Para entradas principales el comando se escribe sin `--shortcut`, pero `status` compara exacto. Coherente, pero el test solo cubre atajo.
6. **Lanzador.** Igual que en `open_with.md` (Dudas 2): el comando depende de `core/tools/python/pythonw.exe -m mito`.
7. **Ejecutables con el mismo nombre en HKLM.** `candidates` no distingue HKCU de HKLM para la etiqueta; `current` sí. Confirmar que la etiqueta debe mostrar el origen.
8. **`read_command` silencia errores.** Cualquier `OSError` (incluido acceso denegado) se convierte en «no registrado». Revisar en el port si conviene distinguir.
9. **Intervalo fijo.** `WATCH_SECONDS = 5` no es configurable en el original. Confirmar que no debe ser clave de configuración.
