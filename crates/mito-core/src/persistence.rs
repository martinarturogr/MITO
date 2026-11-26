//! Persistencia de directorios y archivos de las aplicaciones (`persistence.py`).
//!
//! Contrato: `docs/spec/modules/persistence.md`. Los datos persistentes viven bajo
//! `home/persist/<clave>/<alias>` (o `home/profiles/<profile>/persist/<clave>/<alias>`)
//! y se enlazan a la carpeta de la aplicación: un *junction* para los directorios y un
//! *hardlink* para los archivos. Los originales de la aplicación se apartan en
//! `.mito-defaults`, y `.mito-persist.json` es el diario de los enlaces que ha creado
//! el motor: solo esos se retiran o recrean.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{Error, Result, bail};
use crate::layout::Layout;
use crate::message::Message;
use crate::storage::{identifier, inside, portable_path, read_json, resolve_lenient, write_json};
use mito_win::files::same_file;
use mito_win::links::{create_junction, is_junction, is_symlink, junction_target, remove_junction};

/// Diario de enlaces creados por el motor, dentro de la carpeta de la aplicación.
pub const JOURNAL: &str = ".mito-persist.json";
/// Carpeta (dentro de la aplicación) donde se aparta lo que había antes de persistir.
pub const DEFAULTS: &str = ".mito-defaults";

/// Una entrada de `persist`: `relative` es la ruta dentro de la aplicación y `alias`
/// la ruta bajo la carpeta persistente. Coinciden salvo en la forma `[origen, destino]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Ruta relativa dentro de la carpeta de la aplicación.
    pub relative: String,
    /// Ruta relativa bajo la carpeta de datos persistentes.
    pub alias: String,
}

impl Entry {
    /// Entrada cuyo origen y destino coinciden.
    pub fn same(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            alias: path.clone(),
            relative: path,
        }
    }

    /// Entrada con destino distinto del origen.
    pub fn aliased(relative: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            relative: relative.into(),
            alias: alias.into(),
        }
    }
}

fn invalid_persist() -> Error {
    Error::domain(Message::new(
        "persist debe ser una ruta o [origen, destino]",
    ))
}

/// Entradas del campo `persist` de un manifiesto (`definitions`).
///
/// `persist` puede faltar (sin entradas), ser una cadena o una lista de cadenas y de
/// pares `[origen, destino]`.
///
/// # Errors
///
/// `persist debe ser una ruta o [origen, destino]` si un elemento no es una cadena ni
/// un par de cadenas.
pub fn definitions(persist: Option<&Value>) -> Result<Vec<Entry>> {
    let items: Vec<&Value> = match persist {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(single) => vec![single],
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::String(path) => Ok(Entry::same(path)),
            Value::Array(pair) => match pair.as_slice() {
                [Value::String(relative), Value::String(alias)] => {
                    Ok(Entry::aliased(relative, alias))
                }
                _ => Err(invalid_persist()),
            },
            _ => Err(invalid_persist()),
        })
        .collect()
}

/// Carpeta de datos persistentes de una aplicación: `home/persist/<clave>` o, con
/// profile, `home/profiles/<profile>/persist/<clave>`.
///
/// # Errors
///
/// Los de [`identifier`] para `profile` y de [`inside`] para `key`.
pub fn persist_root(layout: &Layout, key: &str, profile: Option<&str>) -> Result<PathBuf> {
    let base = match profile {
        None => layout.home().join("persist"),
        Some(profile) => layout
            .home()
            .join("profiles")
            .join(identifier(profile)?)
            .join("persist"),
    };
    inside(&base, key)
}

/// Rutas de datos de las entradas, sin crear nada (`locations`).
///
/// Devuelve `(relativo, ruta de datos)` en el orden de `entries`.
///
/// # Errors
///
/// Los de [`persist_root`] y [`inside`] para cada alias.
pub fn locations(
    layout: &Layout,
    key: &str,
    profile: Option<&str>,
    entries: &[Entry],
) -> Result<Vec<(String, PathBuf)>> {
    let folder = persist_root(layout, key, profile)?;
    entries
        .iter()
        .map(|e| Ok((e.relative.clone(), inside(&folder, &e.alias)?)))
        .collect()
}

