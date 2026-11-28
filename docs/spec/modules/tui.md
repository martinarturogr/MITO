# mito-tui: API y reparto (F6)

Diseño Rust de la interfaz. El comportamiento está en [ui.md](ui.md) y
[ui_dialogs.md](ui_dialogs.md); la arquitectura en [ADR 0011](../../adr/0011-tui-con-ratatui.md).
Ante una diferencia entre este documento y los contratos, mandan los contratos salvo
en lo que aquí se decide explícitamente.

## Alcance de F6 (decidido con el propietario)

- **Se oculta** todo lo de fases posteriores (no se dibuja, ni deshabilitado):
  Pocketools, `MCP - disabled`, `MCP · Jobs`, estado de Internet, `Actualizar MITO`,
  «Agregar aplicación del host», filas del host, «Acc. directo», «Extensiones»,
  «Protocolos», «Host · ✓/✗», el selector «Proceso» (siempre `detached`),
  «Exportar», «Importar», «Acceso directo» y «Archivos de configuración» de
  «Gestionar profile». Cada fase los añade en su posición original.
- **«Versiones…» pasa a F11** (depende de `versions.py`). En F6 no se dibuja.
  «Liberar» (versión retenida en Info) sí es de F6.
- Avisos de hooks en el detalle y en Info (F7) **sí** se muestran: son texto.
- Bugs del original que se corrigen (documentar en PARITY): «Abrir» sin entradas de
  aplicación no falla (el original leía `result[2]`); `Esc` durante el diálogo de
  progreso o de paso largo no cambia de página.
- Etiquetas literales del original (`·`, `✓`, `—`, `path`) se conservan sin traducir.

## Módulos y dueños

```
crates/mito-tui/src/
  lib.rs            pub use app::run                                    (Opus, esqueleto)
  theme.rs          colores y estilos del CSS original                   B
  input.rs          Input: tecla, clic, rueda, pegar (desde crossterm)   B
  widgets/          kit: foco, zonas, botón, casilla, campo, selector,   B
                    tabla, texto con scroll, log, cabecera, modal, aviso
  dialogs/          un archivo por diálogo + trait Dialog                B
  model/            datos de Inicio y del catálogo (sin ratatui)          D
  worker.rs         acciones en hilo, confirmaciones y resultado         D
  app/              estado, update, render, bucle de eventos             C
```

B y D trabajan en paralelo (archivos distintos); C integra después. Nadie más que
C toca `lib.rs`, `app/` ni el binario.

## Tipos compartidos

### Traducción

Toda cadena visible pasa por `&mito_i18n::Translator`: `tr.text("Clave")` para textos
fijos y `tr.tr(&Message)` para plantillas con argumentos. Los argumentos que son datos
(claves, versiones, rutas) no se traducen. Los errores se describen con
`model::describe(&mito_engine::Error, &Translator) -> String`: si `error.message()`
devuelve un `Message` se traduce; si no, `error.to_string()`.

### Entrada (`input.rs`, B)

```rust
pub enum Input {
    Key(crossterm::event::KeyEvent),
    Click { column: u16, row: u16 },
    Scroll { column: u16, row: u16, down: bool },
    Paste(String),
}
impl Input { pub fn from_event(event: crossterm::event::Event) -> Option<Self>; }
```

Solo cuentan pulsaciones (`KeyEventKind::Press`), clics izquierdos al soltar o pulsar
(`MouseEventKind::Down(Left)`) y la rueda.

### Kit de widgets (`widgets/`, B)

Modo inmediato con estado retenido: cada widget es un `struct` con su estado y un
`render(&self, frame, area, ui: &mut Ui)` que dibuja y **registra** su zona y su
posición en el orden de foco. El foco y las zonas se reconstruyen en cada fotograma;
el id enfocado persiste entre fotogramas.

```rust
pub type WidgetId = String;              // ids del original: "install", "remove-3", …

/// Contexto de un fotograma de una pantalla (principal o diálogo).
pub struct Ui<'t> { pub tr: &'t Translator, /* zonas y orden de foco */ }
impl Ui<'_> {
    pub fn focusable(&mut self, id: &str, area: Rect);   // orden de registro = orden de Tab
    pub fn is_focused(&self, id: &str) -> bool;
}

/// Foco y zonas de una pantalla, que sobreviven entre fotogramas.
pub struct Focus { /* id enfocado, orden y zonas del último fotograma */ }
impl Focus {
    pub fn focused(&self) -> Option<&str>;
    pub fn set(&mut self, id: &str);
    pub fn next(&mut self); pub fn previous(&mut self);   // Tab / Mayús+Tab, cíclico
    pub fn hit(&self, column: u16, row: u16) -> Option<&str>;  // zona más interna
    pub fn frame<'t>(&mut self, tr: &'t Translator) -> Ui<'t>; // empieza un fotograma
}
```

