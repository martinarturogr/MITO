# Contratos de módulo

Cada archivo `<módulo>.md` describe el contrato de un módulo del original antes de
portarlo. Lo redacta un modelo ligero leyendo el `.py` y su `test_*.py`, y lo revisa
quien define la API Rust. Es la única fuente que lee quien implementa, para no releer
el Python.

## Plantilla

```markdown
# <módulo>.py → <crate>::<módulo>

Origen: `core/app/mito/<módulo>.py` (N líneas), `core/tests/test_<módulo>.py` (N tests).

## Responsabilidad
Una o dos frases.

## API pública del original
| Función/clase | Entradas | Salida | Errores |

## Formatos de disco
Archivos que lee o escribe, con un ejemplo real.

## Invariantes
Lista numerada de reglas que siempre deben cumplirse.

## Casos límite y errores
Lista.

## Casos de test del original
| Test | Qué comprueba |

## Dependencias
Otros módulos del original que usa.

## Notas para Rust
Crates recomendadas, diferencias inevitables, dudas abiertas.
