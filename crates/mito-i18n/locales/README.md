# Bundles de la interfaz MITO

> Copiado del MITO original. Las referencias a `tr()`, `i18n.py` y
> `core/tests/test_i18n.py` describen el API Python; se adaptarán al crate
> `mito-i18n` en la fase F1.

Cada idioma oficial de la Unión Europea admitido tiene un JSON UTF-8. Los nombres
de archivo son los nombres nativos definidos en `LANGUAGES`; `español.json` y
`english.json` son los bundles fuente y de fallback, respectivamente.
Las claves son las plantillas originales en español, igual que en un catálogo
gettext. El bundle español conserva esos mismos textos como valores.

Para añadir un mensaje, usa `tr('Texto propio de MITO')` y añade la misma clave
a ambos bundles. Para valores dinámicos, usa argumentos con nombre:
`tr('Profile {name} creado', name=name)`. No construyas la clave con un f-string
ni traduzcas una descripción, una nota o un valor recibido de un catálogo.
Los parámetros se sustituyen después de buscar la traducción. Conserva exactamente
los nombres de los parámetros en todos los idiomas; su orden puede cambiar.

Para añadir un idioma:

1. Copia el bundle español y traduce sus valores, manteniendo todas las claves.
2. Añade el nombre finito del idioma a `LANGUAGES` en `i18n.py`. Configuración y
   empaquetado utilizan esa lista; no se aceptan nombres de archivo arbitrarios.
   Amplía también la correspondencia de LANGID en `system_language` si debe
   seleccionarse automáticamente para ese idioma de Windows.
3. Amplía las pruebas de bundles e interfaz en `core/tests/test_i18n.py` y verifica
   diálogos, avisos, acciones y mensajes de workers, además del espacio disponible.

`system` es la selección predeterminada, no un bundle: consulta el idioma de
interfaz del usuario de Windows. Las variantes regionales de los idiomas oficiales
de la UE usan su bundle; cualquier idioma desconocido o un fallo de detección usa
`english`. La selección manual tiene prioridad.

El gestor mantiene su idioma durante la sesión. Guardar otra selección requiere
volver a abrirlo. No cambia terminales o aplicaciones abiertas. No traduzcas
tokens internos, nombres de propiedades, IDs, rutas, scripts ni salidas externas.
Los mensajes de servicios usan el contexto de la UI cuando existe; la CLI y el
bootstrap conservan el texto fuente. Los diálogos nativos de autenticación sí
leen `mito.language` al abrirse desde un lanzador.

Las traducciones ausentes o con parámetros distintos buscan primero en inglés y,
si tampoco existen ahí, vuelven al texto fuente.
Los bundles son recursos del programa; no se cargan traducciones desde Scoop.
