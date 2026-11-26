# Desarrollo de MITO (Rust)

Port a Rust del [MITO original](https://github.com/danielgube/mito). Lee
[docs/PLAN.md](docs/PLAN.md), [docs/PARITY.md](docs/PARITY.md) y [docs/adr/](docs/adr/)
antes de cambiar contratos.

## Reglas de producto

- Windows x64. Una instancia por carpeta y un solo home.
- Compatibilidad con el original: mismos formatos de disco (`config.properties`,
  `profiles/<name>.json`, `.mito.json`, `core/state/*.json`, rutas de `core/apps`,
  `home/`, `workspaces/`, `pocketools/`) y misma CLI. Cualquier diferencia necesita ADR.
- La especificación de referencia es el código y los documentos del original
  (`spec/ARCHITECTURE.md`, `spec/USAGE.md`, `MANUAL.md`). Ante una duda de
  comportamiento, se consulta el original; si sigue sin estar claro, se pregunta.
- Sin excepciones por aplicación en el motor: solo capacidades genéricas del manifiesto.
  Un paso no soportado se marca como no soportado.
- Sin telemetría remota: solo `logs/mito.log`.
- Nunca escribir en Git datos del usuario, runtime, descargas ni payloads.

## Reglas de código

- Identificadores en inglés; documentación, comentarios y commits en español.
- `thiserror` en librerías, `anyhow` solo en el binario. Sin `unwrap`/`expect`/`panic`
  fuera de tests (lo impone clippy).
- `unsafe` solo en `mito-win`, con un comentario `// SAFETY:` por bloque. El resto de
  crates usan `#![forbid(unsafe_code)]`.
- Respetar el orden de capas de [ADR 0002](docs/adr/0002-workspace-y-capas.md).
- Toda API pública documentada (`missing_docs`).
- Tests junto al código (`#[cfg(test)]`) y de integración en `tests/`. Invariantes de
  rutas y extracción con proptest; salidas generadas (JSON, TUI) con snapshots de insta.

## Antes de dar por terminado un cambio

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Actualiza `docs/PARITY.md` y `CHANGELOG.md` cuando corresponda.

## Git

- Una rama `feat/fNN-<tema>` por fase y un PR a `main`; el merge lo hace el propietario.
- Conventional Commits en español (`feat(engine): …`, `fix(transfer): …`).
- No hacer push a `main` ni crear releases sin petición explícita.

## Pruebas E2E

Las pruebas con la app real se hacen en `C:\mito-rs` (la raíz de la instancia no puede
superar 32 caracteres sin rutas largas activadas).
