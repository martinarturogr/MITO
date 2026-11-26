//! Extracción de 7z y RAR (y ZIP con métodos no soportados) delegada en el `7z.exe`
//! privado (`extract_7zip` de `archives.py`).
//!
//! El listado `7z l -slt` se valida por completo **antes** de ejecutar la extracción;
//! después se comprueba que no haya aparecido ningún enlace.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

use mito_core::Message;
use mito_core::storage::inside;

use super::Limits;
use crate::error::{Error, Result};

/// `CREATE_NO_WINDOW`: que 7-Zip no abra una consola.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `FILE_ATTRIBUTE_REPARSE_POINT`: enlaces simbólicos y junctions.
#[cfg(windows)]
const REPARSE_POINT: u32 = 0x400;

/// Ejecuta `7z.exe`. Existe para poder sustituirlo en los tests, como el original
/// parchea `subprocess.run`.
pub(crate) trait Runner {
    fn run(&self, program: &Path, args: &[OsString]) -> io::Result<Output>;
}

struct System;

impl Runner for System {
    fn run(&self, program: &Path, args: &[OsString]) -> io::Result<Output> {
        let mut command = Command::new(program);
        command.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.output()
    }
}

/// Extrae `archive` en `destination` con el 7-Zip de `sevenzip` y los límites por defecto.
///
/// # Errors
///
/// Los de [`extract_7zip_with`].
pub fn extract_7zip(archive: &Path, destination: &Path, sevenzip: Option<&Path>) -> Result<()> {
    extract_7zip_with(archive, destination, sevenzip, &Limits::default())
}

/// Extrae `archive` en `destination` con el 7-Zip de `sevenzip` (normalmente
/// `core/tools/7zip/7z.exe`).
///
/// Valida el listado (rutas con `inside`, duplicados, enlaces, cifrado, número de
/// entradas y tamaño) antes de extraer, y tras extraer rechaza cualquier enlace,
/// junction o ruta que escape de `destination`. Diferencia respecto al original: un
/// fallo de 7-Zip (código de salida distinto de 0) es un error de dominio y no una
/// excepción sin tratar.
///
/// # Errors
///
/// `Falta el extractor privado 7-Zip; ejecuta bootstrap` si `sevenzip` es `None` o no
/// es un archivo; `Listado 7z no reconocido`; `7z con enlaces o entradas duplicadas`;
/// `No se admiten archivos cifrados`; `7z demasiado grande`; `Enlace inesperado tras
/// extraer 7z`; y los de ruta de `inside`.
pub fn extract_7zip_with(
    archive: &Path,
    destination: &Path,
    sevenzip: Option<&Path>,
    limits: &Limits,
) -> Result<()> {
    let Some(executable) = sevenzip.filter(|p| p.is_file()) else {
        return Err(missing());
    };
    extract_with(&System, executable, archive, destination, limits)
}

fn missing() -> Error {
    Error::domain(Message::new(
        "Falta el extractor privado 7-Zip; ejecuta bootstrap",
    ))
}

fn failed(output: &Output) -> Error {
    let code = output
        .status
        .code()
        .map_or_else(|| "?".to_owned(), |c| c.to_string());
    Error::domain(Message::new("7-Zip terminó con error (código {p1})").arg("p1", code))
}

pub(crate) fn extract_with(
    runner: &dyn Runner,
    executable: &Path,
    archive: &Path,
    destination: &Path,
    limits: &Limits,
) -> Result<()> {
    let listing = runner
        .run(
            executable,
            &[
                "l".into(),
                "-slt".into(),
                "-ba".into(),
                "-sccUTF-8".into(),
                archive.as_os_str().to_owned(),
            ],
        )
        .map_err(|e| mito_core::Error::io(executable, e))?;
    if !listing.status.success() {
        return Err(failed(&listing));
    }
    validate(
        &String::from_utf8_lossy(&listing.stdout),
        destination,
        limits,
    )?;
    fs::create_dir_all(destination).map_err(|e| mito_core::Error::io(destination, e))?;
    let mut output_arg = OsString::from("-o");
    output_arg.push(destination);
    let extraction = runner
        .run(
            executable,
            &[
                "x".into(),
                archive.as_os_str().to_owned(),
                output_arg,
                "-y".into(),
                "-bd".into(),
            ],
        )
        .map_err(|e| mito_core::Error::io(executable, e))?;
    if !extraction.status.success() {
        return Err(failed(&extraction));
    }
    check_no_links(destination)
}