/// Abre en el Explorador la carpeta de una ruta de datos (la propia ruta si es un
/// directorio; su carpeta si es un archivo).
///
/// # Errors
///
/// `Esta persistencia todavía no se ha creado` si la ruta no existe.
pub fn open_location(path: &Path) -> Result<()> {
    open_location_with(path, |folder| {
        std::process::Command::new("explorer.exe")
            .arg(folder)
            .spawn()
            .map(drop)
    })
}

/// Como [`open_location`] pero con el lanzador inyectado (para probarlo sin abrir
/// ventanas).
///
/// # Errors
///
/// `Esta persistencia todavía no se ha creado` si la ruta no existe, o el error del
/// lanzador.
pub fn open_location_with(path: &Path, launch: impl FnOnce(&Path) -> io::Result<()>) -> Result<()> {
    if !path.exists() {
        return bail(Message::new("Esta persistencia todavía no se ha creado"));
    }
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    launch(folder).map_err(|e| Error::io(folder, e))
}

/// Normaliza una ruta para compararla: sin prefijo `\\?\`, sin `.` ni `..` léxicos y en
/// minúsculas (`normcase(normpath(...))`).
fn comparable(path: &Path) -> String {
    let mut parts: Vec<std::path::Component<'_>> = Vec::new();
    let simplified = dunce::simplified(path);
    for component in simplified.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
                if matches!(parts.last(), Some(std::path::Component::Normal(_))) =>
            {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts
        .iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_lowercase()
}

/// ¿Es `source` ya el enlace del motor hacia `target` (junction o hardlink)?
/// Nunca falla: cualquier error de E/S cuenta como «no enlazado».
pub fn linked(source: &Path, target: &Path) -> bool {
    if is_junction(source) && target.is_dir() {
        let absolute = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
        return junction_target(source).is_ok_and(|t| comparable(&t) == comparable(&absolute));
    }
    source.is_file()
        && !is_symlink(source)
        && target.is_file()
        && same_file(source, target).unwrap_or(false)
}

/// ¿Parece el último segmento de `path` un archivo? Mismo criterio que Scoop: tiene un
/// punto seguido de al menos un carácter (`config.ini`, `.gitconfig`; no `data`).
fn looks_like_file(path: &str) -> bool {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    last.find('.').is_some_and(|i| i + 1 < last.len())
}

fn read_journal(path: &Path) -> Result<Map<String, Value>> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(Map::new())
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (source, destination) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            copy_file(&source, &destination)?;
        }
    }
    Ok(())
}

/// Copia un archivo conservando fecha de modificación y atributos (`shutil.copy2`).
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to)?;
    let modified = fs::metadata(from)?.modified()?;
    OpenOptions::new()
        .write(true)
        .open(to)?
        .set_modified(modified)
}

/// Crea el destino de una entrada que no existía en ningún sitio: un archivo vacío si
/// su nombre parece de archivo y un directorio en otro caso.
fn create_empty_target(target: &Path, alias: &str) -> io::Result<()> {
    if looks_like_file(alias) {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create_new(target).map(drop)
    } else {
        fs::create_dir_all(target)
    }
}

