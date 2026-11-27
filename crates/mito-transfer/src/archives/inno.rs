//! Instaladores de Inno Setup extraídos como datos con el `innounp` privado (`inno.py`).
//!
//! Contrato: `docs/spec/modules/inno.md`. El instalador nunca se ejecuta.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::LazyLock;

use mito_core::Message;
use mito_core::storage::{inside, portable_path};
use regex::Regex;
use sha2::{Digest, Sha256};

use super::sevenzip::is_link;
use super::{MAX_ENTRIES, MAX_EXPANDED, casefold};
use crate::error::{Error, Result};

/// `CREATE_NO_WINDOW`: que `innounp` no abra una consola.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Opciones comunes de `innounp`: sin preguntas (`-b`), sin adivinar formatos más
/// nuevos (`-n`), salida no regional en UTF-8 (`-o`, `-h`, `-u`) y una sola copia de los
/// datos compartidos (`-a`).
const COMMON: [&str; 6] = ["-b", "-n", "-o", "-h", "-u", "-a"];

/// Raíz por defecto de los archivos de una aplicación (`APP`).
pub const APP: &str = "{app}";

/// Constante de Inno elegida por un `extract_dir` desnudo (`inno_root`): `{app}` (sin
/// distinguir mayúsculas) o `{code_Funcion}` (sí las distingue). Cualquier otra
/// (`{sys}`, `{tmp}`, rutas, `{code:X}`) no es una raíz soportada.
pub fn inno_root(extract_dir: &str) -> Option<&str> {
    if extract_dir.to_lowercase() == APP {
        return Some(extract_dir);
    }
    let name = extract_dir.strip_prefix("{code_")?.strip_suffix('}')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some(extract_dir)
}

/// Copias numeradas por Inno (`Editor,1.exe`, `Editor,2.exe`) de `name`: `(n, nombre)`.
fn numbered(name: &str, entry: &str) -> Option<u64> {
    let (stem, suffix) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let entry = entry.to_lowercase();
    let rest = entry.strip_prefix(&format!("{},", stem.to_lowercase()))?;
    let digits = rest.strip_suffix(&suffix.to_lowercase())?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn digest(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).map_err(|e| mito_core::Error::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .map_err(|e| mito_core::Error::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_vec())
}

/// Restaura los nombres declarados (`shortcuts`, `bin`) que Inno renombró con `,N`
/// cuando todas las copias numeradas son idénticas (`restore_declared_targets`). Las
/// copias no se borran.
///
/// # Errors
///
/// `Inno: ambiguous variants for <relativo>` si las copias difieren; rutas fuera del
/// payload y errores de E/S.
pub fn restore_declared_targets(payload: &Path, targets: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for relative in targets {
        if !seen.insert(relative.as_str()) {
            continue;
        }
        let target = inside(payload, relative)?;
        let Some(parent) = target.parent().filter(|p| p.is_dir()) else {
            continue;
        };
        if target.exists() {
            continue;
        }
        let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let mut copies = Vec::new();
        for entry in fs::read_dir(parent).map_err(|e| mito_core::Error::io(parent, e))? {
            let entry = entry.map_err(|e| mito_core::Error::io(parent, e))?;
            let path = entry.path();
            let Some(n) = entry.file_name().to_str().and_then(|e| numbered(name, e)) else {
                continue;
            };
            let kind = entry
                .file_type()
                .map_err(|e| mito_core::Error::io(&path, e))?;
            if kind.is_file() && !kind.is_symlink() {
                copies.push((n, path));
            }
        }
        copies.sort();
        let consecutive = copies
            .iter()
            .enumerate()
            .all(|(index, (n, _))| *n == index as u64 + 1);
        if copies.len() < 2 || !consecutive {
            continue;
        }
        let mut hashes = BTreeSet::new();
        for (_, path) in &copies {
            hashes.insert(digest(path)?);
        }
        if hashes.len() != 1 {
            return Err(Error::untranslated(format!(
                "Inno: ambiguous variants for {relative}"
            )));
        }
        fs::copy(&copies[0].1, &target).map_err(|e| mito_core::Error::io(&target, e))?;
        if let Ok(modified) = fs::metadata(&copies[0].1).and_then(|m| m.modified())
            && let Ok(file) = fs::File::options().write(true).open(&target)
        {
            let _ = file.set_modified(modified);
        }
    }
    Ok(())
}

/// Archivo de la raíz elegida del instalador, según el listado de `innounp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnoFile {
    /// Ruta relativa a la raíz, con `/` y con la capitalización del listado.
    pub relative: String,
    /// Tamaño declarado en bytes.
    pub size: u64,
}

