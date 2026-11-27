//! Paquetes MSI extraídos como datos (`msi.py`): la base se lee con `msi.dll` en modo
//! lectura ([`mito_win::msi`]) y los CAB incrustados se descomprimen con el 7-Zip
//! privado. Nunca se ejecutan acciones de instalación.
//!
//! Contrato: `docs/spec/modules/msi.md`. Diferencia deliberada respecto al original
//! (duda 3 del contrato): todos los CAB se extraen y validan en el directorio de
//! trabajo **antes** de copiar nada a `destination`, de modo que un fallo en el CAB 2
//! no deja el destino a medias.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use mito_core::Message;
use mito_core::storage::{inside, portable_path};
use mito_win::msi::{Database, MsiError};

use super::{Limits, MAX_ENTRIES, MAX_EXPANDED, casefold, extract_7zip_with};
use crate::error::{Error, Result};

/// `msidbFileAttributesNoncompressed`: el archivo no va dentro de un CAB.
const NONCOMPRESSED: i64 = 0x2000;

/// Bloques de filas de una tabla: `columns` de `table` como texto.
type Rows<'a> = &'a dyn Fn(&str, &[&str]) -> Result<Vec<Vec<String>>>;

/// Extrae un CAB (`cabinet`) en `folder` con los `limits` dados.
type CabinetExtractor<'a> = &'a dyn Fn(&Path, &Path, &Limits) -> Result<()>;

fn fail(text: &str) -> Error {
    Error::untranslated(format!("MSI: {text}"))
}

impl From<MsiError> for Error {
    fn from(error: MsiError) -> Self {
        Self::untranslated(error)
    }
}

/// Un archivo de la tabla `File` ya validado.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Planned {
    /// Índice del CAB que lo contiene.
    cabinet: usize,
    /// Clave de `File`: nombre del archivo dentro del CAB.
    key: String,
    /// Ruta relativa de destino, con `/`.
    relative: String,
    /// Tamaño declarado.
    size: u64,
}

/// Resultado de [`plan`]: `(exclusivo, inclusivo, nombre en _Streams)` y archivos.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    cabinets: Vec<(i64, i64, String)>,
    files: Vec<Planned>,
}

/// Nombre local de un segmento (`leaf`): `destino:origen` y `corto|largo` solo en
/// directorios y archivos respectivamente. Con `directory`, `.` es el segmento vacío.
fn leaf(value: &str, directory: bool) -> Result<String> {
    let mut value = value;
    if directory {
        value = value.split_once(':').map_or(value, |(_, source)| source);
    }
    value = value.rsplit('|').next().unwrap_or_default();
    if directory && value == "." {
        return Ok(String::new());
    }
    let path = portable_path(value)?;
    if value.is_empty()
        || path.segments().len() != 1
        || matches!(value, "." | "..")
        || value.contains(['/', '\\'])
    {
        return Err(fail(&format!("invalid name {value}")));
    }
    Ok(value.to_owned())
}

/// `int()` de Python sobre un campo de tabla; el error es de dominio, no un pánico.
fn number(text: &str) -> Result<i64> {
    let trimmed = text.trim();
    trimmed
        .parse::<i64>()
        .map_err(|_| fail(&format!("invalid number {text}")))
}

/// Resuelve el árbol de `Directory` a rutas relativas a `TARGETDIR` (segmentos).
fn resolve_directories(rows: Rows<'_>) -> Result<HashMap<String, Vec<String>>> {
    let mut order = Vec::new();
    let mut directories: HashMap<String, (String, String)> = HashMap::new();
    for row in rows(
        "Directory",
        &["Directory", "Directory_Parent", "DefaultDir"],
    )? {
        let [key, parent, name] =
            <[String; 3]>::try_from(row).map_err(|_| fail("unexpected table shape"))?;
        if directories.insert(key.clone(), (parent, name)).is_none() {
            order.push(key);
        }
    }
    let mut resolved: HashMap<String, Vec<String>> = HashMap::new();
    for key in order {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = key;
        while !resolved.contains_key(&current) {
            let Some((parent, name)) = directories.get(&current) else {
                return Err(fail("cyclic or missing directory"));
            };
            if !seen.insert(current.clone()) {
                return Err(fail("cyclic or missing directory"));
            }
            if parent.is_empty() || *parent == current {
                if current != "TARGETDIR" {
                    return Err(fail("unsupported root directory"));
                }
                resolved.insert(current.clone(), Vec::new());
                break;
            }
            chain.push((current.clone(), leaf(name, true)?));
            current.clone_from(parent);
        }
        let mut path = resolved.get(&current).cloned().unwrap_or_default();
        for (directory, name) in chain.into_iter().rev() {
            if !name.is_empty() {
                path.push(name);
            }
            portable_path(&path.join("/"))?;
            resolved.insert(directory, path.clone());
        }
    }
    Ok(resolved)
}