Un id deshabilitado u oculto no se registra (no recibe foco ni clics). Si el id
enfocado desaparece, el foco pasa al primero registrado.

Widgets mínimos (todos con `id`, `disabled`):

| Widget | Estado | Teclas | Notas |
|---|---|---|---|
| `Button` | `label`, `variant: Default \| Primary \| Success \| Warning \| Error` | Intro, Espacio | Clic = activar. Colores por variante del tema. Enfocado: invertido. |
| `Checkbox` | `label`, `value` | Intro, Espacio | `▣`/`☐` (o `[X]`/`[ ]`) en `#69FF64` encendido, `#8A4B50` apagado. |
| `TextInput` | `value`, `cursor`, `placeholder`, `max_length: Option<usize>` | caracteres, ←→, Inicio/Fin, Retroceso/Supr, pegar | Intro emite `Submitted`. Placeholder atenuado si vacío. Cuenta en `char`. |
| `Select<T>` | `options: Vec<(String, T)>`, `selected: Option<usize>`, `prompt`, `open` | Intro/Espacio abre; ↑↓ y Intro eligen; Esc cierra | Lista desplegable superpuesta (se dibuja la última). |
| `Table` | `columns`, `rows: Vec<TableRow { key, cells }>`, `cursor`, `offset` | ↑↓, RePág/AvPág, Inicio/Fin | Filas alternas (zebra), cursor de fila; clic selecciona fila; rueda desplaza. Emite `Highlighted(key)` al mover el cursor. |
| `TextView` | `lines: Vec<Line>`, `scroll` | ↑↓, RePág/AvPág | Solo lectura (manifiesto, detalle, descargas). `scroll_home()`. |
| `LogView` | entradas `(texto, error: bool)`, auto al final | ↑↓, RePág/AvPág, rueda | Errores en rojo y negrita; ajuste de línea; sin markup. |

Eventos de widget: `enum WidgetEvent { Activated(WidgetId), Toggled(WidgetId, bool),
Changed(WidgetId), Submitted(WidgetId), Highlighted(WidgetId, String) }`. Cada widget
expone `handle(&mut self, input: &Input) -> Option<WidgetEvent>` para cuando tiene el
foco; los clics los resuelve la pantalla con `Focus::hit` y luego `click(&mut self)`.

Utilidades:

- `modal::area(screen: Rect, width: Size, height: Size) -> Rect` (centrado; `Size::Cells(n)`,
  `Size::Percent(n)`, `Size::Auto(n)` con máximo), `modal::frame(frame, area, border: Color)`
  que oscurece el fondo (`Clear` + superposición) y dibuja el panel `#153429` con borde
  redondeado.
- `header::render(frame, area, ui, title, close_id)`: relleno de 3, título centrado en
  negrita `#b5dc64` y botón `×` rojo `#ff7070` (con id `close_id`, registrable).
- `Notice { text, severity: Info | Warning | Error, until: Instant }` y
  `notice::render(frame, screen, &[Notice])`: avisos apilados abajo a la derecha (los
  `notify` del original; 5 s por defecto, 10 s el de idioma).

### Tema (`theme.rs`, B)

Constantes `Color::Rgb` con los valores de ui.md («Estilo»): `SCREEN #102820`,
`TEXT #e7efdf`, `HEADER #215139`, `SIDEBAR #153429`, `TITLE #b5dc64`, `DETAILS #1b3b2e`,
`ROW_DARK #1b3b2e`, `ROW_LIGHT #29513d`, `SUMMARY #95a9bd`, `LOG_BORDER #264858`,
`BUSY #FFD166`, `CHECK_ON #69FF64`, `CHECK_OFF #8A4B50`, `BORDER #b5dc64`,
`BORDER_DANGER #e27878`, `BORDER_WARNING #e5ba65`, `ERROR_TEXT #ff7070`,
`BADGE_ERROR #ff9090`, `EMPTY_CATALOG #4da6ff`, y los colores de las variantes de botón
(Textual: primary `#0178D4`, success `#4EBF71`, warning `#ffa62b`, error `#ba3c5b`).

