//! Proxy del sistema con un ejecutor falso: ningún test arranca PowerShell.

use std::cell::RefCell;

use super::*;

#[test]
fn no_proxy_desde_el_override_de_windows() {
    assert_eq!(
        no_proxy_from_override("<local>;*.corp.example;intranet;10.*;a/b"),
        "localhost,127.0.0.1,::1,.corp.example,intranet"
    );
}

#[test]
fn no_proxy_desde_el_override_casos_limite() {
    let table = [
        ("", "localhost,127.0.0.1,::1"),
        ("<local>", "localhost,127.0.0.1,::1"),
        (
            "  A.Test ,B.TEST;  c.test ",
            "localhost,127.0.0.1,::1,a.test,b.test,c.test",
        ),
        ("localhost;127.0.0.1;::1", "localhost,127.0.0.1,::1"),
        (
            "*.x.test;.x.test;x.test;x.test",
            "localhost,127.0.0.1,::1,.x.test,x.test",
        ),
        ("*;*.*;a*b;*.a*", "localhost,127.0.0.1,::1"),
        ("a\tb\nc", "localhost,127.0.0.1,::1,a,b,c"),
    ];
    for (input, expected) in table {
        assert_eq!(no_proxy_from_override(input), expected, "{input:?}");
    }
}

fn answer(code: i32, output: &str) -> impl Fn(&str, &str) -> Option<(i32, String)> + use<> {
    let output = output.to_owned();
    move |_, _| Some((code, output.clone()))
}

#[test]
fn el_ejecutor_recibe_el_script_y_el_destino() {
    let seen = RefCell::new(Vec::new());
    let run = |script: &str, target: &str| {
        seen.borrow_mut()
            .push((script.to_owned(), target.to_owned()));
        None
    };
    assert_eq!(system_proxy_with("http://check.test/x", &run), None);
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, SYSTEM_PROXY_SCRIPT);
    assert_eq!(seen[0].1, "http://check.test/x");
}

#[test]
fn un_proxy_valido_se_devuelve_con_su_origen_y_su_no_proxy() {
    let json =
        r#"{"url":"http://10.1.2.3:8080","source":"pac","override":"<local>;*.corp.example"}"#;
    assert_eq!(
        system_proxy_with("http://t/", &answer(0, json)),
        Some(SystemProxy {
            url: "http://10.1.2.3:8080".to_owned(),
            source: "pac".to_owned(),
            no_proxy: "localhost,127.0.0.1,::1,.corp.example".to_owned(),
        })
    );
    // Sin `source` ni `override` se usan los valores por defecto del original.
    let minimal =
        system_proxy_with("http://t/", &answer(0, r#"{"url":"https://p:3128"}"#)).unwrap();
    assert_eq!(minimal.source, "wpad");
    assert_eq!(minimal.no_proxy, "localhost,127.0.0.1,::1");
}

#[test]
fn todo_lo_que_no_es_un_proxy_http_valido_es_ruta_directa() {
    let table: [(i32, &str); 9] = [
        (0, r#"{"url":"","source":"wpad","override":""}"#),
        (0, r#"{"source":"wpad"}"#),
        (0, r#"{"url":"ftp://p:21"}"#),
        (0, r#"{"url":"http://u:s@p:1"}"#),
        (0, r#"{"url":null}"#),
        (0, "no es json"),
        (0, "[]"),
        (1, r#"{"url":"http://p:1"}"#),
        (0, ""),
    ];
    for (code, output) in table {
        assert_eq!(
            system_proxy_with("http://t/", &answer(code, output)),
            None,
            "{code} {output}"
        );
    }
}

#[test]
fn si_powershell_no_arranca_o_se_cuelga_no_hay_proxy() {
    assert_eq!(system_proxy_with("http://t/", &|_, _| None), None);
}

#[test]
fn la_sonda_con_credenciales_de_windows_lee_el_ultimo_estado() {
    let seen = RefCell::new(Vec::new());
    let run = |script: &str, env: &[(String, String)]| {
        seen.borrow_mut().push((script.to_owned(), env.to_vec()));
        Some((0, "aviso\n204\n".to_owned()))
    };
    let timeout = Duration::from_secs(4);
    assert!(probe_with_windows_credentials_with(
        "http://check.test/x",
        "http://p:8080",
        timeout,
        204,
        &run
    ));
    assert!(!probe_with_windows_credentials_with(
        "http://check.test/x",
        "",
        timeout,
        200,
        &run
    ));
    let seen = seen.into_inner();
    assert!(seen[0].0.contains("ProxyUseDefaultCredentials"));
    assert!(seen[0].0.contains("MaximumRedirection = 0"));
    assert!(seen[0].0.contains("UseBasicParsing"));
    let env = |at: usize, name: &str| {
        seen[at]
            .1
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(env(0, "MITO_PROBE_URL"), Some("http://check.test/x"));
    assert_eq!(env(0, "MITO_PROBE_PROXY"), Some("http://p:8080"));
    assert_eq!(env(0, "MITO_PROBE_TIMEOUT"), Some("4"));
    assert_eq!(env(1, "MITO_PROBE_PROXY"), Some(""));
}

#[test]
fn la_sonda_con_credenciales_falla_si_powershell_falla_o_no_responde_un_numero() {
    let timeout = Duration::from_millis(100);
    let probe = |outcome: Option<(i32, String)>| {
        probe_with_windows_credentials_with("http://t/", "http://p:1", timeout, 204, &|_, _| {
            outcome.clone()
        })
    };
    assert!(!probe(None));
    assert!(!probe(Some((1, "204".to_owned()))));
    assert!(!probe(Some((0, "texto".to_owned()))));
    assert!(!probe(Some((0, "407".to_owned()))));
    assert!(probe(Some((0, "204".to_owned()))));
}

#[test]
fn el_script_es_el_literal_del_original() {
    assert!(SYSTEM_PROXY_SCRIPT.starts_with("\n$ErrorActionPreference = 'Stop'\n"));
    assert!(SYSTEM_PROXY_SCRIPT.contains("[Net.WebRequest]::GetSystemWebProxy()"));
    assert!(SYSTEM_PROXY_SCRIPT.ends_with("$result | ConvertTo-Json -Compress\n"));
    assert_eq!(SYSTEM_PROXY_SCRIPT.lines().count(), 17);
}
