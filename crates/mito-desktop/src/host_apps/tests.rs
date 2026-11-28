//! Pruebas de `host_apps` (`test_host_apps.py`). Los accesos de `.lnk` se leen con un
//! lector inyectado y las pruebas de ejecución usan el modo `owned`, que espera al
//! proceso: ninguna toca el escritorio ni el menú Inicio reales.

use std::cell::Cell;
use std::collections::BTreeMap;

use mito_engine::Profile;

use super::*;

struct Rig {
    _tmp: tempfile::TempDir,
    folder: PathBuf,
    engine: Engine,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let folder = dunce::canonicalize(tmp.path()).unwrap();
    let engine = Engine::new(folder.join("mito"));
    engine.initialize().unwrap();
    for name in ["one", "two"] {
        let mut profile = Profile::new(name);
        profile.env = vec![("EXTERNAL_APP_TEST".to_owned(), name.to_owned())];
        engine.save_profile(name, &profile).unwrap();
    }
    Rig {
        _tmp: tmp,
        folder,
        engine,
    }
}

impl Rig {
    fn program(&self, relative: &str) -> PathBuf {
        let path = relative
            .split('/')
            .fold(self.folder.clone(), |path, part| path.join(part));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"exe").unwrap();
        path
    }

    fn root(&self) -> &Path {
        self.engine.root()
    }
}

