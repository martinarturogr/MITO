#![allow(clippy::assert_is_empty)]

//! Formularios del portal: casos del original, atributos, entidades y robustez.

use proptest::prelude::*;

use super::*;

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn el_formulario_del_original() {
    let html = r#"<form action="/"><input type="hidden" name="magic" value="1"><input name="username"><input name="password" type="password"></form>"#;
    let forms = portal_forms(html, "password");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].action, "/");
    assert_eq!(
        forms[0].fields,
        fields(&[("magic", "1"), ("username", ""), ("password", "")])
    );
    assert_eq!(forms[0].user_field, "username");
    assert_eq!(forms[0].password_field, "password");
}

#[test]
fn el_formulario_del_portal_de_diagnostico() {
    let html = r#"<form action="/login"><input name="token" type="hidden" value="x"><input name="user" type="text"><input name="pass" type="password"></form>"#;
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.user_field, "user");
    assert_eq!(form.password_field, "pass");
    assert_eq!(
        form.fields,
        fields(&[("token", "x"), ("user", ""), ("pass", "")])
    );
}

#[test]
fn los_formularios_sin_contrasena_no_se_guardan() {
    let html = r#"<form action="/a"><input name="q"></form><form action="/b"><input name="pwd" type="password"></form>"#;
    let forms = portal_forms(html, "password");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].action, "/b");
    assert_eq!(forms[0].user_field, "");
}

#[test]
fn el_campo_configurado_cuenta_como_contrasena_aunque_no_sea_de_ese_tipo() {
    let html = r#"<form><input name="u"><input name="clave" value="publicada"></form>"#;
    assert!(portal_forms(html, "password").is_empty());
    let forms = portal_forms(html, "clave");
    assert_eq!(forms[0].password_field, "clave");
    // F9: el valor publicado de un campo de contraseña no se envía.
    assert_eq!(forms[0].fields, fields(&[("u", ""), ("clave", "")]));
}

#[test]
fn f9_el_valor_de_un_input_password_se_descarta() {
    let html =
        r#"<form><input name="otro" type="PASSWORD" value="secreta"><input name="u"></form>"#;
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.fields, fields(&[("otro", ""), ("u", "")]));
    assert_eq!(form.password_field, "otro");
}

#[test]
fn el_usuario_es_el_primer_text_o_email() {
    let html = r#"<form><input type="hidden" name="h"><input type="tel" name="t"><input type="email" name="mail"><input name="otro"><input type="password" name="p"></form>"#;
    assert_eq!(portal_forms(html, "password")[0].user_field, "mail");
    let html = r#"<form><input name="a" type="TEXT"><input name="b"><input type="password" name="p"></form>"#;
    assert_eq!(portal_forms(html, "password")[0].user_field, "a");
}

#[test]
fn los_atributos_admiten_todas_las_formas() {
    let html = concat!(
        "<FORM ACTION='/a?x=1&amp;y=2'>",
        "<INPUT NAME=\"d\" VALUE=\"doble\">",
        "<input name='s' value='simple'>",
        "<input name=u value=sinComillas>",
        "<input name=v value>",
        "<input name=\"w\" value=\"\">",
        "<input name=p type=password>",
        "<input value=\"huerfano\">",
        "<input name=\"\" value=\"vacio\">",
        "</FORM>"
    );
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.action, "/a?x=1&y=2");
    assert_eq!(
        form.fields,
        fields(&[
            ("d", "doble"),
            ("s", "simple"),
            ("u", "sinComillas"),
            // F8: un atributo sin valor cuenta como "".
            ("v", ""),
            ("w", ""),
            ("p", ""),
        ])
    );
}

#[test]
fn las_entidades_se_decodifican_en_los_atributos() {
    let html = r#"<form action="/a?x=1&amp;y=2"><input name="csrf" type="hidden" value="t&amp;k"><input name="n&#x41;" value="&lt;&eacute;&#66;&quot;"><input name="p" type="password"></form>"#;
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.action, "/a?x=1&y=2");
    assert_eq!(
        form.fields,
        fields(&[("csrf", "t&k"), ("nA", "<\u{e9}B\""), ("p", "")])
    );
}

