# Bundles de la interfaz MITO

Cada idioma oficial de la Unión Europea admitido tiene un JSON UTF-8. Los nombres
de archivo son los nombres nativos de `LANGUAGES` (`mito_core::config::LANGUAGES`,
reexportada por `mito-i18n`); `español.json` y `english.json` son los bundles fuente
y de fallback, respectivamente. Las claves son las plantillas originales en español,
igual que en un catálogo gettext. El bundle español conserva esos mismos textos como
valores.

Los 24 bundles se incrustan en el binario con `include_str!` (`src/bundle.rs`); no se
leen archivos en tiempo de ejecución ni se cargan traducciones desde Scoop.

Para añadir un mensaje, escribe `Message::new("Texto propio de MITO")` y añade la misma
clave a ambos bundles (y a los demás idiomas). Para valores dinámicos, usa argumentos
con nombre: `Message::new("Profile {name} creado").arg("name", name)`. No construyas
la plantilla con `format!` ni traduzcas una descripción, una nota o un valor recibido
de un catálogo. Los parámetros se sustituyen después de buscar la traducción. Conserva
exactamente los nombres de los parámetros en todos los idiomas; su orden puede cambiar.
Si el original concatena un dato tras `tr('…: ')`, usa `Message::with_suffix`.

El test `crates/mito-i18n/tests/catalogs.rs` comprueba que todos los bundles tienen las
mismas claves, valores no vacíos, los mismos marcadores y ninguna clave repetida, y que
cada plantilla `Message::new("…")` de `mito-core` está en `español.json`.

Para añadir un idioma:

1. Copia el bundle español y traduce sus valores, manteniendo todas las claves.
2. Añade el nombre finito del idioma a `LANGUAGES` en
   `crates/mito-core/src/config/properties.rs`, el archivo en la tabla `EMBEDDED` de
   `crates/mito-i18n/src/bundle.rs` y su caché. Configuración y empaquetado utilizan
   esa lista; no se aceptan nombres de archivo arbitrarios. Amplía también la tabla de
   `language_for_langid` (`crates/mito-i18n/src/translator.rs`) si debe seleccionarse
   automáticamente para ese idioma de Windows.
3. Amplía las pruebas de `crates/mito-i18n/src/tests.rs` y verifica diálogos, avisos,
   acciones y mensajes de workers, además del espacio disponible.

`system` es la selección predeterminada, no un bundle: consulta el idioma de interfaz
del usuario de Windows (`GetUserDefaultUILanguage`). Las variantes regionales de los
idiomas oficiales de la UE usan su bundle; cualquier idioma desconocido o un fallo de
detección usa `english`. La selección manual (`mito.language`) tiene prioridad.

El gestor mantiene su idioma durante la sesión. Guardar otra selección requiere
volver a abrirlo. No cambia terminales o aplicaciones abiertas. No traduzcas
tokens internos, nombres de propiedades, IDs, rutas, scripts ni salidas externas.
Los mensajes de servicios son un `Message` con la plantilla española: la TUI los
traduce con `Translator::tr`; la CLI y el log conservan el texto fuente (`Display`).

Las traducciones ausentes o con parámetros distintos buscan primero en inglés y,
si tampoco existen ahí, vuelven al texto fuente.
