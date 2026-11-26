# Icono Pocketools

Diseño del 28 de septiembre de 2026: tuerca-engranaje naranja grande de ocho
 dientes, contorno verde oscuro y un pequeño «>» azul eléctrico en el centro.
Exterior transparente, sin el octágono ni los circuitos de la propuesta anterior.

Maestro creado con image_gen integrado, usando el Pocketools anterior como
referencia. Prompt: conservar la tuerca naranja, ampliarla a todo el cuadrado,
eliminar la insignia posterior y colocar un pequeño «>» azul eléctrico centrado.

`pocketools_icon.png` conserva el resultado original. `build_branding.py --icons-only`
recorta por alpha > 48, mantiene proporciones y encuadra sin margen en el eje mayor.
Emite `pocketools.ico` con PNG RGBA de 16, 20, 24, 32, 40, 48, 64, 128 y 256 px,
y `pocketools_icon_preview.png`. Este modo sólo necesita Pillow.

Pendiente de validación visual del usuario en Windows y en accesos con etiqueta.