### Diálogos (`dialogs/`, B)

```rust
pub trait Dialog {
    /// Dibuja el diálogo sobre la pantalla completa `screen`.
    fn render(&mut self, frame: &mut Frame, screen: Rect, ui: &mut Ui);
    /// Procesa una entrada; el diálogo usa su propio `Focus`.
    fn handle(&mut self, input: &Input, context: &mut Context) -> Step;
    /// Tic periódico (cada 250 ms) para relojes y cierres diferidos.
    fn tick(&mut self, context: &mut Context) -> Step { Step::Stay }
    fn focus(&mut self) -> &mut Focus;
}
pub enum Step { Stay, Close(Outcome) }
/// Lo que un diálogo necesita del exterior: traducción, avisos y la instancia.
pub struct Context<'a> { pub tr: &'a Translator, pub engine: &'a Engine, pub notices: &'a mut Vec<Notice> }
pub enum Outcome {
    Closed,                                    // manifiesto, integridad, paso largo, progreso
    Confirmed(bool),                           // nueva versión, eliminar, sin hash, borrar profile
    Release,                                   // Info → «Liberar»
    Open { arguments: Vec<String>, entry: Option<String> },   // Abrir aplicación
    Saved(bool),                               // configuración
    Profile(ProfileChoice),                    // Gestionar profile
}
pub enum ProfileChoice { Cancel, Create(String), Configuration, Delete }
```

`Esc` en un diálogo cancelable cierra con su valor de cancelación. Diálogos de F6, cada
uno con su constructor y comportamiento de ui.md / ui_dialogs.md:

| Diálogo | Constructor | Resultado |
|---|---|---|
| `ManifestDialog` | `(key, &Value)` | `Closed`; JSON con indentación 4 y `ensure_ascii=False` (como `json.dumps`) en `TextView` sin ajuste, con scroll horizontal opcional |
| `InstallVersionDialog` | `(key, previous, version)` | `Confirmed` |
| `RemoveComponentDialog` | `(&Record, profiles: Vec<String>)` | `Confirmed`; foco inicial en «Cancelar» |
| `UnverifiedDownloadsDialog` | `(Vec<String>)` | `Confirmed`; foco en «Cancelar» |
| `DownloadIntegrityDialog` | `(model::IntegrityInfo)` | `Closed` |
| `ComponentInfoDialog` | `(&Engine, &Record, profile: Option<&str>)` | `Release` o `Closed`; «Abrir» de rutas llama a `mito_core::persistence::open_location` y avisa del error |
| `OpenApplicationDialog` | `(name, entries: Vec<String>)` | `Open`; argumentos con `mito_engine::arguments::split`; sin selector «Proceso» |
| `ConfigurationDialog` | `(&Engine, profile: Option<&str>)` | `Saved`; usa `mito_core::config::Configuration` (lectura, `validated`, `save`) |
| `ManageProfileDialog` | `(profile: Option<&str>, can_delete: bool)` | `Profile`; solo «Crear profile», «Configuración» y «Eliminar profile» |
| `DeleteProfileDialog` | `(profile)` | `Confirmed`; foco en «Cancelar» |
| `LongStepDialog` | `(Message)` | sin `Esc`; `finish()` y reloj «Tiempo transcurrido: {p1} s» |
| `OperationDialog` | `(Operation)` | sin `Esc`; lee `operation.state()` en cada tic; «Cancelar instalación» llama a `operation.cancel()`; `human_size` y velocidad de ui.md |

`IntegrityInfo` es de `model` (D); B la recibe ya construida.

Las pruebas de B usan `ratatui::backend::TestBackend` y `insta` para el aspecto de cada
diálogo (snapshots de texto de 80×24 o 100×30) y simulan entradas para su resultado.

### Modelo (`model/`, D)

Sin ratatui. Funciones puras o que solo leen del motor, para que C las dibuje.

