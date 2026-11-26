//! Pruebas del traductor y de la selección de idioma.

use std::cell::Cell;
use std::fs;

use mito_core::config::Configuration;
use mito_core::{Layout, Message};

use crate::{Bundle, LANGUAGES, Translator, bundle, language_for_langid, system_language};

fn open(template: &'static str) -> Message {
    Message::new(template)
}

fn created(value: &str) -> Message {
    Message::new("Profile {p1} creado").arg("p1", value)
}

#[test]
fn el_ingles_traduce_y_los_valores_no_se_traducen_ni_se_reinterpolan() {
    let english = Translator::new("english");
    assert_eq!(english.tr(&open("Abrir")), "Open");
    assert_eq!(
        english.tr(&created("Eliminar {x}")),
        "Profile Eliminar {x} created"
    );
    assert_eq!(english.tr(&open("unknown.message")), "unknown.message");
    assert_eq!(english.text("Abrir"), "Open");
    assert_eq!(english.text("unknown.message"), "unknown.message");
}

#[test]
fn un_idioma_fuera_de_la_lista_es_ingles() {
    for bad in ["../../outside", "french", "ENGLISH", "", "../english"] {
        assert_eq!(Translator::new(bad).language(), "english", "{bad}");
    }
    assert_eq!(Translator::new("español").language(), "español");
}

#[test]
fn el_espanol_es_la_identidad() {
    let spanish = Translator::new("español");
    assert_eq!(spanish.tr(&created("dev")), "Profile dev creado");
    assert_eq!(spanish.tr(&open("Abrir")), "Abrir");
}

#[test]
fn sin_catalogos_se_usa_la_plantilla_espanola_interpolada() {
    let translator = Translator::with_bundles("english", Bundle::empty(), Bundle::empty());
    assert_eq!(translator.tr(&created("one")), "Profile one creado");
}

#[test]
fn un_catalogo_activo_vacio_usa_el_ingles() {
    let translator =
        Translator::with_bundles("español", Bundle::empty(), bundle("english").clone());
    assert_eq!(translator.tr(&open("Abrir")), "Open");
}

#[test]
fn las_entradas_invalidas_del_catalogo_caen_a_la_plantilla() {
    let english = Bundle::parse(
        r#"{"Abrir": 42, "Profile {p1} creado": "Profile {missing}", "Cerrar": "Close"}"#,
    );
    let translator = Translator::with_bundles("english", english.clone(), english);
    assert_eq!(translator.tr(&open("Cerrar")), "Close");
    assert_eq!(translator.tr(&open("Abrir")), "Abrir");
    assert_eq!(translator.tr(&created("test")), "Profile test creado");
}