/// Valida toda la base (directorios, componentes, medios, archivos, secuencias, tamaños
/// y colisiones) sin tocar el disco (`plan`).
fn plan(rows: Rows<'_>, max_expanded: u64) -> Result<Plan> {
    let resolved = resolve_directories(rows)?;
    let components: HashMap<String, String> = rows("Component", &["Component", "Directory_"])?
        .into_iter()
        .filter_map(|row| {
            let mut fields = row.into_iter();
            Some((fields.next()?, fields.next()?))
        })
        .collect();
    let mut media = Vec::new();
    for row in rows("Media", &["DiskId", "LastSequence", "Cabinet"])? {
        let [disk, last, cabinet] =
            <[String; 3]>::try_from(row).map_err(|_| fail("unexpected table shape"))?;
        media.push((number(&disk)?, number(&last)?, cabinet));
    }
    media.sort();
    let mut previous = 0;
    let mut cabinets = Vec::new();
    for (_, last, cabinet) in media {
        if last < previous || !cabinet.starts_with('#') || cabinet.chars().count() < 2 {
            return Err(fail(
                "only embedded cabinets are supported (no external media)",
            ));
        }
        cabinets.push((previous, last, cabinet[1..].to_owned()));
        previous = last;
    }
    let mut files = Vec::new();
    let (mut targets, mut ids, mut sequences) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut total = 0_u64;
    let columns = [
        "File",
        "Component_",
        "FileName",
        "FileSize",
        "Attributes",
        "Sequence",
    ];
    for row in rows("File", &columns)? {
        let [key, component, name, size, attributes, sequence] =
            <[String; 6]>::try_from(row).map_err(|_| fail("unexpected table shape"))?;
        let (size, sequence) = (number(&size)?, number(&sequence)?);
        let attributes = if attributes.is_empty() {
            0
        } else {
            number(&attributes)?
        };
        if size < 0 || sequence < 1 || attributes & NONCOMPRESSED != 0 {
            return Err(fail(
                "unsupported uncompressed file or invalid size/sequence",
            ));
        }
        leaf(&key, false)?;
        let Some(directory) = components.get(&component).and_then(|d| resolved.get(d)) else {
            return Err(fail("missing file component/directory"));
        };
        let mut segments = directory.clone();
        segments.push(leaf(&name, false)?);
        let relative = segments.join("/");
        portable_path(&relative)?;
        let folded = casefold(&relative);
        if !targets.insert(folded) || !ids.insert(casefold(&key)) || !sequences.insert(sequence) {
            return Err(fail("duplicate file, sequence or destination"));
        }
        // `size` es no negativo aquí.
        total = total.saturating_add(size.unsigned_abs());
        let Some(cabinet) = cabinets
            .iter()
            .position(|(low, high, _)| *low < sequence && sequence <= *high)
        else {
            return Err(fail("file has no embedded cabinet"));
        };
        files.push(Planned {
            cabinet,
            key,
            relative,
            size: size.unsigned_abs(),
        });
    }
    if files.is_empty() || total > max_expanded {
        return Err(fail("empty or oversized payload"));
    }
    for file in &files {
        let mut parent = file.relative.as_str();
        while let Some((upper, _)) = parent.rsplit_once('/') {
            if targets.contains(&casefold(upper)) {
                return Err(fail("file/directory collision"));
            }
            parent = upper;
        }
    }
    Ok(Plan { cabinets, files })
}