```rust
/// Inicio.
pub struct HomeRow {
    pub installation: String, pub key: String, pub version: String, pub pinned: bool,
    pub active: bool, pub can_open: bool,
    pub update: Option<Candidate>,          // `available_update` con el catálogo actual
}
pub struct Home { pub rows: Vec<HomeRow>, pub active: usize, pub pending: Vec<String> }
pub fn home(engine: &Engine, profile: Option<&str>, catalog: &BTreeMap<String, Manifest>) -> Result<Home>;
pub fn summary(home: &Home, profile: Option<&str>, tr: &Translator) -> String;  // texto de #installed-summary (0 del host)
pub fn row_label(row: &HomeRow, tr: &Translator) -> String;  // '{key}   ·   {version}' + retenida

/// Catálogo.
pub struct Catalog { /* entradas (key, manifest, compatibilidad), deshabilitados, ruta del filtro */ }
impl Catalog {
    pub fn load(engine: &Engine) -> Result<Self>;              // `load_catalog`
    pub fn buckets(engine: &Engine) -> Vec<String>;            // nombres de los filtros
    pub fn disabled(&self) -> &BTreeSet<String>;
    pub fn set_enabled(&mut self, bucket: &str, enabled: bool) -> Result<()>; // escribe catalog-filters.json
    pub fn filter(&self, term: &str, installed: &BTreeSet<String>, tr: &Translator) -> Listing;
    pub fn details(&self, key: &str, installed: bool, tr: &Translator) -> Option<Details>;
    pub fn manifest(&self, key: &str) -> Option<&Value>;
    pub fn version(&self, key: &str) -> Option<String>;        // la que vio el usuario
}
pub struct Listing { pub rows: Vec<CatalogRow>, pub summary: String, pub empty_hint: Option<String>, pub details: String }
pub struct CatalogRow { pub key: String, pub version: String, pub availability: String, pub license: String, pub compatible: bool, pub installed: bool }
pub struct Details { pub text: String, pub can_install: bool }  // texto exacto de on_data_table_row_highlighted
pub fn license_label(manifest: &Value, tr: &Translator) -> String;
pub fn describe(error: &mito_engine::Error, tr: &Translator) -> String;
/// Datos de una descarga alterada; `from_error` recorre la cadena `source()` buscando
/// `mito_transfer::Error::Integrity`.
pub struct IntegrityInfo { pub url: String, pub size: Option<u64>, pub host: String, pub portal: bool }
impl IntegrityInfo { pub fn from_error(error: &mito_engine::Error) -> Option<Self>; }
```

### Trabajo en hilo (`worker.rs`, D)

```rust
pub enum Action {
    CheckUpdates,                                  // refresca manifiestos de instalados (fuerza el diario)
    Create(String), DeleteProfile,
    Toggle { installation: String, active: bool },
    Install(String),                               // install_from_catalog
    RemoveComponent(String), UpdateComponent(String), UpdateAll(Vec<String>),
    ReleaseVersion(String),
    Launch { installation: String, arguments: Vec<String>, shortcut: Option<String> },
    Refresh,                                       // importa todos los buckets de sources()
    StartupUpdates,                                // check_component_updates de on_mount (diario)
}
pub enum Request { InstallVersion { key: String, previous: String, version: String }, Unverified(Vec<String>) }
pub enum Report {
    Log { message: Message, error: bool, long_step: Option<bool> },
    Confirm { request: Request, answer: std::sync::mpsc::Sender<bool> },
    Started(Operation),                            // la operación en curso, para el diálogo de progreso
    Done { profile: Option<String>, failure: Option<Failure> },
}
pub struct Failure { pub cancelled: bool, pub integrity: Option<IntegrityInfo> }
pub fn spawn(engine: Arc<Engine>, action: Action, profile: Option<String>, reports: Sender<Report>) -> JoinHandle<()>;
```

- Cada acción reproduce la tabla «Tabla de acciones del worker» de ui.md con las
  llamadas Rust del motor (`Engine::install` con `InstallOptions { profile,
  confirm_plan, confirm_unverified }`, `remove`, `update`, `update_all`,
  `release_version`, `set_active`, `create_profile`, `delete_profile`, `launch`,
  `Catalog::import_bucket`/`refresh_keys`/`refresh_installed` con `DownloaderFetcher`)
  y sus mensajes exactos.
- Las mutaciones toman `MutationLock` como la CLI (`refresh`, y las que lo tomen en el
  motor ya lo hacen dentro).
- `Cancelled` → log error «Instalación cancelada. No se ha publicado nada.»; otro error →
  `error_tracking::capture` con `operation = "ui." + acción` y log error «Error: {p1}»
  (con `describe`); integridad → `Failure.integrity`.
- Las confirmaciones bloquean el hilo en un canal hasta que la interfaz responde; si la
  interfaz se cierra (canal roto) cuentan como `false`.
- `Done` siempre llega, también tras un pánico (captura con `catch_unwind` o hilo
  vigilante) para no dejar la interfaz ocupada.
