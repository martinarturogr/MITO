# native_ui.py → mito-tui::native_ui

Origen: `core/app/mito/native_ui.py` (15 líneas), `core/tests/test_i18n.py` (test `test_native_labels_are_escaped_as_data`,
líneas 152-156; la comprobación estática de etiquetas en líneas 40-70). Consumidores: `host_apps.py` (`choose_file`),
`host_data.py` (`choose_folder`), `network.py` (`prompt_credentials`), `profile_files_ui.py` (`choose_path`),
`profile_transfer_ui.py` (`choose_export_file`).

## Responsabilidad

Localiza las etiquetas fijas de los diálogos nativos de Windows (`FolderBrowserDialog`, `OpenFileDialog`,
`System.Windows.Forms.Form`) que van dentro de scripts de PowerShell. Solo sustituye literales completos de cadena
entre comillas simples. El texto traducido se escapa como dato: las comillas simples se duplican, así que una
traducción no puede cerrar el literal ni inyectar código. Rutas y credenciales no pasan por aquí: llegan por stdin
o por la salida JSON.

## API pública del original

| Función | Entradas | Salida | Errores |
|---|---|---|---|
| `localize_script(script, *labels, translate=tr)` (10) | texto del script, etiquetas (cadenas en español), función de traducción | texto del script con literales sustituidos | ninguno propio |

Implementación literal:

```python
def localize_script(script, *labels, translate=tr):
    def literal(value):
        return "'" + value.replace("'", "''") + "'"
    for label in labels:
        script = script.replace(literal(label), literal(translate(label)))
    return script
```

Reglas:
1. Por cada etiqueta, en el orden de los argumentos, se reemplazan **todas** las apariciones de `'<etiqueta>'` por `'<traducción>'`.
2. El literal se compone como `'` + valor con `'` duplicado + `'`. La traducción se escapa igual.
3. Si la etiqueta no aparece en el script, no pasa nada. Si `translate` devuelve la misma cadena (fallback), el script no cambia.
4. `$`, `` ` ``, `"` y caracteres Unicode no necesitan escape: el literal es de comillas simples (sin expansión de PowerShell).
5. Los argumentos posicionales se sustituyen en el orden dado. Si una sustitución crea un literal que coincide con una
   etiqueta posterior, esa segunda pasada sí aplica (no ocurre en los scripts actuales).

Ejemplo del test: `localize_script("$title = 'Open'", 'Open', translate=lambda _: "User's $folder")` → `"$title = 'User''s $folder'"`.

## Convención de ejecución común

Todos los scripts se ejecutan como:

```
powershell.exe -NoLogo -NoProfile -STA -Command <script>
```

(`network.prompt_credentials` usa además `encoding='utf-8'` sin `text=True` y sin `check`; ver la tabla). Salida: JSON de una cadena (o de un objeto en red). Si
la salida está vacía, el resultado es `None` (cancelación). Los scripts empiezan con un salto de línea tras la comilla de
apertura (`r'''`, `'''` o `r"""`); esa línea vacía forma parte del contrato.

| Script | Función | Entrada por stdin | Labels (`localize_script`) | Salida | `check` |
|---|---|---|---|---|---|
| A | `host_apps.choose_file()` | ninguna | `MITO - Agregar aplicación del host` y el filtro | JSON cadena o vacío | `True` |
| B | `host_data.choose_folder(kind)` | carpeta inicial (`str(host_folder(kind))`) | `MITO - Selecciona la carpeta del host para enlazar` | JSON cadena o vacío | `True` |
| C | `network.prompt_credentials(failed, translate)` | ninguna | `MITO - Autenticación de red`, texto de ayuda, `Conectar`, `Cancelar` | JSON `{"username","password"}` si `returncode == 0` y hay salida; si no `None` | no (`returncode` manual) |
| D | `profile_files_ui.choose_path(initial, folder=True)` | carpeta inicial | `MITO - Carpeta de configuración` | JSON cadena o vacío | `True` |
| E | `profile_files_ui.choose_path(initial, folder=False)` | carpeta inicial | `MITO - Archivo de configuración` | JSON cadena o vacío | `True` |
| F | `profile_transfer_ui.choose_export_file(initial)` (importar) | carpeta inicial | `MITO - Importar profile` y el filtro | JSON cadena o vacío | `True` |

