# Icono MITO

Fuente de concepto: render aportado directamente por el usuario el 27 de
septiembre de 2026 (PNG de 1024 × 1016 sobre fondo claro). Sustituye al
hexágono generado por IA del día anterior, y antes a la cotorra argentina de
la etapa YITO, conservada en `mito-original.ico` y en la historia git.

Dirección visual: insignia octogonal de neón con borde verde esmeralda-lima,
interior verde muy oscuro con trazas de circuito impreso cian-azuladas
rematadas en nodos redondos y un chevron de terminal «>» naranja cálido en
relieve tipo tubo, centrado. Sin texto ni marco exterior.

Proceso: eliminación del fondo claro por flood fill desde el marco sobre la
máscara «claro y desaturado», erosión de 2 px para cortar la franja
anti-alias, recorte al contenido y escalado del contenido sólido hasta dejar
2 px de margen: el octágono aprovecha prácticamente el 100 % del lienzo
(968 × 1020 px de 1024). Transparencia real comprobada sobre fondo claro y
oscuro.

`mito.ico` contiene PNG RGBA de 16, 20, 24, 32, 40, 48, 64, 128 y 256
píxeles; `mito_icon_preview.png` es la vista previa de 256. El chevron naranja
se reconoce hasta 20–24 px; a 16 px queda una insignia verde con centro
luminoso.

Toda la derivación se reproduce con `build_branding.py` (este directorio):
sobre el maestro MITO no se aplica ningún desplazamiento de color (su paleta
es final), sólo limpieza, escalado y emisión de ICO, preview y variantes
aplanadas sobre degradado verde oscuro: `mito_ico.jpg` (1024²),
`mito_logo.jpg` (1024², con wordmark MITO y «My Integrated Terminal
Orchestrator»), `mito_logo.png` (banner README de 1407 × 768) y
`mito_shortcut.jpg` (1408 × 768, sin texto).

Actualización 28/09/2026: los ICO se encuadran ahora sin margen en el eje mayor
mediante build_branding.py --icons-only, conservando proporciones y maestro.
El eje horizontal conserva el espacio derivado de la forma original.