- `StartupUpdates`/`CheckUpdates` usan `mito_catalog::checks::daily` con la clave
  `component-updates` como el original y registran «Comprobando actualizaciones de
  componentes instalados…» y «Manifiestos de componentes instalados comprobados: {p1}.».

Las pruebas de D usan una instancia temporal con un bucket local (como los tests del
motor) y un `Downloader` falso; no hay red.

### Aplicación (`app/`, C)

`pub fn run(engine: Engine, profile: Option<String>) -> Result<(), TuiError>`:
inicializa la terminal (modo crudo, pantalla alternativa, ratón, pegado), restaura
siempre al salir (también con pánico), y ejecuta el bucle: eventos de crossterm con
`poll` de 50 ms, tics de 250 ms, informes del hilo y redibujado.

`App` reproduce `YitoApp` de F6: barra lateral (sin lo oculto), páginas Inicio y
Catálogo, panel de log con «Copiar log» (`mito_win::clipboard::set_text`), cabecera
«MITO · {versión}» con el subtítulo y pie con los atajos traducidos («Ctrl+Q Salir»,
«Ctrl+F Buscar componentes», «Esc Inicio»). Ocupado, indicador a los 2 s, diálogo de
progreso a los 2 s, paso largo a los 2 s, avisos, `refresh_terminal_default` con
`mito_terminal::settings::prepare`, `apply_safely` al arrancar y
`ensure_default_profile` antes de abrir.

## Alcance de F8 (integración con Windows)

F8 activa lo que F6 ocultó y que ahora implementa `mito-desktop`. Sigue oculto lo de
F9-F12 (red, Pocketools, MCP, «Actualizar MITO», «Exportar», «Importar» y «Archivos de
configuración»).

- **`integration::Integration`** reúne lo que toca el sistema: escritorio, `AppData` del
  host, rama del registro de «Abrir con» y de protocolos, búsqueda de aplicaciones del
  host, selectores nativos y lanzamiento del host. `Integration::system()` usa lo real;
  las pruebas la sustituyen con `App::with_integration` (carpetas temporales,
  `Classes::under("Software\MITO-tests\…")` y selectores falsos). El hilo de trabajo la
  recibe con `worker::spawn_integrated`.
- **Acciones nuevas** (`worker/desktop.rs`): `HostAdd`, `HostLink { identity, kind }`
  (`host-cache`/`host-data`), `OpenWith`, `HostExpose`, `HostOpen`, `HostRemove`,
  `HostShortcut`, `MitoShortcut` y `ApplicationShortcut`; `Launch` gana `coupled`.
  Las preguntas de enlaces llegan por `Request::HostConfirm { message, label }`
  (`HostDataConfirm`) y la elección de aplicación del host por `Report::Pick` con
  respuesta `Pick::{Path, Browse, Cancel}`.
- **Diálogos nuevos**: `ShortcutDialog`, `HostDiscoverDialog`, `HostInfoDialog`,
  `RemoveHostDialog`, `HostDataConfirm`, `OpenWithDialog` y `ProtocolDialog`;
  `OpenApplicationDialog` gana el selector «Proceso» (`detached`/`attached`, literal) y
  «Gestionar profile» el botón «Acceso directo».
- **`ProtocolDialog`** habla con un `ProtocolBackend`. El real, `ThreadedSession`, deja la
  `mito_desktop::protocols::Session` (que toma prestado el motor) en su propio hilo; al
  soltarse se cierra el canal y la sesión libera lo reclamado. El tic del diálogo vigila
  cada `WATCH_SECONDS` (5 s).
- **Inicio**: cada elemento ocupa tres filas (nombre y dos líneas de botones que fluyen
  por el ancho), porque los ocho botones de un componente activo no caben en una línea
  de 80 columnas. Los estados (`Extensiones ✓/✗`, `Host · ✓/✗`, `Cache · ✓/✗`,
  `Data · ✓/✗`) se calculan en `model::host_state` al refrescar Inicio y cualquier error
  de un estado cuenta como `✗`; solo la lectura de `host-applications.json` se registra
  como error. Las filas del host usan `#593300`/`#8f5000`.
- **Decisiones**: en la pregunta de «Host · …» el `{p0}` es `Roaming`/`Local`
  (`Kind::folder`, el `KINDS` de `host_data` del contrato) y no «Data (AppData/Roaming)»
  como resume `ui.md`; el diálogo de descubrimiento mueve la lista con ↑↓ aunque el foco
  esté en el filtro; la lista de «Abrir con» y la de descubrimiento son una `Table` sin
  cabecera.