/// Fila de la tabla `innounp -v`: tamaño, fecha y hora (16 caracteres rellenados a 19
/// más dos separadores: cinco espacios) y nombre, con sus espacios conservados.
static ROW: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^\s*(\d+)\s+\d{4}-\d{2}-\d{2} \d{2}:\d{2} {5}(.+)$").ok());

/// `str.splitlines()` de Python: además de `\n` y `\r\n`, separa por `\r`, `\v`, `\f`,
/// los separadores de archivo/grupo/registro, `NEL` y los de línea y párrafo Unicode.
fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let width = match ch {
            '\r' if chars.peek().is_some_and(|(_, next)| *next == '\n') => {
                chars.next();
                2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => ch.len_utf8(),
            _ => continue,
        };
        lines.push(&text[start..index]);
        start = index + width;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Interpreta la tabla de `innounp -v` (no regional, UTF-8) y devuelve los archivos bajo
/// `root` (`{app}` por defecto), fallando ante cualquier línea desconocida
/// (`app_inventory`). Las otras raíces (`{tmp}`, …) cuentan para los límites y los
/// duplicados, pero no se devuelven.
///
/// # Errors
///
/// `Inno: unrecognized file listing`, `Inno: non-canonical file path`,
/// `Inno: duplicate or overlapping file path`, `Inno: archive exceeds extraction limits`,
/// `Inno: no supported {root} payload or incomplete listing` y los de ruta de
/// [`portable_path`].
pub fn app_inventory(
    listing: &str,
    root: &str,
    max_entries: usize,
    max_expanded: u64,
) -> Result<Vec<InnoFile>> {
    let unrecognized = || Error::untranslated("Inno: unrecognized file listing");
    let row = ROW.as_ref().ok_or_else(unrecognized)?;
    let mut entries: Vec<InnoFile> = Vec::new();
    let mut known = HashSet::new();
    let (mut all_paths, mut directories) = (HashSet::new(), HashSet::new());
    let (mut started, mut ended) = (false, false);
    let (mut total, mut count) = (0_u64, 0_usize);
    for line in splitlines(listing) {
        if !started {
            started = line.starts_with("Size") && line.contains("Filename");
            continue;
        }
        if line.starts_with("---") {
            if count > 0 {
                ended = true;
                break;
            }
            continue;
        }
        let captures = row.captures(line).ok_or_else(unrecognized)?;
        let size = captures[1].parse::<u64>().unwrap_or(u64::MAX);
        let name = &captures[2];
        let path = portable_path(name)?;
        if path.is_empty() || path.segments().join("\\") != name.replace('/', "\\") {
            return Err(Error::untranslated("Inno: non-canonical file path"));
        }
        let folded = casefold(&path.segments().join("\\"));
        let parents: HashSet<String> = (1..path.segments().len())
            .map(|n| casefold(&path.segments()[..n].join("\\")))
            .collect();
        if all_paths.contains(&folded)
            || directories.contains(&folded)
            || parents.iter().any(|p| all_paths.contains(p))
        {
            return Err(Error::untranslated(
                "Inno: duplicate or overlapping file path",
            ));
        }
        all_paths.insert(folded);
        directories.extend(parents);
        count += 1;
        total = total.saturating_add(size);
        if count > max_entries || total > max_expanded {
            return Err(Error::untranslated(
                "Inno: archive exceeds extraction limits",
            ));
        }
        if path.segments().len() > 1 && casefold(&path.segments()[0]) == casefold(root) {
            let relative = path.segments()[1..].join("/");
            if known.insert(casefold(&relative)) {
                entries.push(InnoFile { relative, size });
            }
        }
    }
    if !started || !ended || entries.is_empty() {
        return Err(Error::untranslated(format!(
            "Inno: no supported {root} payload or incomplete listing"
        )));
    }
    Ok(entries)
}

/// Ejecuta `innounp.exe`. Existe para poder sustituirlo en los tests, como el original
/// parchea `subprocess.run`.
pub(crate) trait Runner {
    fn run(&self, program: &Path, args: &[OsString], cwd: &Path) -> io::Result<Output>;
}

struct System;

impl Runner for System {
    fn run(&self, program: &Path, args: &[OsString], cwd: &Path) -> io::Result<Output> {
        let mut command = Command::new(program);
        command.args(args).current_dir(cwd).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.output()
    }
}

/// Extrae los archivos bajo `constant` (`{app}` o `{code_X}`) del instalador `archive`
/// en `destination` con `innounp` (`extract_inno`). Primero valida el listado
/// (`innounp -v`) y solo publica en `destination` si lo extraído coincide con él. El
/// instalador nunca se ejecuta: solo se lanza `innounp`, sin entrada estándar.
///
/// El directorio de trabajo se crea junto a `archive` (la carpeta de staging), no en
/// `destination`.
///
/// # Errors
///
/// `Inno: …` (sin traducir, como el original), `Herramienta incompleta: innounp` si
/// `innounp` no es un archivo, límites y E/S.
pub fn extract_inno(
    archive: &Path,
    destination: &Path,
    innounp: &Path,
    constant: &str,
) -> Result<()> {
    extract_with(&System, archive, destination, innounp, constant)
}

fn tool_failed(output: &Output) -> Error {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
    let (stderr, stdout) = (text(&output.stderr), text(&output.stdout));
    let code = output
        .status
        .code()
        .map_or_else(|| "?".to_owned(), |c| c.to_string());
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Error::untranslated(format!("Inno: extraction tool failed ({code}): {detail}"))
}

pub(crate) fn extract_with(
    runner: &dyn Runner,
    archive: &Path,
    destination: &Path,
    executable: &Path,
    constant: &str,
) -> Result<()> {
    if inno_root(constant).is_none() {
        return Err(Error::untranslated(format!(
            "Inno: unsupported root {constant}"
        )));
    }
    if !executable.is_file() {
        return Err(Error::domain(
            Message::new("Herramienta incompleta: {p1}").arg("p1", "innounp"),
        ));
    }
    let archive = std::path::absolute(archive).unwrap_or_else(|_| archive.to_path_buf());
    let parent = archive.parent().unwrap_or_else(|| Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    let stage = stage.path();
    let run = |arguments: Vec<OsString>| -> Result<String> {
        let output = runner
            .run(executable, &arguments, stage)
            .map_err(|e| mito_core::Error::io(executable, e))?;
        if !output.status.success() {
            return Err(tool_failed(&output));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| Error::untranslated("Inno: extraction tool output is not UTF-8"))
    };
    let common = || COMMON.iter().map(OsString::from);
    let mut listing: Vec<OsString> = vec!["-v".into()];
    listing.extend(common());
    listing.push(archive.as_os_str().to_owned());
    let inventory = app_inventory(&run(listing)?, constant, MAX_ENTRIES, MAX_EXPANDED)?;
    let output = stage.join("files");
    fs::create_dir(&output).map_err(|e| mito_core::Error::io(&output, e))?;
    let mut extraction: Vec<OsString> = vec!["-x".into()];
    extraction.extend(common());
    extraction.push(format!("-c{constant}").into());
    let mut folder = OsString::from("-d");
    folder.push(&output);
    extraction.push(folder);
    extraction.push(archive.as_os_str().to_owned());
    extraction.push(format!("{constant}\\*").into());
    run(extraction)?;
    let actual = extracted_files(&output)?;
    let listed: HashMap<String, u64> = inventory
        .iter()
        .map(|f| (casefold(&f.relative), f.size))
        .collect();
    let found: HashMap<String, u64> = actual
        .iter()
        .map(|(key, (_, size))| (key.clone(), *size))
        .collect();
    if listed != found {
        return Err(Error::untranslated(
            "Inno: extracted inventory differs from validated listing",
        ));
    }
    fs::create_dir_all(destination).map_err(|e| mito_core::Error::io(destination, e))?;
    for file in &inventory {
        let target = inside(destination, &file.relative)?;
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder).map_err(|e| mito_core::Error::io(folder, e))?;
        }
        let source = &actual[&casefold(&file.relative)].0;
        fs::copy(source, &target).map_err(|e| mito_core::Error::io(&target, e))?;
        // `shutil.copy2` conserva la fecha de modificación.
        if let Ok(modified) = fs::metadata(source).and_then(|m| m.modified())
            && let Ok(copy) = fs::File::options().write(true).open(&target)
        {
            let _ = copy.set_modified(modified);
        }
    }
    Ok(())
}