/// Cabecera de un CAB: `MSCF` y sin los bits `prev`/`next` (spanning).
fn check_cabinet(path: &Path) -> Result<()> {
    let mut header = Vec::with_capacity(36);
    fs::File::open(path)
        .and_then(|file| file.take(36).read_to_end(&mut header))
        .map_err(|e| mito_core::Error::io(path, e))?;
    let spanning = header
        .get(30..32)
        .is_none_or(|flags| u16::from_le_bytes([flags[0], flags[1]]) & 3 != 0);
    if header.len() != 36 || !header.starts_with(b"MSCF") || spanning {
        return Err(fail("invalid or spanning cabinet is unsupported"));
    }
    Ok(())
}

/// Archivos de primer nivel de `folder` por nombre en `casefold`; una subcarpeta (o un
/// nombre repetido) es un contenido distinto del esperado.
fn top_level(folder: &Path) -> Result<Option<HashMap<String, PathBuf>>> {
    let mut actual = HashMap::new();
    for entry in fs::read_dir(folder).map_err(|e| mito_core::Error::io(folder, e))? {
        let entry = entry.map_err(|e| mito_core::Error::io(folder, e))?;
        let kind = entry
            .file_type()
            .map_err(|e| mito_core::Error::io(entry.path(), e))?;
        if kind.is_dir() {
            return Ok(None);
        }
        if kind.is_file()
            && actual
                .insert(casefold(&entry.file_name().to_string_lossy()), entry.path())
                .is_some()
        {
            return Ok(None);
        }
    }
    Ok(Some(actual))
}

/// Extrae los archivos de `archive` en `destination` (`extract_msi`). `log` recibe el
/// progreso (`MSI: CAB {p1}/{p2}` y `Archivos MSI: {p2}/{p3}`).
///
/// # Errors
///
/// `MSI: …` (sin traducir, como el original), los de 7-Zip, límites y E/S.
pub fn extract_msi(
    archive: &Path,
    destination: &Path,
    sevenzip: Option<&Path>,
    log: &dyn Fn(Message),
) -> Result<()> {
    let database = Database::open(archive)?;
    let rows = |table: &str, columns: &[&str]| -> Result<Vec<Vec<String>>> {
        Ok(database.rows(table, columns, MAX_ENTRIES)?)
    };
    let stream = |name: &str, target: &Path| -> Result<()> {
        let mut output = fs::File::create(target).map_err(|e| mito_core::Error::io(target, e))?;
        database.read_stream(name, &mut output, MAX_EXPANDED)?;
        Ok(())
    };
    let extractor = |cabinet: &Path, folder: &Path, limits: &Limits| {
        extract_7zip_with(cabinet, folder, sevenzip, limits)
    };
    extract_with(&rows, &stream, &extractor, destination, MAX_EXPANDED, log)
}