/// Prepara la persistencia de una aplicación instalada en `payload`.
///
/// Para cada entrada: aparta lo que hubiera en la aplicación a `.mito-defaults`, copia
/// esos valores a la carpeta de datos si aún no existe, anota el enlace en el diario
/// `.mito-persist.json` y crea el enlace (junction para directorios, hardlink para
/// archivos). Un enlace ya correcto se omite. `key` es la clave de la aplicación
/// (`bucket/app`) y `destination` la carpeta de datos si no es la de `home/persist`.
///
/// Si la aplicación no declara entradas no se crea nada. No hay *rollback*: el diario se
/// escribe antes de cada enlace, de modo que una preparación interrumpida se puede
/// reintentar.
///
/// # Errors
///
/// Los de [`portable_path`] e [`inside`] para cada ruta, `persist requiere una ruta
/// concreta` (ruta vacía), `persist anidado dentro de otro enlace`, `Definición persist
/// modificada inesperadamente`, `Se esperaba un enlace persistente`, `No se reemplaza
/// un enlace ajeno al motor`, `Ruta persist fuera de MITO` y los errores de E/S.
pub fn prepare(
    layout: &Layout,
    payload: &Path,
    key: &str,
    entries: &[Entry],
    destination: Option<&Path>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let journal_path = payload.join(JOURNAL);
    let mut journal = read_journal(&journal_path)?;
    let data_root = match destination {
        Some(folder) => folder.to_path_buf(),
        None => inside(&layout.home().join("persist"), key)?,
    };
    for entry in entries {
        prepare_entry(
            layout,
            payload,
            &data_root,
            entry,
            &mut journal,
            &journal_path,
        )?;
    }
    Ok(())
}