Nota del nombre: la función F se llama `choose_export_file` pero abre `OpenFileDialog` con título `MITO - Importar profile`
(es el selector de importación). Ver Dudas.

## Tabla de etiquetas

| Clave (español, exacta) | Script | ¿Pasa por `tr`? | ¿Comprobada por `test_i18n`? |
|---|---|---|---|
| `MITO - Agregar aplicación del host` | A | sí | sí (argumento constante posicional) |
| `Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)\|*.exe;*.bat;*.cmd;*.ps1` | A | sí | sí |
| `MITO - Selecciona la carpeta del host para enlazar` | B | sí | sí |
| `MITO - Autenticación de red` | C | sí (`translate=` del `Translator` de la raíz) | sí |
| `Usuario y contraseña del portal. Cancelar continúa sin conexión.` | C | sí | sí |
| `No se pudo verificar Internet. Reintenta o cancela para continuar.` | C (solo si `failed`) | sí | sí |
| `Conectar` | C | sí | sí |
| `Cancelar` | C | sí | sí |
| `MITO - Carpeta de configuración` | D | sí | no (argumento `*labels`) |
| `MITO - Archivo de configuración` | E | sí | no (argumento `*labels`) |
| `MITO - Importar profile` | F | sí | sí |
| `Profiles MITO (*.mito.json;*.json;*.zip)\|*.mito.json;*.json;*.zip\|Todos los archivos (*.*)\|*.*` | F | sí | sí |

Las etiquetas que no pasan por `tr` no existen en esta tabla. Los títulos del resto de la interfaz están en otros contratos.

Notas sobre `test_i18n`: el test recorre los `.py` y comprueba que los argumentos **constantes** de `localize_script`
(posicionales tras el script) están en el catálogo en español. Los argumentos `*labels` no se comprueban estáticamente.

## Scripts literales

Cada bloque reproduce el texto exacto del original. El único cambio en tiempo de ejecución es `localize_script` (y, en C, el
reemplazo previo de la frase de ayuda si `failed`).

### A. `host_apps.choose_file()` (host_apps.py:138-160)

Sin entrada. Tras ejecutar, `localize_script(script, 'MITO - Agregar aplicación del host', 'Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)|*.exe;*.bat;*.cmd;*.ps1')`.

```powershell

[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'MITO - Agregar aplicación del host'
$dialog.Filter = 'Aplicaciones y scripts (*.exe;*.bat;*.cmd;*.ps1)|*.exe;*.bat;*.cmd;*.ps1'
$dialog.InitialDirectory = [Environment]::GetFolderPath('UserProfile')
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.FileName -Compress
    }
} finally { $dialog.Dispose() }
```

Llamada: `subprocess.run(['powershell.exe','-NoLogo','-NoProfile','-STA','-Command', script], capture_output=True, text=True, encoding='utf-8', check=True, creationflags=CREATE_NO_WINDOW)`. Resultado: `json.loads(stdout)` si hay salida, si no `None`.

### B. `host_data.choose_folder(kind)` (host_data.py:202-222)

Entrada: `input=str(host_folder(kind))`. Localización: `localize_script(script, 'MITO - Selecciona la carpeta del host para enlazar')`.

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

Llamada: `subprocess.run([...,'-STA','-Command', script], input=..., capture_output=True, text=True, encoding='utf-8', check=True, creationflags=CREATE_NO_WINDOW)`.

### C. `network.prompt_credentials(failed=False, translate=tr)` (network.py:436-480)