fn message(result: Result<impl std::fmt::Debug>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn la_identidad_es_el_sha256_de_la_ruta_sin_mayusculas() {
    // SHA-256 de `c:\apps\editor\editor.exe`, calculado de forma independiente.
    assert_eq!(
        identity(Path::new(r"C:\Apps\Editor\editor.exe")),
        "2c01437f9a3ead99"
    );
    assert_eq!(
        identity(Path::new(r"c:\APPS\editor\EDITOR.EXE")),
        identity(Path::new(r"C:\Apps\Editor\editor.exe"))
    );
}

#[test]
fn registrar_deduplica_y_quitar_solo_quita_la_referencia() {
    let rig = rig();
    let application = rig.program("Host App.exe");
    let id = add(rig.root(), &application).unwrap();
    assert_eq!(id, add(rig.root(), &application).unwrap());
    assert_eq!(inventory(rig.root()).unwrap().len(), 1);
    assert_eq!(target(rig.root(), &id).unwrap(), application);
    let host = HostRoots::new(rig.folder.join("roaming"), rig.folder.join("local"));
    remove(rig.root(), &host, &id).unwrap();
    assert_eq!(inventory(rig.root()).unwrap(), []);
    assert_eq!(fs::read(&application).unwrap(), b"exe");
    assert_eq!(
        message(launch(&rig.engine, "one", &id, &[], false, false)),
        "La aplicación del host ya no está registrada en MITO"
    );
    let text = rig.program("Host App.txt");
    assert_eq!(
        message(add(rig.root(), &text)),
        "Selecciona un archivo .exe, .bat, .cmd o .ps1 existente"
    );
    assert!(add(rig.root(), Path::new("relativa.exe")).is_err());
    assert!(add(rig.root(), &rig.folder.join("falta.exe")).is_err());
}

#[test]
fn el_inventario_tiene_el_formato_del_original() {
    let rig = rig();
    let application = rig.program("Editor/editor.exe");
    let id = add(rig.root(), &application).unwrap();
    let text = fs::read_to_string(rig.root().join(INVENTORY)).unwrap();
    assert_eq!(
        text,
        format!(
            "{{\n  \"{id}\": {}\n}}\n",
            serde_json::to_string(&application.display().to_string()).unwrap()
        )
    );
}

#[test]
fn quitar_una_aplicacion_conserva_el_orden_de_las_demas() {
    let rig = rig();
    let host = HostRoots::new(rig.folder.join("roaming"), rig.folder.join("local"));
    let ids: Vec<String> = ["b.exe", "a.exe", "c.exe"]
        .iter()
        .map(|name| add(rig.root(), &rig.program(name)).unwrap())
        .collect();
    remove(rig.root(), &host, &ids[1]).unwrap();
    let left: Vec<String> = inventory(rig.root())
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(left, [ids[0].clone(), ids[2].clone()]);
}

#[test]
fn la_aplicacion_usa_el_entorno_del_profile_seleccionado() {
    let rig = rig();
    let application = rig.program("Application.exe");
    let id = add(rig.root(), &application).unwrap();
    for profile in ["one", "two"] {
        let (command, flags) = launch_command(&rig.engine, profile, &id, &[]).unwrap();
        assert_eq!(flags, 0);
        assert_eq!(command.env["MITO_PROFILE"], profile);
        assert_eq!(command.env["EXTERNAL_APP_TEST"], profile);
        let appdata = rig.root().join("home").join("AppData");
        assert_eq!(
            command.env["APPDATA"],
            appdata.join("Roaming").display().to_string()
        );
        assert_eq!(
            command.env["LOCALAPPDATA"],
            appdata.join("Local").display().to_string()
        );
        let shims = rig
            .root()
            .join("core")
            .join("state")
            .join("shims")
            .join(profile);
        assert!(command.env["PATH"].contains(&shims.display().to_string()));
    }
}

#[test]
fn un_exe_se_lanza_en_el_workspace_del_profile_con_su_entorno() {
    let rig = rig();
    let application = rig.program("Application.exe");
    let id = add(rig.root(), &application).unwrap();
    let expected = rig.engine.environment("one").unwrap();
    let args = vec!["--name".to_owned(), "two words".to_owned()];
    let (command, flags) = launch_command(&rig.engine, "one", &id, &args).unwrap();
    assert_eq!(flags, 0);
    assert_eq!(command.program, application.display().to_string());
    assert_eq!(command.args, args);
    assert_eq!(command.raw_args, None);
    assert_eq!(command.cwd, rig.root().join("workspaces/one"));
    assert_eq!(command.env, expected);
    assert!(command.cwd.is_dir());
}

#[test]
fn los_bat_y_ps1_usan_su_interprete_en_una_consola_nueva() {
    let rig = rig();
    let script = rig.program("tool.cmd");
    let id = add(rig.root(), &script).unwrap();
    let (command, flags) = launch_command(&rig.engine, "one", &id, &["a b".to_owned()]).unwrap();
    assert_eq!(flags, CREATE_NEW_CONSOLE);
    assert!(
        command
            .program
            .to_lowercase()
            .ends_with(r"system32\cmd.exe")
    );
    assert_eq!(command.args, Vec::<String>::new());
    assert_eq!(
        command.raw_args.as_deref(),
        Some(r#"/d /v:off /s /c ""%MITO_COMMAND_ARG_0%" "%MITO_COMMAND_ARG_1%"""#)
    );
    assert_eq!(command.env["MITO_COMMAND_ARG_1"], "a b");

    let ps1 = rig.program("tool.ps1");
    let id = add(rig.root(), &ps1).unwrap();
    let (command, flags) = launch_command(&rig.engine, "one", &id, &["x".to_owned()]).unwrap();
    assert_eq!(flags, CREATE_NEW_CONSOLE);
    assert!(
        command
            .program
            .to_lowercase()
            .ends_with(r"windowspowershell\v1.0\powershell.exe")
    );
    assert_eq!(
        command.args,
        [
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &ps1.display().to_string(),
            "x"
        ]
    );
}

#[test]
fn un_bat_rechaza_argumentos_con_comillas() {
    let rig = rig();
    let id = add(rig.root(), &rig.program("tool.bat")).unwrap();
    assert!(
        message(launch_command(
            &rig.engine,
            "one",
            &id,
            &["a\"b".to_owned()]
        ))
        .starts_with("INVALID_ARGUMENT: BAT/CMD argv cannot contain quotes or newlines")
    );
}

#[test]
fn rutas_con_metacaracteres_y_powershell_se_ejecutan() {
    let rig = rig();
    let expected = rig.engine.environment("one").unwrap();
    for extension in ["cmd", "bat"] {
        let application = rig.folder.join(format!("tool %TEMP% ! & ^.{extension}"));
        fs::write(
            &application,
            "@echo off\r\n> \"%~dp0result.txt\" echo %APPDATA%\r\n",
        )
        .unwrap();
        let id = add(rig.root(), &application).unwrap();
        assert_eq!(
            launch(&rig.engine, "one", &id, &[], false, true).unwrap(),
            0
        );
        let result = rig.folder.join("result.txt");
        assert_eq!(
            fs::read_to_string(&result).unwrap().trim(),
            expected["APPDATA"]
        );
        fs::remove_file(result).unwrap();
    }
    let application = rig.folder.join("tool's & !%.ps1");
    fs::write(
        &application,
        "@{AppData=$env:APPDATA;LocalAppData=$env:LOCALAPPDATA;Profile=$env:MITO_PROFILE} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'result.json') -Encoding UTF8",
    )
    .unwrap();
    let id = add(rig.root(), &application).unwrap();
    assert_eq!(
        launch(&rig.engine, "one", &id, &[], false, true).unwrap(),
        0
    );
    let text = fs::read_to_string(rig.folder.join("result.json")).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
    assert_eq!(data["AppData"], expected["APPDATA"]);
    assert_eq!(data["LocalAppData"], expected["LOCALAPPDATA"]);
    assert_eq!(data["Profile"], "one");
}

#[test]
fn owned_devuelve_el_codigo_de_salida_con_los_argumentos() {
    let rig = rig();
    let application = rig.folder.join("exit.cmd");
    fs::write(&application, "@echo off\r\nexit /b %~1\r\n").unwrap();
    let id = add(rig.root(), &application).unwrap();
    let code = launch(&rig.engine, "one", &id, &["7".to_owned()], false, true).unwrap();
    assert_eq!(code, 7);
}

#[test]
fn coupled_y_owned_a_la_vez_no_son_validos() {
    let rig = rig();
    let id = add(rig.root(), &rig.program("Host App.exe")).unwrap();
    assert_eq!(
        message(launch(&rig.engine, "one", &id, &[], true, true)),
        "Modo de lanzamiento no válido"
    );
}

#[test]
fn coupled_abre_una_pestana_de_la_terminal() {
    let rig = rig();
    let id = add(rig.root(), &rig.program("Host App.exe")).unwrap();
    // Sin Windows Terminal portable en la instancia temporal, el error demuestra que
    // el lanzamiento acoplado se delega en la terminal.
    assert_eq!(
        message(launch(
            &rig.engine,
            "one",
            &id,
            &["--name".to_owned()],
            true,
            false
        )),
        "Falta Windows Terminal portable. Ejecuta mito.cmd bootstrap."
    );
}

#[test]
fn un_archivo_desaparecido_no_se_lanza() {
    let rig = rig();
    let application = rig.program("Gone.exe");
    let id = add(rig.root(), &application).unwrap();
    fs::remove_file(&application).unwrap();
    assert_eq!(
        message(launch(&rig.engine, "one", &id, &[], false, false)),
        "El archivo de la aplicación del host ya no está disponible"
    );
}

// --- Descubrimiento ---------------------------------------------------------------------

struct Menu {
    rig: Rig,
    start: PathBuf,
    targets: BTreeMap<PathBuf, PathBuf>,
}

fn menu() -> Menu {
    let rig = rig();
    let menu = rig.folder.join("Start Menu");
    fs::create_dir_all(menu.join("Tools")).unwrap();
    Menu {
        rig,
        start: menu,
        targets: BTreeMap::new(),
    }
}

impl Menu {
    fn shortcut(&mut self, name: &str, target: &Path) {
        let link = self.start.join(format!("{name}.lnk"));
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::write(&link, b"lnk").unwrap();
        self.targets.insert(link, target.to_owned());
    }

    fn found(&self) -> Vec<(String, PathBuf)> {
        discover(
            self.rig.root(),
            &[self.start.clone(), self.rig.folder.join("absent")],
            &|link| self.targets.get(link).cloned(),
        )
        .unwrap()
    }
}

#[test]
fn discover_filtra_y_deduplica() {
    let mut menu = menu();
    let editor = menu.rig.program("Editor/editor.exe");
    menu.shortcut("Editor", &editor);
    menu.shortcut("Tools/Editor (copia)", &editor);
    let uninstall = menu.rig.program("Editor/unins000.exe");
    menu.shortcut("Uninstall Editor", &uninstall);
    let manual = menu.rig.program("Editor/manual.pdf");
    menu.shortcut("Manual", &manual);
    let gone = menu.rig.folder.join("Gone/gone.exe");
    menu.shortcut("Missing", &gone);
    let inside = menu.rig.program("mito/core/apps/x/x.exe");
    menu.shortcut("Inside MITO", &inside);
    let added = menu.rig.program("Added/added.exe");
    menu.shortcut("Added", &added);
    add(menu.rig.root(), &added).unwrap();
    let script = menu.rig.program("Scripts/run.cmd");
    menu.shortcut("Tools/Run", &script);
    // Un acceso cuyo nombre es de desinstalador aunque el destino no lo sea.
    let other = menu.rig.program("Other/other.exe");
    menu.shortcut("Desinstalar Other", &other);
    assert_eq!(
        menu.found(),
        [("Editor".to_owned(), editor), ("Run".to_owned(), script),]
    );
}

#[test]
fn discover_sin_accesos_no_resuelve() {
    let rig = rig();
    let calls = Cell::new(0);
    let found = discover(rig.root(), &[rig.folder.join("empty")], &|_| {
        calls.set(calls.get() + 1);
        None
    })
    .unwrap();
    assert_eq!(found, []);
    assert_eq!(calls.get(), 0);
}

#[test]
fn discover_ordena_por_etiqueta_sin_mayusculas() {
    let mut menu = menu();
    let zeta = menu.rig.program("z/zeta.exe");
    let alfa = menu.rig.program("a/alfa.exe");
    menu.shortcut("zeta", &zeta);
    menu.shortcut("Alfa", &alfa);
    let labels: Vec<String> = menu.found().into_iter().map(|(label, _)| label).collect();
    assert_eq!(labels, ["Alfa", "zeta"]);
}

#[test]
fn un_destino_irresoluble_se_salta() {
    let mut menu = menu();
    let link = menu.start.join("Broken.lnk");
    fs::write(&link, b"lnk").unwrap();
    let editor = menu.rig.program("Editor/editor.exe");
    menu.shortcut("Editor", &editor);
    assert_eq!(menu.found().len(), 1);
}

#[test]
fn reconoce_desinstaladores() {
    for name in [
        "unins000.exe",
        "Uninstall App.exe",
        "Desinstalar.exe",
        "UNINS.exe",
    ] {
        assert!(uninstaller(Path::new(name)), "{name}");
    }
    assert!(!uninstaller(Path::new("editor.exe")));
}

#[test]
fn las_carpetas_de_accesos_se_leen_del_shell() {
    // Solo lectura de carpetas conocidas: no se crea ni se toca nada.
    let folders = shortcut_folders();
    assert_ne!(folders, Vec::<PathBuf>::new());
    assert!(folders.iter().all(|f| f.is_absolute()));
}

#[test]
fn discover_no_sigue_junctions_ni_se_queda_en_bucles() {
    let mut menu = menu();
    let editor = menu.rig.folder.join("editor.exe");
    fs::write(&editor, b"MZ").unwrap();
    menu.shortcut("Editor", &editor);
    // Un junction que apunta a su propia carpeta padre: seguirlo no terminaría nunca.
    mito_win::links::create_junction(&menu.start, &menu.start.join("Tools").join("loop")).unwrap();
    let found = menu.found();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "Editor");
}