fn prepare_entry(
    layout: &Layout,
    payload: &Path,
    data_root: &Path,
    entry: &Entry,
    journal: &mut Map<String, Value>,
    journal_path: &Path,
) -> Result<()> {
    let Entry { relative, alias } = entry;
    // La sintaxis se valida con independencia del destino absoluto de un enlace viejo.
    let portable = portable_path(relative)?;
    if portable.is_empty() || portable_path(alias)?.is_empty() {
        return bail(Message::new("persist requiere una ruta concreta"));
    }
    let source = portable.join_to(payload);
    let parent = source.parent().unwrap_or(payload);
    if !resolve_lenient(parent).starts_with(resolve_lenient(payload)) {
        return bail(Message::new("persist anidado dentro de otro enlace"));
    }
    let target = inside(data_root, alias)?;
    let defaults = inside(&payload.join(DEFAULTS), relative)?;
    let registered = journal.get(relative).and_then(Value::as_str);
    if registered == Some(alias.as_str()) && linked(&source, &target) {
        // Reconstruir un enlace intacto en cada arranque sería trabajo inútil.
        return Ok(());
    }
    if let Some(previous) = registered {
        if previous != alias {
            return bail(Message::new(
                "Definición persist modificada inesperadamente",
            ));
        }
        if is_junction(&source) {
            // Quita solo el punto de reanálisis, nunca el contenido del destino.
            remove_junction(&source).map_err(|e| Error::io(&source, e))?;
        } else if source.is_file() {
            fs::remove_file(&source).map_err(|e| Error::io(&source, e))?;
        } else if source.exists() {
            return bail(Message::new("Se esperaba un enlace persistente"));
        }
    } else if is_junction(&source) || is_symlink(&source) {
        return bail(Message::new("No se reemplaza un enlace ajeno al motor"));
    } else if source.exists() {
        let defaults_parent = defaults.parent().unwrap_or(payload);
        fs::create_dir_all(defaults_parent).map_err(|e| Error::io(defaults_parent, e))?;
        let root = resolve_lenient(layout.root());
        if !resolve_lenient(&source).starts_with(&root)
            || !resolve_lenient(&defaults).starts_with(&root)
        {
            return bail(Message::new("Ruta persist fuera de MITO"));
        }
        fs::rename(&source, &defaults).map_err(|e| Error::io(&source, e))?;
    }
    if !target.exists() && defaults.exists() {
        let parent = target.parent().unwrap_or(data_root);
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        if defaults.is_dir() {
            copy_tree(&defaults, &target).map_err(|e| Error::io(&target, e))?;
        } else {
            copy_file(&defaults, &target).map_err(|e| Error::io(&target, e))?;
        }
    }
    if !target.exists() {
        // Diferencia deliberada con el original, que creaba siempre un directorio y así
        // convertía `config.ini` en una carpeta: un archivo se persiste como archivo.
        create_empty_target(&target, alias).map_err(|e| Error::io(&target, e))?;
    }
    let link_parent = source.parent().unwrap_or(payload);
    fs::create_dir_all(link_parent).map_err(|e| Error::io(link_parent, e))?;
    // Diario antes del enlace: una creación interrumpida se puede reintentar.
    journal.insert(relative.clone(), Value::String(alias.clone()));
    write_json(journal_path, &*journal)?;
    if target.is_dir() {
        create_junction(&target, &source).map_err(|e| Error::io(&source, e))
    } else {
        fs::hard_link(&target, &source).map_err(|e| Error::io(&source, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;
    use std::cell::RefCell;

    struct Fixture {
        _dir: tempfile::TempDir,
        layout: Layout,
        payload: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let payload = layout.apps().join("main").join("editor");
        fs::create_dir_all(&payload).unwrap();
        Fixture {
            _dir: dir,
            layout,
            payload,
        }
    }

    fn data(f: &Fixture) -> PathBuf {
        f.layout.home().join("persist").join("main").join("editor")
    }

    fn run(f: &Fixture, entries: &[Entry]) -> Result<()> {
        prepare(&f.layout, &f.payload, "main/editor", entries, None)
    }

    fn message(result: Result<()>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn las_rutas_siguen_el_ambito_del_manifiesto_y_los_alias() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let entries = definitions(Some(&json!([["data", "saved"], "config.ini"]))).unwrap();
        let one = locations(&layout, "main/editor", Some("one"), &entries).unwrap();
        assert_eq!(
            one[0].1,
            dir.path()
                .join("home/profiles/one/persist/main/editor/saved")
        );
        assert_eq!(one[0].0, "data");
        let two = locations(&layout, "main/editor", Some("two"), &entries).unwrap();
        assert_ne!(one[0].1, two[0].1);
        let global = locations(&layout, "main/editor", None, &entries).unwrap();
        assert_eq!(
            global[1].1,
            dir.path().join("home/persist/main/editor/config.ini")
        );
        assert!(!dir.path().join("home").exists(), "locations no crea nada");
    }

    #[test]
    fn los_archivos_abren_su_carpeta_y_las_rutas_inexistentes_se_rechazan() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.ini");
        fs::write(&file, "x").unwrap();
        let opened = RefCell::new(Vec::new());
        open_location_with(&file, |folder| {
            opened.borrow_mut().push(folder.to_path_buf());
            Ok(())
        })
        .unwrap();
        open_location_with(dir.path(), |folder| {
            opened.borrow_mut().push(folder.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            opened.into_inner(),
            [dir.path().to_path_buf(), dir.path().to_path_buf()]
        );
        let missing = open_location_with(&dir.path().join("missing"), |_| Ok(()));
        assert_eq!(
            message(missing),
            "Esta persistencia todavía no se ha creado"
        );
    }

    #[test]
    fn definiciones_del_manifiesto() {
        assert_eq!(definitions(None).unwrap(), []);
        assert_eq!(definitions(Some(&json!(null))).unwrap(), []);
        assert_eq!(definitions(Some(&json!([]))).unwrap(), []);
        assert_eq!(
            definitions(Some(&json!("data"))).unwrap(),
            [Entry::same("data")]
        );
        assert_eq!(
            definitions(Some(&json!(["a", ["b", "c"]]))).unwrap(),
            [Entry::same("a"), Entry::aliased("b", "c")]
        );
        for bad in [
            json!([["a"]]),
            json!([["a", "b", "c"]]),
            json!([1]),
            json!([["a", 1]]),
        ] {
            assert_eq!(
                definitions(Some(&bad)).unwrap_err().to_string(),
                "persist debe ser una ruta o [origen, destino]",
                "{bad}"
            );
        }
    }

    #[test]
    fn un_directorio_existente_se_aparta_se_copia_y_se_enlaza() {
        let f = fixture();
        fs::create_dir_all(f.payload.join("data/sub")).unwrap();
        fs::write(f.payload.join("data/a.txt"), "uno").unwrap();
        fs::write(f.payload.join("data/sub/b.txt"), "dos").unwrap();
        run(&f, &[Entry::aliased("data", "saved")]).unwrap();
        let target = data(&f).join("saved");
        assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "uno");
        assert_eq!(fs::read_to_string(target.join("sub/b.txt")).unwrap(), "dos");
        assert!(is_junction(&f.payload.join("data")));
        assert_eq!(
            fs::read_to_string(f.payload.join(DEFAULTS).join("data/a.txt")).unwrap(),
            "uno"
        );
        // Lo que escribe la aplicación llega a la carpeta persistente.
        fs::write(f.payload.join("data/nuevo.txt"), "n").unwrap();
        assert!(target.join("nuevo.txt").exists());
        let journal: Value = read_json(&f.payload.join(JOURNAL)).unwrap();
        assert_eq!(journal, json!({"data": "saved"}));
    }

    #[test]
    fn un_archivo_existente_se_enlaza_con_un_hardlink() {
        let f = fixture();
        fs::write(f.payload.join("config.ini"), "valor").unwrap();
        run(&f, &[Entry::same("config.ini")]).unwrap();
        let target = data(&f).join("config.ini");
        assert_eq!(fs::read_to_string(&target).unwrap(), "valor");
        assert!(target.is_file());
        assert!(same_file(&f.payload.join("config.ini"), &target).unwrap());
        fs::write(f.payload.join("config.ini"), "cambiado").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "cambiado");
        assert_eq!(
            fs::read_to_string(f.payload.join(DEFAULTS).join("config.ini")).unwrap(),
            "valor"
        );
    }

    #[test]
    fn un_archivo_sin_origen_previo_se_persiste_como_archivo() {
        let f = fixture();
        run(
            &f,
            &[Entry::same("config.ini"), Entry::same("sub/dir/state.json")],
        )
        .unwrap();
        for path in ["config.ini", "sub/dir/state.json"] {
            let source = f.payload.join(path);
            assert!(source.is_file(), "{path} debe ser un archivo");
            assert!(data(&f).join(path).is_file());
            assert!(!is_junction(&source));
            assert!(same_file(&source, &data(&f).join(path)).unwrap());
        }
    }

    #[test]
    fn un_directorio_sin_origen_previo_se_persiste_como_directorio() {
        let f = fixture();
        run(&f, &[Entry::same("data")]).unwrap();
        assert!(is_junction(&f.payload.join("data")));
        assert!(data(&f).join("data").is_dir());
    }

    #[test]
    fn preparar_dos_veces_no_cambia_nada_y_un_enlace_roto_se_reconstruye() {
        let f = fixture();
        fs::write(f.payload.join("a.ini"), "x").unwrap();
        let entries = [Entry::same("data"), Entry::same("a.ini")];
        run(&f, &entries).unwrap();
        assert!(linked(&f.payload.join("data"), &data(&f).join("data")));
        assert!(linked(&f.payload.join("a.ini"), &data(&f).join("a.ini")));
        let journal_before = fs::read(f.payload.join(JOURNAL)).unwrap();
        run(&f, &entries).unwrap();
        assert_eq!(fs::read(f.payload.join(JOURNAL)).unwrap(), journal_before);
        // Un junction que apunta a otro sitio (instancia movida) se recrea.
        let other = f.layout.root().join("otro");
        fs::create_dir_all(&other).unwrap();
        remove_junction(&f.payload.join("data")).unwrap();
        create_junction(&other, &f.payload.join("data")).unwrap();
        assert!(!linked(&f.payload.join("data"), &data(&f).join("data")));
        run(&f, &entries).unwrap();
        assert!(linked(&f.payload.join("data"), &data(&f).join("data")));
        assert!(other.exists(), "el destino viejo no se toca");
    }

    #[test]
    fn sin_entradas_no_se_crea_nada() {
        let f = fixture();
        run(&f, &[]).unwrap();
        assert!(!f.payload.join(JOURNAL).exists());
        assert!(!f.layout.home().exists());
    }

    #[test]
    fn el_destino_alternativo_sustituye_a_home_persist() {
        let f = fixture();
        let destination = f.layout.home().join("profiles/p/persist/main/editor");
        prepare(
            &f.layout,
            &f.payload,
            "main/editor",
            &[Entry::same("data")],
            Some(&destination),
        )
        .unwrap();
        assert!(destination.join("data").is_dir());
        assert!(!f.layout.home().join("persist").exists());
    }

    #[test]
    fn un_alias_distinto_al_registrado_es_un_error() {
        let f = fixture();
        run(&f, &[Entry::aliased("data", "uno")]).unwrap();
        assert_eq!(
            message(run(&f, &[Entry::aliased("data", "dos")])),
            "Definición persist modificada inesperadamente"
        );
    }

    #[test]
    fn un_enlace_ajeno_no_se_reemplaza() {
        let f = fixture();
        let outside = f.layout.root().join("fuera");
        fs::create_dir_all(&outside).unwrap();
        create_junction(&outside, &f.payload.join("data")).unwrap();
        assert_eq!(
            message(run(&f, &[Entry::same("data")])),
            "No se reemplaza un enlace ajeno al motor"
        );
        assert!(is_junction(&f.payload.join("data")));
    }

    #[test]
    fn un_directorio_real_donde_el_diario_espera_un_enlace_es_un_error() {
        let f = fixture();
        write_json(&f.payload.join(JOURNAL), &json!({"data": "data"})).unwrap();
        fs::create_dir_all(f.payload.join("data")).unwrap();
        fs::write(f.payload.join("data/precioso.txt"), "x").unwrap();
        assert_eq!(
            message(run(&f, &[Entry::same("data")])),
            "Se esperaba un enlace persistente"
        );
        assert!(f.payload.join("data/precioso.txt").exists());
    }

    #[test]
    fn un_persist_dentro_de_otro_enlace_se_rechaza() {
        let f = fixture();
        let outside = f.layout.root().join("fuera");
        fs::create_dir_all(&outside).unwrap();
        create_junction(&outside, &f.payload.join("enlace")).unwrap();
        assert_eq!(
            message(run(&f, &[Entry::same("enlace/dato.txt")])),
            "persist anidado dentro de otro enlace"
        );
        assert!(!outside.join("dato.txt").exists());
    }

    #[test]
    fn rutas_invalidas_o_vacias_se_rechazan() {
        let f = fixture();
        for bad in ["../x", "C:/x", "a/../b", "con", "a*b"] {
            assert!(run(&f, &[Entry::same(bad)]).is_err(), "{bad}");
        }
        assert!(run(&f, &[Entry::aliased("ok", "../fuera")]).is_err());
        for empty in ["", ".", "./"] {
            assert_eq!(
                message(run(&f, &[Entry::same(empty)])),
                "persist requiere una ruta concreta",
                "{empty:?}"
            );
        }
        assert!(!f.payload.join(JOURNAL).exists());
    }

    #[test]
    fn linked_es_falso_para_lo_que_no_es_el_enlace_del_motor() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        assert!(!linked(&a, &b));
        assert!(!linked(&dir.path().join("falta"), &b));
        fs::write(dir.path().join("f1"), "x").unwrap();
        fs::copy(dir.path().join("f1"), dir.path().join("f2")).unwrap();
        assert!(!linked(&dir.path().join("f1"), &dir.path().join("f2")));
    }

    #[test]
    fn criterio_de_archivo_como_scoop() {
        for file in ["config.ini", "a/b/state.json", ".gitconfig", "x.tar.gz"] {
            assert!(looks_like_file(file), "{file}");
        }
        for dir in ["data", "a/b/profiles", "carpeta.", "dir/sin"] {
            assert!(!looks_like_file(dir), "{dir}");
        }
    }

    proptest! {
        #[test]
        fn las_rutas_de_datos_quedan_siempre_bajo_su_carpeta(
            alias in "[a-z./\\\\]{0,12}",
            key in "[a-z]{1,4}(/[a-z]{1,4}){0,2}",
        ) {
            let dir = tempfile::tempdir().unwrap();
            let layout = Layout::new(dir.path());
            let entries = [Entry::aliased("x", alias)];
            if let Ok(paths) = locations(&layout, &key, None, &entries) {
                let root = persist_root(&layout, &key, None).unwrap();
                for (_, path) in paths {
                    prop_assert!(path.starts_with(&root));
                }
            }
        }
    }
}