Texto base (plantilla de `r"""`):

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

Si `failed` es verdadero, antes de localizar se hace `script.replace('Usuario y contraseña del portal. Cancelar continúa sin conexión.', 'No se pudo verificar Internet. Reintenta o cancela para continuar.')`
(sustitución de texto plano, no de literal). Después:

```python
localize_script(script, 'MITO - Autenticación de red',
    'Usuario y contraseña del portal. Cancelar continúa sin conexión.',
    'No se pudo verificar Internet. Reintenta o cancela para continuar.',
    'Conectar', 'Cancelar', translate=translate)
```

Llamada: `subprocess.run(['powershell.exe','-NoLogo','-NoProfile','-STA','-Command', script], capture_output=True, encoding='utf-8', creationflags=CREATE_NO_WINDOW)`. Sin `check` y sin `text=True`. Resultado: `json.loads(stdout)` si `returncode == 0` y hay salida; si no `None`. La contraseña viaja por la salida JSON; el diálogo solo la ve el usuario.

### D. `profile_files_ui.choose_path(initial_directory, folder=True)` (carpeta)

Literal `r"""` (sin sustituciones de contenido salvo `localize_script(script, 'MITO - Carpeta de configuración')`):

```powershell

[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'MITO - Carpeta de configuración'
$dialog.ShowNewFolderButton = $false
$dialog.SelectedPath = [Console]::In.ReadToEnd()
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.SelectedPath -Compress
    }
} finally { $dialog.Dispose() }
```

### E. `profile_files_ui.choose_path(initial_directory, folder=False)` (archivo)

```powershell

[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'MITO - Archivo de configuración'
$dialog.InitialDirectory = [Console]::In.ReadToEnd()
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.FileName -Compress
    }
} finally { $dialog.Dispose() }
```

Ambas variantes (D y E) se ejecutan con `input=str(initial_directory)`, `check=True`, `-STA`.

### F. `profile_transfer_ui.choose_export_file(initial_directory)` (importar profile)

Localización: `localize_script(script, 'MITO - Importar profile', 'Profiles MITO (*.mito.json;*.json;*.zip)|*.mito.json;*.json;*.zip|Todos los archivos (*.*)|*.*')`.

```powershell

[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'MITO - Importar profile'
$dialog.Filter = 'Profiles MITO (*.mito.json;*.json;*.zip)|*.mito.json;*.json;*.zip|Todos los archivos (*.*)|*.*'
$dialog.InitialDirectory = [Console]::In.ReadToEnd()
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.FileName -Compress
    }
} finally { $dialog.Dispose() }
```

Llamada: `input=str(initial_directory)`, `check=True`, `-STA`.

## Formatos de disco

No hay archivos. El único dato persistente que intervienen es el JSON de salida (cadena de ruta o objeto de credenciales)
y el stdin con la carpeta inicial.

## Invariantes

1. El script nunca recibe rutas ni credenciales dentro del texto: las rutas van por stdin, o las credenciales salen por stdout.
2. Un literal traducido nunca puede romper la cadena de PowerShell: las comillas simples se duplican.
3. `localize_script` solo sustituye literales completos delimitados por comillas simples. Una etiqueta que aparezca dentro de
   un texto más largo, o sin las comillas que la delimitan, no se toca.
4. Sin traducción disponible, `tr` devuelve la clave español y el script queda igual al original.
5. Cancelación del usuario = salida vacía = `None` (A, B, D, E, F). En C, código distinto de 0 también = `None`.

## Casos límite y errores

- `CalledProcessError` de A, B, D, E, F se propaga (no está capturado en el módulo); los llamadores de UI lo gestionan.
- Si PowerShell no está disponible: `FileNotFoundError` (no capturado).
- JSON inválido en la salida: `json.JSONDecodeError` (no capturado).
- Una traducción con `'` → `''` en el resultado (probado).
- Una etiqueta que contenga `|` (filtros A y F): la traducción debe conservar los patrones `*.ext` (ver Dudas).