#[test]
fn parametros_incompatibles_cuentan_como_ausentes_y_se_busca_en_ingles() {
    let message = Message::new("Hola {name}").arg("name", "Ana");
    let translator = Translator::with_bundles(
        "français",
        Bundle::empty(),
        Bundle::parse(r#"{"Hola {name}": "Hello {name}"}"#),
    );
    assert_eq!(translator.tr(&message), "Hello Ana");
    // Si el mensaje no trae el argumento que pide la traducción, esta se descarta.
    assert_eq!(translator.tr(&Message::new("Hola {name}")), "Hola {name}");
}

#[test]
fn el_sufijo_no_se_traduce() {
    let message = Message::new("Variable reservada por MITO: ").with_suffix("PATH");
    assert_eq!(
        Translator::new("español").tr(&message),
        "Variable reservada por MITO: PATH"
    );
    let english = Translator::new("english").tr(&message);
    assert!(english.ends_with("PATH"));
    assert!(!english.starts_with("Variable reservada"), "{english}");
}

#[test]
fn el_idioma_primario_de_windows_decide_incluidas_las_variantes_regionales() {
    let table = [
        (0x040a, "español"),
        (0x0c0a, "español"),
        (0x080a, "español"),
        (0x0409, "english"),
        (0x0809, "english"),
        (0x040c, "français"),
        (0x0407, "deutsch"),
        (0x0402, "български"),
        (0x041a, "hrvatski"),
        (0x0405, "čeština"),
        (0x0406, "dansk"),
        (0x0413, "nederlands"),
        (0x0425, "eesti"),
        (0x040b, "suomi"),
        (0x0408, "ελληνικά"),
        (0x040e, "magyar"),
        (0x043c, "gaeilge"),
        (0x0410, "italiano"),
        (0x0426, "latviešu"),
        (0x0427, "lietuvių"),
        (0x043a, "malti"),
        (0x0415, "polski"),
        (0x0416, "português"),
        (0x0418, "română"),
        (0x041b, "slovenčina"),
        (0x0424, "slovenščina"),
        (0x041d, "svenska"),
        (0, "english"),
    ];
    for (langid, expected) in table {
        assert_eq!(language_for_langid(langid), expected, "{langid:#06x}");
        assert_eq!(
            Translator::with_detector("system", || language_for_langid(langid)).language(),
            expected
        );
    }
    // Un idioma que no es de la UE (japonés) usa inglés.
    assert_eq!(language_for_langid(0x0411), "english");
}

#[test]
fn la_tabla_de_langid_cubre_los_24_idiomas() {
    let mut found: Vec<&str> = (0_u16..0x400).map(language_for_langid).collect();
    found.sort_unstable();
    found.dedup();
    let mut expected = LANGUAGES.to_vec();
    expected.sort_unstable();
    assert_eq!(found, expected);
}

#[test]
fn la_deteccion_real_da_un_idioma_admitido() {
    assert!(LANGUAGES.contains(&system_language()));
    assert!(LANGUAGES.contains(&Translator::default().language()));
}

#[test]
fn la_configuracion_guardada_decide_el_idioma() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let detect = || "español";
    assert_eq!(
        Translator::from_layout_with_detector(&layout, detect)
            .unwrap()
            .language(),
        "español"
    );
    for language in LANGUAGES {
        Configuration::global(&layout)
            .unwrap()
            .save([("mito.language", language)])
            .unwrap();
        assert_eq!(
            Translator::from_layout_with_detector(&layout, || "english")
                .unwrap()
                .language(),
            language
        );
    }
    Configuration::global(&layout)
        .unwrap()
        .save([("mito.language", "system")])
        .unwrap();
    assert_eq!(
        Translator::from_layout_with_detector(&layout, detect)
            .unwrap()
            .language(),
        "español"
    );
    let before = fs::read(layout.config_file()).unwrap();
    for invalid in ["french", "german", "italian", "", "../english", "ENGLISH"] {
        assert!(
            Configuration::global(&layout)
                .unwrap()
                .save([("mito.language", invalid)])
                .is_err(),
            "{invalid}"
        );
        assert_eq!(fs::read(layout.config_file()).unwrap(), before);
    }
    fs::write(layout.config_file(), "\u{feff}mito.language=unknown\n").unwrap();
    assert_eq!(
        Translator::from_layout_with_detector(&layout, detect)
            .unwrap()
            .language(),
        "english"
    );
}

#[test]
fn la_seleccion_manual_gana_y_no_consulta_el_sistema() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let calls = Cell::new(0);
    let detect = || {
        calls.set(calls.get() + 1);
        "english"
    };
    // Sin archivo se detecta y no se escribe nada.
    assert_eq!(
        Translator::from_layout_with_detector(&layout, detect)
            .unwrap()
            .language(),
        "english"
    );
    assert_eq!(calls.get(), 1);
    assert!(!layout.config_file().exists());
    fs::write(layout.config_file(), "mito.language=español\n").unwrap();
    assert_eq!(
        Translator::from_layout_with_detector(&layout, detect)
            .unwrap()
            .language(),
        "español"
    );
    fs::write(layout.config_file(), "mito.language=english\n").unwrap();
    assert_eq!(
        Translator::from_layout_with_detector(&layout, || "español")
            .unwrap()
            .language(),
        "english"
    );
    assert_eq!(
        calls.get(),
        1,
        "con valor manual no se llama a la detección"
    );
}