#[test]
fn un_input_repetido_conserva_la_posicion_y_el_ultimo_valor() {
    let html = r#"<form><input name="a" value="1"><input name="b" value="2"><input name="a" value="3"><input name="p" type="password"></form>"#;
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.fields, fields(&[("a", "3"), ("b", "2"), ("p", "")]));
}

#[test]
fn comentarios_y_scripts_se_ignoran() {
    let html = concat!(
        "<!-- <form action='/falso'><input name='p' type='password'></form> -->",
        "<script>var f = '<form action=\"/js\"><input name=\"p\" type=\"password\"></form>';</script>",
        "<style> <form action='/css'> </style>",
        "<textarea><form action='/area'></textarea>",
        "<!DOCTYPE html><?php echo 1 ?>",
        "<form action='/real'><input name='u'><input name='p' type='password'></form>"
    );
    let forms = portal_forms(html, "password");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].action, "/real");
}

#[test]
fn un_script_sin_cerrar_ignora_el_resto() {
    let html = "<script><form action='/x'><input name='p' type='password'></form>";
    assert!(portal_forms(html, "password").is_empty());
}

#[test]
fn un_formulario_dentro_de_otro_sustituye_al_abierto() {
    let html = r#"<form action="/a"><input name="x"><form action="/b"><input name="p" type="password"></form>"#;
    let forms = portal_forms(html, "password");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].action, "/b");
    assert_eq!(forms[0].fields, fields(&[("p", "")]));
}

#[test]
fn los_inputs_fuera_de_un_formulario_y_los_formularios_sin_cerrar_se_descartan() {
    let html =
        r#"<input name="p" type="password"><form action="/a"><input name="q" type="password">"#;
    assert!(portal_forms(html, "password").is_empty());
}

#[test]
fn un_formulario_autocerrado_se_cierra_sin_campos() {
    let html = r#"<form action="/a"/><input name="p" type="password"></form>"#;
    assert!(portal_forms(html, "password").is_empty());
}

#[test]
fn las_etiquetas_incompletas_son_texto() {
    for html in [
        "<form action=\"/a",
        "<form action=\"/a\" ",
        "<form><input name=\"p\" type=\"password",
        "<",
        "<<<",
        "</",
        "<!--",
        "<!-",
        "",
    ] {
        assert!(portal_forms(html, "password").is_empty(), "{html:?}");
    }
}

#[test]
fn las_etiquetas_y_atributos_no_distinguen_mayusculas_y_toleran_espacios() {
    let html = "<FoRm\n action = \"/a\"\t><InPuT\r\nNaMe\n=\n\"p\"\nTyPe = 'Password' /></fOrM >";
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.action, "/a");
    assert_eq!(form.fields, fields(&[("p", "")]));
}

#[test]
fn el_texto_no_ascii_se_conserva() {
    let html = "<form action=\"/ñ\"><input name=\"usuário\" value=\"ç\"><input name=\"p\" type=\"password\"></form>";
    let form = &portal_forms(html, "password")[0];
    assert_eq!(form.action, "/ñ");
    assert_eq!(form.user_field, "usuário");
    assert_eq!(form.fields[0], ("usuário".to_owned(), "ç".to_owned()));
}

proptest! {
    #[test]
    fn nunca_entra_en_panico_con_texto_arbitrario(html in "\\PC*", name in "\\PC{0,8}") {
        let _ = portal_forms(&html, &name);
    }

    #[test]
    fn nunca_entra_en_panico_con_marcado_hostil(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("<form"), Just("<input"), Just("</form>"), Just("<!--"), Just("-->"),
                Just("<script>"), Just("</script"), Just("name="), Just("type=password"),
                Just("\""), Just("'"), Just("="), Just("/"), Just(">"), Just("<"), Just(" "),
                Just("&amp;"), Just("&#x"), Just("&#"), Just("ñ"), Just("<!"), Just("</"),
            ],
            0..60,
        )
    ) {
        let html: String = parts.concat();
        for form in portal_forms(&html, "password") {
            prop_assert!(!form.password_field.is_empty());
        }
    }
}