## Casos de test del original

| Test | Archivo:líneas | Qué comprueba |
|---|---|---|
| `test_native_labels_are_escaped_as_data` | test_i18n.py:152-156 | `localize_script("$title = 'Open'", 'Open', translate=lambda _: "User's $folder")` → `"$title = 'User''s $folder'"`. |
| (comprobación estática de etiquetas) | test_i18n.py:55-62 | Para cada `localize_script(…)` en el código, los argumentos constantes posicionales deben existir en el catálogo de origen. |

## Dependencias

- `i18n.tr` (fallback y claves). En C, `Translator.from_root(root)` (idioma de `config.properties`).
- Ningún otro módulo del original.
- Consumidores: `host_apps.py:155`, `host_data.py:217`, `network.py:473`, `profile_files_ui.py:46`, `profile_transfer_ui.py:27`.

## Notas para Rust

- `mito-tui::native_ui`: una función `localize_script(script: &str, labels: &[&str], translate: impl Fn(&str) -> String) -> String`
  con la misma regla de literal (`'` → `''`) y reemplazo de todas las apariciones.
- Los scripts pueden ser constantes `&'static str` o archivos `.ps1` versionados (`include_str!`). Cualquier cambio de un
  carácter cambia el contrato: se comparan con snapshot de `insta` (AGENTS.md).
- Ejecución: `std::process::Command` con `-NoLogo -NoProfile -STA -Command`, `creation_flags(0x08000000)` (CREATE_NO_WINDOW),
  stdin `write_all` para B, D, E, F; `encoding` UTF-8 en stdout.
- Credenciales (C): leer el JSON, usar `zeroize` o `secrecy` en el tipo y no registrar la salida en logs.
- `mito-tui` no usa `unsafe`; la llamada al proceso va en `mito-win` si se comparte con otros módulos (decidir en ADR).

## Dudas

1. **Nombre de F.** `choose_export_file` abre el selector de **importación** (título `MITO - Importar profile`). El nombre
   parece un error de copia. En el port conviene nombrarlo `choose_import_file` y documentar la decisión.
2. **Filtros con `|`.** A y F traducen el filtro completo. Si un traductor cambia `*.exe` o `*.mito.json`, el selector deja de
   filtrar. Ideal: no traducir la parte de patrones (separar la etiqueta visible de los patrones) o bloquear la clave en revisión.
   Hoy el original lo traduce entero.
3. **Etiquetas no comprobadas.** D y E pasan `*labels`, así que `test_i18n` no comprueba que existan en el catálogo.
   Confirmar que las 24 traducciones contienen `MITO - Carpeta de configuración` y `MITO - Archivo de configuración`.
4. **Contraseña en memoria.** C devuelve la contraseña por stdout y `prompt_credentials` solo limpia `credentials` en Python;
   la cadena persiste en memoria, y en PowerShell el `TextBox` se limpia pero no el objeto `String`. Revisar en el port.
5. **Salida vacía con código 0 en C.** Si el usuario acepta sin texto, el script devuelve `{"username":"","password":""}`;
   `prompt_credentials` lo devuelve como credenciales válidas. Confirmar que `network.authenticate` lo trata como fallo.
6. **`failed` antes de `localize_script`.** La sustitución de la frase de ayuda es sobre texto español; si una traducción
   contiene la frase española, no se mapeará. Caso improbable, sin test.
7. **Texto de la etiqueta de C en `$l.Text`.** Si `failed`, la frase cambia de texto (`No se pudo verificar Internet…`).
   Ambas frases están en el catálogo. Confirmar que la traducción de la frase de error sigue sin contener `'`.
8. **Inconsistencia de flags.** A, B, D, E, F usan `-STA` y `check=True`; C no usa `check` ni `text=True` y devuelve `None` si el
   código es distinto de 0. Es intencional según el texto original; documentar.