/// Núcleo de [`extract_msi`] con la lectura de la base y el 7-Zip inyectados.
fn extract_with(
    rows: Rows<'_>,
    stream: &dyn Fn(&str, &Path) -> Result<()>,
    extract_cabinet: CabinetExtractor<'_>,
    destination: &Path,
    max_expanded: u64,
    log: &dyn Fn(Message),
) -> Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    let scratch = tempfile::tempdir_in(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    let plan = plan(rows, max_expanded)?;
    let mut expanded = Vec::with_capacity(plan.cabinets.len());
    for (index, (_, _, name)) in plan.cabinets.iter().enumerate() {
        log(Message::new("MSI: CAB {p1}/{p2}")
            .arg("p1", index + 1)
            .arg("p2", plan.cabinets.len()));
        let cabinet = scratch.path().join(format!("payload-{index}.cab"));
        stream(name, &cabinet)?;
        // Un CAB que continúa en otro disco podría pedir otro archivo del disco.
        check_cabinet(&cabinet)?;
        let expected: HashMap<String, &Planned> = plan
            .files
            .iter()
            .filter(|file| file.cabinet == index)
            .map(|file| (casefold(&file.key), file))
            .collect();
        let folder = scratch.path().join(format!("expanded-{index}"));
        let limits = Limits {
            max_entries: expected.len(),
            max_expanded: expected.values().map(|file| file.size).sum(),
        };
        extract_cabinet(&cabinet, &folder, &limits)?;
        let matches = top_level(&folder)?.filter(|actual| {
            actual.len() == expected.len() && expected.keys().all(|key| actual.contains_key(key))
        });
        let Some(actual) = matches else {
            return Err(fail("cabinet contents do not match File table"));
        };
        for (key, file) in &expected {
            let size = fs::metadata(&actual[key])
                .map_err(|e| mito_core::Error::io(&actual[key], e))?
                .len();
            if size != file.size {
                return Err(fail(&format!("file size mismatch {}", file.relative)));
            }
        }
        fs::remove_file(&cabinet).map_err(|e| mito_core::Error::io(&cabinet, e))?;
        expanded.push(actual);
    }
    // Todo validado: ahora se publica en `destination`.
    let mut completed = 0;
    for file in &plan.files {
        let source = &expanded[file.cabinet][&casefold(&file.key)];
        let target = inside(destination, &file.relative)?;
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder).map_err(|e| mito_core::Error::io(folder, e))?;
        }
        fs::copy(source, &target).map_err(|e| mito_core::Error::io(&target, e))?;
        completed += 1;
        if completed % 1000 == 0 || completed == plan.files.len() {
            log(Message::new("Archivos {p1}: {p2}/{p3}")
                .arg("p1", "MSI")
                .arg("p2", completed)
                .arg("p3", plan.files.len()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::process::Command;

    use mito_win::msi::DatabaseBuilder;

    use super::*;

    type Case<'a> = (&'a str, &'a [(&'a str, &'a [u8])], &'a str);
    type Tables = HashMap<&'static str, Vec<Vec<&'static str>>>;

    /// Tablas de `Tables` de `test_msi.py`.
    fn tables() -> Tables {
        HashMap::from([
            (
                "Directory",
                vec![
                    vec!["TARGETDIR", "", "SourceDir"],
                    vec!["APP", "TARGETDIR", "TARGET:SHORT|Package"],
                    vec!["BIN", "APP", "bin"],
                ],
            ),
            ("Component", vec![vec!["comp", "BIN"]]),
            ("Media", vec![vec!["1", "1", "#cab"]]),
            (
                "File",
                vec![vec![
                    "payload",
                    "comp",
                    "TOOL.EXE|tool.exe",
                    "7",
                    "16384",
                    "1",
                ]],
            ),
        ])
    }

    fn plan_of(tables: &Tables, limit: u64) -> Result<Plan> {
        let rows = |table: &str, _: &[&str]| -> Result<Vec<Vec<String>>> {
            Ok(tables[table]
                .iter()
                .map(|row| row.iter().map(|&c| c.to_owned()).collect())
                .collect())
        };
        plan(&rows, limit)
    }

    fn cell(
        tables: &mut Tables,
        table: &'static str,
        row: usize,
        column: usize,
        value: &'static str,
    ) {
        tables.get_mut(table).unwrap()[row][column] = value;
    }

    #[test]
    fn los_nombres_de_origen_y_los_medios_incrustados() {
        let plan = plan_of(&tables(), MAX_EXPANDED).unwrap();
        assert_eq!(plan.cabinets, [(0, 1, "cab".to_owned())]);
        assert_eq!(
            plan.files,
            [Planned {
                cabinet: 0,
                key: "payload".into(),
                relative: "Package/bin/tool.exe".into(),
                size: 7,
            }]
        );
    }

    #[test]
    fn leaf_aplica_las_reglas_de_origen() {
        assert_eq!(leaf("TARGET:SHORT|Package", true).unwrap(), "Package");
        assert_eq!(leaf("TOOL.EXE|tool.exe", false).unwrap(), "tool.exe");
        assert_eq!(leaf(".", true).unwrap(), "");
        assert_eq!(leaf("a|b|c", true).unwrap(), "c");
        for bad in ["", "..", ".", "a\\b", "a/b", "NUL.txt", "C:\\x"] {
            assert!(leaf(bad, false).is_err(), "{bad}");
        }
        // Con `directory`, `:` separa destino y origen: `x:..` queda `..`.
        assert!(leaf("x:..", true).is_err());
    }

    #[test]
    fn el_punto_es_la_misma_carpeta_y_targetdir_ignora_su_nombre() {
        let mut data = tables();
        cell(&mut data, "Directory", 0, 2, "no es SourceDir");
        cell(&mut data, "Directory", 2, 2, ".");
        let plan = plan_of(&data, MAX_EXPANDED).unwrap();
        assert_eq!(plan.files[0].relative, "Package/tool.exe");
    }

    #[test]
    fn rutas_referencias_duplicados_y_medios_invalidos() {
        // (tabla, fila, columna, valor), como test_invalid_paths_references_….
        let changes: [(&str, usize, usize, &str); 15] = [
            ("Directory", 1, 2, "../escape"),
            ("Directory", 1, 2, "C:\\escape"),
            ("Directory", 1, 1, "BIN"),
            ("Directory", 1, 1, "missing"),
            ("Directory", 1, 0, "OTHER"),
            ("File", 0, 2, "..\\escape.exe"),
            ("File", 0, 2, "aux.exe"),
            ("File", 0, 3, "-1"),
            ("File", 0, 3, "no es un número"),
            ("File", 0, 4, "8192"),
            ("File", 0, 5, "2"),
            ("Media", 0, 2, "external.cab"),
            ("Media", 0, 2, ""),
            ("Component", 0, 1, "NOPE"),
            ("File", 0, 1, "ghost"),
        ];
        for (table, row, column, value) in changes {
            let mut data = tables();
            cell(&mut data, table, row, column, value);
            let error = plan_of(&data, MAX_EXPANDED).unwrap_err().to_string();
            assert!(
                error.starts_with("MSI: ") || error.contains("Ruta"),
                "{value}: {error}"
            );
        }
        let mut data = tables();
        cell(&mut data, "File", 0, 5, "2");
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: file has no embedded cabinet"
        );
        let mut data = tables();
        cell(&mut data, "Media", 0, 2, "external.cab");
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: only embedded cabinets are supported (no external media)"
        );
        let mut data = tables();
        cell(&mut data, "Directory", 1, 1, "BIN");
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: cyclic or missing directory"
        );
        let mut data = tables();
        cell(&mut data, "Directory", 1, 1, "");
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: unsupported root directory"
        );
        let mut data = tables();
        cell(&mut data, "File", 0, 4, "8192");
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: unsupported uncompressed file or invalid size/sequence"
        );
        assert_eq!(
            plan_of(&tables(), 6).unwrap_err().to_string(),
            "MSI: empty or oversized payload"
        );
    }

    #[test]
    fn duplicados_y_colisiones() {
        let mut data = tables();
        data.get_mut("File").unwrap().push(vec![
            "other",
            "comp",
            "TOOL.EXE|TOOL.exe",
            "7",
            "16384",
            "1",
        ]);
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: duplicate file, sequence or destination"
        );
        let mut data = tables();
        data.get_mut("File")
            .unwrap()
            .push(vec!["PAYLOAD", "comp", "other.exe", "7", "16384", "2"]);
        data.get_mut("Media").unwrap()[0][1] = "2";
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: duplicate file, sequence or destination"
        );
        // Un archivo `bin` y otro dentro de la carpeta `bin`.
        let mut data = tables();
        data.get_mut("Directory")
            .unwrap()
            .push(vec!["SUB", "BIN", "tool.exe"]);
        data.get_mut("Component").unwrap().push(vec!["c2", "SUB"]);
        data.get_mut("File")
            .unwrap()
            .push(vec!["second", "c2", "child.exe", "1", "16384", "2"]);
        data.get_mut("Media").unwrap()[0][1] = "2";
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: file/directory collision"
        );
        // Sin archivos.
        let mut data = tables();
        data.get_mut("File").unwrap().clear();
        assert_eq!(
            plan_of(&data, MAX_EXPANDED).unwrap_err().to_string(),
            "MSI: empty or oversized payload"
        );
    }

    #[test]
    fn los_medios_se_ordenan_y_cubren_rangos() {
        let mut data = tables();
        data.get_mut("Media").unwrap().clear();
        data.get_mut("Media")
            .unwrap()
            .extend([vec!["2", "5", "#b"], vec!["1", "3", "#a"]]);
        data.get_mut("File")
            .unwrap()
            .push(vec!["k2", "comp", "t2.exe", "1", "16384", "4"]);
        let plan = plan_of(&data, MAX_EXPANDED).unwrap();
        assert_eq!(
            plan.cabinets,
            [(0, 3, "a".to_owned()), (3, 5, "b".to_owned())]
        );
        assert_eq!(
            plan.files.iter().map(|f| f.cabinet).collect::<Vec<_>>(),
            [0, 1]
        );
        // LastSequence decreciente.
        data.get_mut("Media").unwrap()[0][1] = "2";
        assert!(plan_of(&data, MAX_EXPANDED).is_err());
    }

    // ---- Extracción con bases reales (msi.dll) y 7-Zip inyectado o real ----

    /// CAB falso: solo la cabecera que valida `extract_msi` (el contenido lo inyecta el
    /// extractor de pruebas).
    fn fake_cabinet(flags: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 64];
        data[..4].copy_from_slice(b"MSCF");
        data[30..32].copy_from_slice(&flags.to_le_bytes());
        data
    }

    struct Fixture {
        path: PathBuf,
    }

    /// Crea una base MSI con las tablas del contrato y los `cabinets` en `_Streams`.
    fn build(
        folder: &Path,
        media_cabinet: &str,
        files: &[(&str, &str, &str, u64, u32)],
        cabinets: &[(&str, &Path)],
    ) -> Fixture {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let number = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = folder.join(format!("fixture-{number}.msi"));
        let builder = DatabaseBuilder::create(&path).unwrap();
        for sql in [
            "CREATE TABLE `Directory` (`Directory` CHAR(72) NOT NULL, `Directory_Parent` CHAR(72), `DefaultDir` CHAR(255) NOT NULL PRIMARY KEY `Directory`)",
            "CREATE TABLE `Component` (`Component` CHAR(72) NOT NULL, `Directory_` CHAR(72) NOT NULL PRIMARY KEY `Component`)",
            "CREATE TABLE `Media` (`DiskId` SHORT NOT NULL, `LastSequence` SHORT NOT NULL, `Cabinet` CHAR(255) PRIMARY KEY `DiskId`)",
            "CREATE TABLE `File` (`File` CHAR(72) NOT NULL, `Component_` CHAR(72) NOT NULL, `FileName` CHAR(255) NOT NULL, `FileSize` LONG NOT NULL, `Attributes` SHORT, `Sequence` SHORT NOT NULL PRIMARY KEY `File`)",
            "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('TARGETDIR', '', 'SourceDir')",
            "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('APP', 'TARGETDIR', 'TARGET:SHORT|Package')",
            "INSERT INTO `Directory` (`Directory`, `Directory_Parent`, `DefaultDir`) VALUES ('BIN', 'APP', 'bin')",
            "INSERT INTO `Component` (`Component`, `Directory_`) VALUES ('comp', 'BIN')",
            // Una acción personalizada que nunca debe ejecutarse.
            "CREATE TABLE `CustomAction` (`Action` CHAR(72) NOT NULL, `Type` SHORT NOT NULL, `Source` CHAR(72), `Target` CHAR(255) PRIMARY KEY `Action`)",
            "INSERT INTO `CustomAction` (`Action`, `Type`, `Source`, `Target`) VALUES ('MustNeverRun', 19, '', 'MITO must not execute MSI actions')",
        ] {
            builder.execute(sql, &[]).unwrap();
        }
        let last = files.iter().map(|f| f.4).max().unwrap_or(1);
        builder
            .execute(
                &format!("INSERT INTO `Media` (`DiskId`, `LastSequence`, `Cabinet`) VALUES (1, {last}, '{media_cabinet}')"),
                &[],
            )
            .unwrap();
        for (key, name, _, size, sequence) in files {
            builder
                .execute(
                    &format!("INSERT INTO `File` (`File`, `Component_`, `FileName`, `FileSize`, `Attributes`, `Sequence`) VALUES ('{key}', 'comp', '{name}', {size}, 16384, {sequence})"),
                    &[],
                )
                .unwrap();
        }
        for (name, source) in cabinets {
            builder.add_stream(name, source).unwrap();
        }
        builder.commit().unwrap();
        Fixture { path }
    }

    /// 7-Zip de pruebas: crea en `folder` los archivos `(nombre, contenido)`.
    fn writer<'a>(
        files: &'a [(&'a str, &'a [u8])],
        calls: &'a RefCell<Vec<Limits>>,
    ) -> impl Fn(&Path, &Path, &Limits) -> Result<()> + 'a {
        move |_, folder, limits| {
            calls.borrow_mut().push(*limits);
            fs::create_dir_all(folder).unwrap();
            for (name, content) in files {
                fs::write(folder.join(name), content).unwrap();
            }
            Ok(())
        }
    }

    fn run_fake(
        fixture: &Fixture,
        out: &Path,
        files: &[(&str, &[u8])],
    ) -> (Result<()>, Vec<String>, Vec<Limits>) {
        let database = Database::open(&fixture.path).unwrap();
        let rows = |t: &str, c: &[&str]| -> Result<Vec<Vec<String>>> {
            Ok(database.rows(t, c, MAX_ENTRIES)?)
        };
        let stream = |name: &str, target: &Path| -> Result<()> {
            let mut output = fs::File::create(target).unwrap();
            database.read_stream(name, &mut output, MAX_EXPANDED)?;
            Ok(())
        };
        let calls = RefCell::new(Vec::new());
        let logs = RefCell::new(Vec::new());
        let extractor = writer(files, &calls);
        let result = extract_with(
            &rows,
            &stream,
            &extractor,
            out,
            MAX_EXPANDED,
            &|m: Message| logs.borrow_mut().push(m.to_string()),
        );
        (result, logs.take(), calls.take())
    }

    #[test]
    fn extrae_el_cab_incrustado_y_publica_solo_tras_validar() {
        let folder = tempfile::tempdir().unwrap();
        let cab = folder.path().join("fixture.cab");
        fs::write(&cab, fake_cabinet(0)).unwrap();
        let fixture = build(
            folder.path(),
            "#cab",
            &[("payload", "TOOL.EXE|tool.exe", "", 7, 1)],
            &[("cab", &cab)],
        );
        let out = folder.path().join("work").join("out");
        let (result, logs, calls) = run_fake(&fixture, &out, &[("PAYLOAD", b"fixture")]);
        result.unwrap();
        assert_eq!(
            fs::read(out.join("Package/bin/tool.exe")).unwrap(),
            b"fixture"
        );
        assert_eq!(logs, ["MSI: CAB 1/1", "Archivos MSI: 1/1"]);
        // Los límites son los de la lista de archivos del CAB.
        assert_eq!(
            calls,
            [Limits {
                max_entries: 1,
                max_expanded: 7
            }]
        );
        // El directorio de trabajo se borra.
        let leftovers: Vec<_> = fs::read_dir(folder.path().join("work"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["out"]);
    }

    #[test]
    fn los_fallos_se_detectan_antes_de_publicar() {
        let folder = tempfile::tempdir().unwrap();
        let cab = folder.path().join("fixture.cab");
        let spanning = folder.path().join("spanning.cab");
        fs::write(&cab, fake_cabinet(0)).unwrap();
        fs::write(&spanning, fake_cabinet(1)).unwrap();
        let files = [("payload", "tool.exe", "", 7, 1)];
        let good = build(folder.path(), "#cab", &files, &[("cab", &cab)]);
        let cases: Vec<Case> = vec![
            (
                "tamaño",
                &[("payload", b"short")],
                "MSI: file size mismatch Package/bin/tool.exe",
            ),
            (
                "de más",
                &[("payload", b"fixture"), ("unknown", b"x")],
                "MSI: cabinet contents do not match File table",
            ),
            (
                "de menos",
                &[],
                "MSI: cabinet contents do not match File table",
            ),
            (
                "otro nombre",
                &[("tool.exe", b"fixture")],
                "MSI: cabinet contents do not match File table",
            ),
        ];
        for (name, content, expected) in cases {
            let out = folder.path().join(format!("out-{name}"));
            let (result, _, _) = run_fake(&good, &out, content);
            assert_eq!(result.unwrap_err().to_string(), expected, "{name}");
            assert!(!out.exists(), "{name}");
        }
        // Una subcarpeta dentro del CAB.
        let out = folder.path().join("out-dir");
        let database = Database::open(&good.path).unwrap();
        let rows = |t: &str, c: &[&str]| -> Result<Vec<Vec<String>>> {
            Ok(database.rows(t, c, MAX_ENTRIES)?)
        };
        let stream = |name: &str, target: &Path| -> Result<()> {
            let mut output = fs::File::create(target).unwrap();
            database.read_stream(name, &mut output, MAX_EXPANDED)?;
            Ok(())
        };
        let subfolder = |_: &Path, folder: &Path, _: &Limits| -> Result<()> {
            fs::create_dir_all(folder.join("sub")).unwrap();
            fs::write(folder.join("payload"), b"fixture").unwrap();
            Ok(())
        };
        let error =
            extract_with(&rows, &stream, &subfolder, &out, MAX_EXPANDED, &|_| {}).unwrap_err();
        assert_eq!(
            error.to_string(),
            "MSI: cabinet contents do not match File table"
        );
        assert!(!out.exists());
        // CAB con el bit de spanning.
        let bad = build(folder.path(), "#cab", &files, &[("cab", &spanning)]);
        let out = folder.path().join("out-spanning");
        let (result, _, calls) = run_fake(&bad, &out, &[("payload", b"fixture")]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "MSI: invalid or spanning cabinet is unsupported"
        );
        assert_eq!(calls, Vec::<Limits>::new());
        assert!(!out.exists());
        // Medio externo y flujo ausente.
        let external = build(folder.path(), "external.cab", &files, &[]);
        let out = folder.path().join("out-external");
        let (result, _, _) = run_fake(&external, &out, &[]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "MSI: only embedded cabinets are supported (no external media)"
        );
        let missing = build(folder.path(), "#cab", &files, &[]);
        let (result, _, _) = run_fake(&missing, &out, &[]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "MSI: missing embedded cabinet cab"
        );
        assert!(!out.exists());
    }

    #[test]
    fn un_archivo_que_no_es_msi_no_publica_nada() {
        let folder = tempfile::tempdir().unwrap();
        let archive = folder.path().join("tool.msi");
        fs::write(&archive, b"invalid MSI").unwrap();
        let out = folder.path().join("out");
        let error = extract_msi(&archive, &out, None, &|_| {}).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("MSI: Windows database error "),
            "{error}"
        );
        assert!(!out.exists());
    }

    fn real_7zip() -> Option<PathBuf> {
        [
            std::env::var_os("MITO_7ZIP").map(PathBuf::from),
            Some(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe")),
            Some(PathBuf::from(r"C:\Program Files (x86)\7-Zip\7z.exe")),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    }

    #[test]
    fn msi_real_con_makecab_y_7zip() {
        let Some(sevenzip) = real_7zip() else {
            eprintln!("se omite: no hay 7-Zip (define MITO_7ZIP)");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let payload = folder.path().join("payload");
        fs::write(&payload, b"fixture").unwrap();
        let cab = folder.path().join("fixture.cab");
        let made = Command::new(r"C:\Windows\System32\makecab.exe")
            .args(["/D", "CompressionType=MSZIP"])
            .arg(&payload)
            .arg(&cab)
            .output()
            .unwrap();
        assert!(made.status.success(), "makecab falló");
        let fixture = build(
            folder.path(),
            "#cab",
            &[("payload", "TOOL.EXE|tool.exe", "", 7, 1)],
            &[("cab", &cab)],
        );
        let out = folder.path().join("stage").join("out");
        let logs = RefCell::new(Vec::new());
        extract_msi(&fixture.path, &out, Some(&sevenzip), &|m| {
            logs.borrow_mut().push(m.to_string());
        })
        .unwrap();
        assert_eq!(
            fs::read(out.join("Package/bin/tool.exe")).unwrap(),
            b"fixture"
        );
        assert_eq!(logs.into_inner(), ["MSI: CAB 1/1", "Archivos MSI: 1/1"]);
        // Sin 7-Zip falla y no publica.
        let out = folder.path().join("stage").join("sin-7zip");
        let error = extract_msi(&fixture.path, &out, None, &|_| {}).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Falta el extractor privado 7-Zip; ejecuta bootstrap"
        );
        assert!(!out.exists());
    }
}