/// Registros `clave = valor` separados por líneas vacías.
fn parse(stdout: &str) -> Vec<HashMap<&str, &str>> {
    let mut records = Vec::new();
    let mut record = HashMap::new();
    for line in stdout.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
        } else if let Some((key, value)) = line.split_once(" = ") {
            record.insert(key, value);
        }
    }
    records
}

fn validate(stdout: &str, destination: &Path, limits: &Limits) -> Result<()> {
    let records = parse(stdout);
    let unrecognised = || Error::domain(Message::new("Listado 7z no reconocido"));
    let mut total = 0_u64;
    let mut seen = HashSet::new();
    for entry in &records {
        let path = entry.get("Path").ok_or_else(unrecognised)?;
        let target = inside(destination, path)?;
        let key = target.to_string_lossy().to_lowercase();
        let linked = ["Symbolic Link", "Hard Link"]
            .iter()
            .any(|k| entry.get(k).is_some_and(|v| !matches!(*v, "" | "-")));
        // El original busca `l` en cualquier posición de `Attributes`.
        let attributes_link = entry.get("Attributes").is_some_and(|a| a.contains('l'));
        if seen.contains(&key) || linked || attributes_link {
            return Err(Error::domain(Message::new(
                "7z con enlaces o entradas duplicadas",
            )));
        }
        if entry.get("Encrypted") == Some(&"+") {
            return Err(Error::domain(Message::new(
                "No se admiten archivos cifrados",
            )));
        }
        seen.insert(key);
        let size = match entry.get("Size").map(|s| s.trim()) {
            None | Some("") => 0,
            Some(text) => text.parse::<u64>().map_err(|_| unrecognised())?,
        };
        total = total.saturating_add(size);
    }
    if records.len() > limits.max_entries || total > limits.max_expanded {
        return Err(Error::domain(Message::new("7z demasiado grande")));
    }
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Tras extraer: ninguna ruta puede ser un enlace, una junction ni resolver fuera de
/// `destination`.
fn check_no_links(destination: &Path) -> Result<()> {
    let unexpected = || Error::domain(Message::new("Enlace inesperado tras extraer 7z"));
    let root = fs::canonicalize(destination).map_err(|e| mito_core::Error::io(destination, e))?;
    let mut pending = vec![destination.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let entries = fs::read_dir(&folder).map_err(|e| mito_core::Error::io(&folder, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| mito_core::Error::io(&folder, e))?;
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|e| mito_core::Error::io(&path, e))?;
            if is_link(&metadata) {
                return Err(unexpected());
            }
            let resolved = fs::canonicalize(&path).map_err(|e| mito_core::Error::io(&path, e))?;
            if !resolved.starts_with(&root) {
                return Err(unexpected());
            }
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// 7-Zip simulado: devuelve el listado indicado y registra las invocaciones.
    struct Fake {
        listing: String,
        calls: RefCell<Vec<Vec<String>>>,
        on_extract: Box<dyn Fn()>,
    }

    impl Fake {
        fn new(listing: &str) -> Self {
            Self {
                listing: listing.to_owned(),
                calls: RefCell::default(),
                on_extract: Box::new(|| {}),
            }
        }
    }

    fn output(code: i32, stdout: &str) -> Output {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt as _;
            std::process::ExitStatus::from_raw(code.cast_unsigned())
        };
        Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    impl Runner for Fake {
        fn run(&self, _: &Path, args: &[OsString]) -> io::Result<Output> {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let listing = args[0] == "l";
            self.calls.borrow_mut().push(args);
            if listing {
                Ok(output(0, &self.listing))
            } else {
                (self.on_extract)();
                Ok(output(0, ""))
            }
        }
    }

    fn run(listing: &str, root: &Path) -> (Result<()>, Vec<Vec<String>>) {
        let fake = Fake::new(listing);
        let result = extract_with(
            &fake,
            Path::new("7z.exe"),
            Path::new("download.rar"),
            &root.join("out"),
            &Limits::default(),
        );
        (result, fake.calls.take())
    }

    #[test]
    fn un_listado_inseguro_se_rechaza_antes_de_extraer() {
        // Casos de test_rar_rejects_unsafe_entries_before_extraction.
        let dir = tempfile::tempdir().unwrap();
        for (listing, expected) in [
            (
                "Path = ../escape\nSize = 1\n",
                "Ruta no portable: ../escape",
            ),
            (
                "Path = settings\nSize = 1\nSymbolic Link = outside\n",
                "7z con enlaces o entradas duplicadas",
            ),
            (
                "Path = settings\nSize = 1\nEncrypted = +\n",
                "No se admiten archivos cifrados",
            ),
        ] {
            let (result, calls) = run(listing, dir.path());
            assert_eq!(result.unwrap_err().to_string(), expected, "{listing}");
            // Una sola llamada: el listado; nunca la extracción.
            assert_eq!(calls.len(), 1, "{listing}");
            assert_eq!(calls[0][0], "l");
            assert!(!dir.path().join("out").exists());
        }
    }

    #[test]
    fn otros_listados_invalidos() {
        let dir = tempfile::tempdir().unwrap();
        let cases = [
            ("Size = 1\n", "Listado 7z no reconocido"),
            (
                "Path = a\n\nPath = A\n",
                "7z con enlaces o entradas duplicadas",
            ),
            (
                "Path = a\nAttributes = lrwxrwxrwx\n",
                "7z con enlaces o entradas duplicadas",
            ),
            (
                "Path = a\nHard Link = b\n",
                "7z con enlaces o entradas duplicadas",
            ),
            ("Path = a\nSize = 99999999999999\n", "7z demasiado grande"),
        ];
        for (listing, expected) in cases {
            let (result, calls) = run(listing, dir.path());
            assert_eq!(result.unwrap_err().to_string(), expected, "{listing}");
            assert_eq!(calls.len(), 1);
        }
        // Enlaces vacíos o `-` no cuentan.
        let (result, calls) = run(
            "Path = a\nSize = 1\nSymbolic Link = \nHard Link = -\nAttributes = -rw-r--r--\n",
            dir.path(),
        );
        result.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ["l", "-slt", "-ba", "-sccUTF-8", "download.rar"]);
        assert_eq!(calls[1][0], "x");
        assert!(calls[1][2].ends_with("out"), "{:?}", calls[1]);
        assert_eq!(&calls[1][3..], ["-y", "-bd"]);
        assert!(calls[1][2].starts_with("-o"));
    }

    #[test]
    fn el_limite_de_entradas_se_aplica_al_listado() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new("Path = a\n\nPath = b\n\nPath = c\n");
        let limits = Limits {
            max_entries: 2,
            ..Limits::default()
        };
        let error = extract_with(
            &fake,
            Path::new("7z.exe"),
            Path::new("a.7z"),
            &dir.path().join("out"),
            &limits,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "7z demasiado grande");
    }

    #[test]
    fn un_enlace_aparecido_tras_extraer_se_detecta() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let mut fake = Fake::new("Path = settings\nSize = 1\n");
        let (link_dir, link_target) = (out.clone(), outside.clone());
        fake.on_extract = Box::new(move || {
            fs::create_dir_all(&link_dir).unwrap();
            // Una junction (o un enlace simbólico) hacia fuera de `destination`.
            let link = link_dir.join("settings");
            if std::os::windows::fs::symlink_dir(&link_target, &link).is_err() {
                let status = Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(&link)
                    .arg(&link_target)
                    .output()
                    .unwrap();
                assert!(status.status.success());
            }
        });
        let result = extract_with(
            &fake,
            Path::new("7z.exe"),
            Path::new("a.rar"),
            &out,
            &Limits::default(),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Enlace inesperado tras extraer 7z"
        );
    }

    #[test]
    fn sin_7zip_no_se_toca_nada() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.7z");
        fs::write(&archive, b"7z").unwrap();
        for sevenzip in [None, Some(dir.path().join("core/tools/7zip/7z.exe"))] {
            let error =
                extract_7zip(&archive, &dir.path().join("out"), sevenzip.as_deref()).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Falta el extractor privado 7-Zip; ejecuta bootstrap"
            );
        }
        assert!(!dir.path().join("out").exists());
    }
}