/// Archivos de `output` (recursivo) por ruta relativa en `casefold`, con su tamaño.
/// Rechaza enlaces, junctions, rutas que no confinan y nombres repetidos.
fn extracted_files(output: &Path) -> Result<HashMap<String, (PathBuf, u64)>> {
    let mut actual = HashMap::new();
    let mut pending = vec![(output.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in fs::read_dir(&folder).map_err(|e| mito_core::Error::io(&folder, e))? {
            let entry = entry.map_err(|e| mito_core::Error::io(&folder, e))?;
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|e| mito_core::Error::io(&path, e))?;
            if is_link(&metadata) {
                return Err(Error::untranslated(
                    "Inno: unmanaged link in extracted payload",
                ));
            }
            let relative = format!("{prefix}{}", entry.file_name().to_string_lossy());
            inside(output, &relative)?;
            if metadata.is_dir() {
                pending.push((path, format!("{relative}/")));
            } else if metadata.is_file()
                && actual
                    .insert(casefold(&relative), (path, metadata.len()))
                    .is_some()
            {
                return Err(Error::untranslated("Inno: duplicate extracted file"));
            }
        }
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_raiz_solo_acepta_app_y_code() {
        assert_eq!(inno_root("{code_GetDestDir}"), Some("{code_GetDestDir}"));
        assert_eq!(inno_root("{app}"), Some("{app}"));
        assert_eq!(inno_root("{APP}"), Some("{APP}"));
        for other in [
            "{sys}",
            "{tmp}",
            "{code_x}\\sub",
            "sub",
            "{code:GetDestDir}",
            "{CODE_x}",
            "{code_}",
            "{code_1x}",
            "",
        ] {
            assert_eq!(inno_root(other), None, "{other}");
        }
    }

    #[test]
    fn restaura_el_nombre_si_las_copias_son_identicas() {
        let folder = tempfile::tempdir().unwrap();
        let payload = folder.path();
        fs::write(payload.join("Editor,1.exe"), b"same").unwrap();
        fs::write(payload.join("Editor,2.EXE"), b"same").unwrap();
        fs::create_dir(payload.join("bin")).unwrap();
        fs::write(payload.join("bin/Tool,1.exe"), b"a").unwrap();
        fs::write(payload.join("bin/Tool,2.exe"), b"b").unwrap();
        fs::write(payload.join("Solo,1.exe"), b"x").unwrap();
        fs::write(payload.join("Gap,1.exe"), b"x").unwrap();
        fs::write(payload.join("Gap,3.exe"), b"x").unwrap();
        restore_declared_targets(
            payload,
            &[
                "Editor.exe".into(),
                "Editor.exe".into(),
                "Solo.exe".into(),
                "Gap.exe".into(),
                "missing/X.exe".into(),
            ],
        )
        .unwrap();
        assert_eq!(fs::read(payload.join("Editor.exe")).unwrap(), b"same");
        assert!(payload.join("Editor,1.exe").exists());
        assert!(!payload.join("Solo.exe").exists());
        assert!(!payload.join("Gap.exe").exists());
        let error = restore_declared_targets(payload, &["bin/Tool.exe".into()]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Inno: ambiguous variants for bin/Tool.exe"
        );
        assert!(restore_declared_targets(payload, &["../fuera.exe".into()]).is_err());
    }

    // ---- app_inventory ----

    /// Tabla de `innounp -v` con las filas `(tamaño, nombre)`.
    fn table(rows: &[(u64, &str)]) -> String {
        let mut text = String::from(
            "; Version detected: 5500 (Unicode)\nSize       Date/Time        Filename\n---------------------------------\n",
        );
        for (size, name) in rows {
            writeln!(text, "{size:>10} 2024-01-02 10:11     {name}").unwrap();
        }
        text.push_str("---------------------------------\nTotal\n");
        text
    }

    fn inventory(rows: &[(u64, &str)]) -> Result<Vec<InnoFile>> {
        app_inventory(&table(rows), APP, MAX_ENTRIES, MAX_EXPANDED)
    }

    #[test]
    fn el_inventario_normaliza_app_y_omite_otras_raices() {
        let files = inventory(&[
            (3, "{app}\\Sub\\Editor.exe"),
            (9, "{tmp}\\helper.exe"),
            (5, "{APP}\\Con espacios  .txt"),
        ])
        .unwrap();
        assert_eq!(
            files,
            [
                InnoFile {
                    relative: "Sub/Editor.exe".into(),
                    size: 3
                },
                InnoFile {
                    relative: "Con espacios  .txt".into(),
                    size: 5
                },
            ]
        );
    }

    #[test]
    fn una_constante_con_codigo_elige_su_raiz() {
        let listing = table(&[(1, "{app}\\A.exe"), (2, "{code_GetDestDir}\\Editor.exe")]);
        let files =
            app_inventory(&listing, "{code_GetDestDir}", MAX_ENTRIES, MAX_EXPANDED).unwrap();
        assert_eq!(
            files,
            [InnoFile {
                relative: "Editor.exe".into(),
                size: 2
            }]
        );
    }

    #[test]
    fn rechaza_rutas_inseguras_o_ambiguas() {
        let cases: [&[(u64, &str)]; 8] = [
            &[(1, "{app}\\..\\escape")],
            &[(1, "C:\\escape")],
            &[(1, "{app}\\NUL.txt")],
            &[(1, "{app}\\a"), (1, "{app}\\A")],
            &[(1, "{app}\\file"), (1, "{app}\\file\\child")],
            &[(1, "{app}\\file\\child"), (1, "{app}\\file")],
            &[(1, "{app}\\a\\\\b")],
            &[(1, "{app}/a//b")],
        ];
        for rows in cases {
            assert!(inventory(rows).is_err(), "{rows:?}");
        }
        assert_eq!(
            inventory(&[(1, "{app}\\a"), (1, "{app}\\A")])
                .unwrap_err()
                .to_string(),
            "Inno: duplicate or overlapping file path"
        );
        assert_eq!(
            inventory(&[(1, "{app}/a//b")]).unwrap_err().to_string(),
            "Inno: non-canonical file path"
        );
        // Las barras normales canónicas se aceptan.
        assert_eq!(
            inventory(&[(1, "{app}/a/b.txt")]).unwrap()[0].relative,
            "a/b.txt"
        );
    }

    #[test]
    fn rechaza_limites_cargas_ausentes_y_tablas_incompletas() {
        let listing = table(&[(1, "{app}\\a"), (2, "{app}\\b")]);
        let limits = |entries, expanded| app_inventory(&listing, APP, entries, expanded);
        assert_eq!(
            limits(MAX_ENTRIES, 2).unwrap_err().to_string(),
            "Inno: archive exceeds extraction limits"
        );
        assert_eq!(
            limits(0, MAX_EXPANDED).unwrap_err().to_string(),
            "Inno: archive exceeds extraction limits"
        );
        assert_eq!(limits(2, 3).unwrap().len(), 2);
        assert_eq!(
            inventory(&[(1, "{tmp}\\a")]).unwrap_err().to_string(),
            "Inno: no supported {app} payload or incomplete listing"
        );
        let unterminated = "Size  Date  Filename\n---\n         1 2024-01-02 10:11     {app}\\a\n";
        for text in [unterminated, "", "nada que ver\n", "Size Filename\n"] {
            assert_eq!(
                app_inventory(text, APP, MAX_ENTRIES, MAX_EXPANDED)
                    .unwrap_err()
                    .to_string(),
                "Inno: no supported {app} payload or incomplete listing",
                "{text}"
            );
        }
        // Una línea desconocida o en blanco tras la cabecera aborta.
        for line in [
            "",
            "basura",
            "  12 2024-01-02 10:11 {app}\\a",
            "x 2024-01-02 10:11     a",
        ] {
            let text = format!("Size Filename\n---\n{line}\n");
            assert_eq!(
                app_inventory(&text, APP, MAX_ENTRIES, MAX_EXPANDED)
                    .unwrap_err()
                    .to_string(),
                "Inno: unrecognized file listing",
                "{line}"
            );
        }
    }

    #[test]
    fn splitlines_separa_como_python() {
        assert_eq!(splitlines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n"), ["a"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert_eq!(splitlines("a\u{2028}b\x0cc"), ["a", "b", "c"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
    }

    // ---- extract_inno ----

    use std::cell::{Cell, RefCell};
    use std::fmt::Write as _;

    fn output(code: i32, stdout: &str, stderr: &str) -> Output {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt as _;
            std::process::ExitStatus::from_raw(code.cast_unsigned())
        };
        Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// `innounp` simulado: lista `listing` y, al extraer, escribe `files` en `-d`.
    struct Fake {
        listing: String,
        files: Vec<(&'static str, &'static [u8])>,
        destination: PathBuf,
        calls: RefCell<Vec<Vec<String>>>,
        cwds: RefCell<Vec<PathBuf>>,
        published_early: Cell<bool>,
        fail_with: Option<(i32, &'static str, &'static str)>,
    }

    impl Fake {
        fn new(rows: &[(u64, &str)], files: &[(&'static str, &'static [u8])], dir: &Path) -> Self {
            Self {
                listing: table(rows),
                files: files.to_vec(),
                destination: dir.join("payload"),
                calls: RefCell::default(),
                cwds: RefCell::default(),
                published_early: Cell::new(false),
                fail_with: None,
            }
        }
    }

    impl Runner for Fake {
        fn run(&self, _: &Path, args: &[OsString], cwd: &Path) -> io::Result<Output> {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            self.cwds.borrow_mut().push(cwd.to_path_buf());
            self.calls.borrow_mut().push(args.clone());
            if let Some((code, stdout, stderr)) = self.fail_with {
                return Ok(output(code, stdout, stderr));
            }
            if args[0] == "-v" {
                self.published_early.set(self.destination.exists());
                return Ok(output(0, &self.listing, ""));
            }
            let folder = args.iter().find_map(|a| a.strip_prefix("-d")).unwrap();
            for (name, content) in &self.files {
                let path = Path::new(folder).join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, content).unwrap();
            }
            Ok(output(0, "", ""))
        }
    }

    fn setup(dir: &Path) -> PathBuf {
        let tool = dir.join("innounp.exe");
        fs::write(&tool, b"tool").unwrap();
        fs::write(dir.join("setup.exe"), b"installer").unwrap();
        tool
    }

    #[test]
    fn solo_corre_la_herramienta_y_el_listado_precede_a_la_publicacion() {
        let folder = tempfile::tempdir().unwrap();
        let tool = setup(folder.path());
        let archive = folder.path().join("setup.exe");
        let fake = Fake::new(
            &[
                (3, "{app}\\Sub\\Editor.exe"),
                (9, "{tmp}\\helper.exe"),
                (1, "{app}\\a.txt"),
            ],
            &[("Sub/Editor.exe", b"app"), ("a.txt", b"1")],
            folder.path(),
        );
        extract_with(&fake, &archive, &folder.path().join("payload"), &tool, APP).unwrap();
        assert!(!fake.published_early.get());
        let calls = fake.calls.take();
        let archive_text = archive.to_string_lossy().into_owned();
        assert_eq!(
            calls[0],
            ["-v", "-b", "-n", "-o", "-h", "-u", "-a", &archive_text]
        );
        assert_eq!(calls.len(), 2);
        let output = calls[1][8].strip_prefix("-d").unwrap().to_owned();
        assert!(output.ends_with("files"), "{output}");
        assert_eq!(
            &calls[1][..8],
            ["-x", "-b", "-n", "-o", "-h", "-u", "-a", "-c{app}"]
        );
        assert_eq!(calls[1][9], archive_text);
        assert_eq!(calls[1][10], "{app}\\*");
        // `cwd` es el directorio de trabajo, junto al instalador, y ya no existe.
        let cwds = fake.cwds.take();
        assert_eq!(cwds[0], cwds[1]);
        assert_eq!(cwds[0].parent().unwrap(), folder.path());
        assert!(!cwds[0].exists());
        let payload = folder.path().join("payload");
        assert_eq!(fs::read(payload.join("Sub/Editor.exe")).unwrap(), b"app");
        assert_eq!(fs::read(payload.join("a.txt")).unwrap(), b"1");
        assert!(!payload.join("helper.exe").exists());
    }

    #[test]
    fn una_constante_con_codigo_se_pasa_a_innounp() {
        let folder = tempfile::tempdir().unwrap();
        let tool = setup(folder.path());
        let fake = Fake::new(
            &[(2, "{code_GetDestDir}\\Editor.exe")],
            &[("Editor.exe", b"ed")],
            folder.path(),
        );
        extract_with(
            &fake,
            &folder.path().join("setup.exe"),
            &folder.path().join("payload"),
            &tool,
            "{code_GetDestDir}",
        )
        .unwrap();
        let calls = fake.calls.take();
        assert_eq!(calls[1][7], "-c{code_GetDestDir}");
        assert_eq!(calls[1][10], "{code_GetDestDir}\\*");
        assert!(folder.path().join("payload/Editor.exe").is_file());
    }

    #[test]
    fn un_inventario_distinto_del_listado_no_publica() {
        let folder = tempfile::tempdir().unwrap();
        let tool = setup(folder.path());
        for (name, files) in [
            ("tamaño", vec![("a.txt", b"wrong size" as &[u8])]),
            ("de más", vec![("a.txt", b"1"), ("extra.txt", b"x")]),
            ("de menos", vec![]),
        ] {
            let fake = Fake::new(&[(1, "{app}\\a.txt")], &files, folder.path());
            let error = extract_with(
                &fake,
                &folder.path().join("setup.exe"),
                &folder.path().join("payload"),
                &tool,
                APP,
            )
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                "Inno: extracted inventory differs from validated listing",
                "{name}"
            );
            assert!(!folder.path().join("payload").exists(), "{name}");
        }
    }

    #[test]
    fn errores_de_la_herramienta_y_de_la_raiz() {
        let folder = tempfile::tempdir().unwrap();
        let tool = setup(folder.path());
        let archive = folder.path().join("setup.exe");
        let payload = folder.path().join("payload");
        // Sin herramienta: nunca se lanza ningún proceso.
        let fake = Fake::new(&[(1, "{app}\\a.txt")], &[("a.txt", b"1")], folder.path());
        let error = extract_with(
            &fake,
            &archive,
            &payload,
            &folder.path().join("no.exe"),
            APP,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Herramienta incompleta: innounp");
        assert_eq!(fake.calls.take(), Vec::<Vec<String>>::new());
        // Raíz no soportada, antes de comprobar la herramienta.
        let error = extract_with(
            &fake,
            &archive,
            &payload,
            &folder.path().join("no.exe"),
            "{sys}",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Inno: unsupported root {sys}");
        // Código de salida distinto de 0: stderr, o stdout si está vacío.
        let mut failing = Fake::new(&[], &[], folder.path());
        failing.fail_with = Some((2, "salida", " fallo \n"));
        let error = extract_with(&failing, &archive, &payload, &tool, APP).unwrap_err();
        assert_eq!(error.to_string(), "Inno: extraction tool failed (2): fallo");
        failing.fail_with = Some((3, " solo stdout ", ""));
        let error = extract_with(&failing, &archive, &payload, &tool, APP).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Inno: extraction tool failed (3): solo stdout"
        );
        // Un listado sin carga `{app}` no llega a extraer.
        let other = Fake::new(&[(1, "{tmp}\\a.txt")], &[], folder.path());
        let error = extract_with(&other, &archive, &payload, &tool, APP).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Inno: no supported {app} payload or incomplete listing"
        );
        assert_eq!(other.calls.take().len(), 1);
        assert!(!payload.exists());
    }

    /// Como [`Fake`], pero además deja un enlace (o junction) hacia fuera en `-d`.
    struct Linking(PathBuf, Fake);

    impl Runner for Linking {
        fn run(&self, program: &Path, args: &[OsString], cwd: &Path) -> io::Result<Output> {
            let result = self.1.run(program, args, cwd)?;
            if let Some(folder) = args
                .iter()
                .find_map(|a| a.to_string_lossy().strip_prefix("-d").map(PathBuf::from))
            {
                let link = folder.join("link");
                if std::os::windows::fs::symlink_dir(&self.0, &link).is_err() {
                    let status = Command::new("cmd")
                        .args(["/C", "mklink", "/J"])
                        .arg(&link)
                        .arg(&self.0)
                        .output()?;
                    assert!(status.status.success());
                }
            }
            Ok(result)
        }
    }

    #[test]
    fn un_enlace_en_lo_extraido_se_rechaza() {
        let folder = tempfile::tempdir().unwrap();
        let tool = setup(folder.path());
        let outside = folder.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let runner = Linking(
            outside,
            Fake::new(&[(1, "{app}\\a.txt")], &[("a.txt", b"1")], folder.path()),
        );
        let error = extract_with(
            &runner,
            &folder.path().join("setup.exe"),
            &folder.path().join("payload"),
            &tool,
            APP,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Inno: unmanaged link in extracted payload"
        );
        assert!(!folder.path().join("payload").exists());
    }

    /// `innounp` falso como script `.cmd`, para probar el lanzamiento real del proceso.
    #[test]
    fn el_proceso_real_se_lanza_sin_entrada_y_en_el_directorio_de_trabajo() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("setup.exe"), b"installer").unwrap();
        let listing = table(&[(5, "{app}\\Sub\\Editor.exe")]);
        fs::write(folder.path().join("listing.txt"), listing).unwrap();
        // `-d<carpeta>` es el noveno argumento; `exit /b` propaga el código.
        let script = "@echo off\r\n\
            if \"%~1\"==\"-v\" (type \"%~dp0listing.txt\" & exit /b 0)\r\n\
            set \"target=%~9\"\r\n\
            set \"target=%target:~2%\"\r\n\
            mkdir \"%target%\\Sub\"\r\n\
            echo abc> \"%target%\\Sub\\Editor.exe\"\r\n\
            exit /b 0\r\n";
        let tool = folder.path().join("innounp.cmd");
        fs::write(&tool, script).unwrap();
        extract_inno(
            &folder.path().join("setup.exe"),
            &folder.path().join("payload"),
            &tool,
            APP,
        )
        .unwrap();
        // `echo abc>` escribe 5 bytes (`abc` y CRLF), el tamaño del listado.
        assert_eq!(
            fs::read(folder.path().join("payload/Sub/Editor.exe")).unwrap(),
            b"abc\r\n"
        );
    }
}
