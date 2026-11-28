//! Valores por defecto, rangos y mensajes de las claves de red.
#![allow(clippy::assert_is_empty)]

use mito_core::config::Values;

use super::*;

fn values(pairs: &[(&str, &str)]) -> Values {
    pairs.iter().copied().collect()
}

#[test]
fn sin_claves_valen_los_valores_por_defecto_del_original() {
    let config = NetworkConfig::from_values(&Values::new()).unwrap();
    assert_eq!(config.timeout, Duration::from_secs(4));
    assert_eq!(config.check_url, "https://www.gstatic.com/generate_204");
    assert_eq!(config.expected, 204);
    assert_eq!(config.auth_url, "");
    assert!(!config.enabled);
    assert_eq!(config.username_field, "username");
    assert_eq!(config.password_field, "password");
    assert!(config.require_https);
    assert!(config.form_fields.is_empty());
}

#[test]
fn lee_todas_las_claves() {
    let config = NetworkConfig::from_values(&values(&[
        ("network.timeout_seconds", "1.5"),
        ("network.check_url", "http://check.test/x"),
        ("network.check_status", "200"),
        ("enable_proxy_authentication", "TRUE"),
        ("proxy_authentication_url", "http://portal.test/login"),
        ("proxy_authentication_username_field", "user"),
        ("proxy_authentication_password_field", "pass"),
        ("proxy_authentication_require_https", "False"),
        ("proxy_authentication_form_field.extra", "value"),
        ("proxy_authentication_form_field.otro", "2"),
    ]))
    .unwrap();
    assert_eq!(config.timeout, Duration::from_millis(1500));
    assert_eq!(config.check_url, "http://check.test/x");
    assert_eq!(config.expected, 200);
    assert!(config.enabled);
    assert_eq!(config.username_field, "user");
    assert_eq!(config.password_field, "pass");
    assert!(!config.require_https);
    assert_eq!(
        config.form_fields,
        [
            ("extra".to_owned(), "value".to_owned()),
            ("otro".to_owned(), "2".to_owned())
        ]
    );
}

#[test]
fn la_autenticacion_exige_la_url_del_portal_y_solo_true_la_activa() {
    let enabled =
        |pairs: &[(&str, &str)]| NetworkConfig::from_values(&values(pairs)).unwrap().enabled;
    assert!(!enabled(&[("enable_proxy_authentication", "true")]));
    assert!(!enabled(&[("proxy_authentication_url", "http://p.test/")]));
    assert!(!enabled(&[
        ("enable_proxy_authentication", "yes"),
        ("proxy_authentication_url", "http://p.test/")
    ]));
    assert!(enabled(&[
        ("enable_proxy_authentication", "true"),
        ("proxy_authentication_url", "http://p.test/")
    ]));
}

#[test]
fn solo_false_desactiva_la_exigencia_de_https() {
    let require = |value: &str| {
        NetworkConfig::from_values(&values(&[("proxy_authentication_require_https", value)]))
            .unwrap()
            .require_https
    };
    assert!(require("true"));
    assert!(require("no"));
    assert!(require(""));
    assert!(!require("false"));
    assert!(!require("FALSE"));
}

fn message(pairs: &[(&str, &str)]) -> String {
    NetworkConfig::from_values(&values(pairs))
        .unwrap_err()
        .to_string()
}

#[test]
fn los_errores_son_de_dominio_con_los_mensajes_del_original() {
    for bad in ["0", "-1", "61", "nan", "inf", "-inf"] {
        assert_eq!(
            message(&[("network.timeout_seconds", bad)]),
            "network.timeout_seconds debe estar entre 0 y 60",
            "{bad}"
        );
    }
    for bad in ["abc", "", "1__0", "_1", "4s"] {
        assert_eq!(
            message(&[("network.timeout_seconds", bad)]),
            "network.timeout_seconds: introduce un número mayor que 0 y hasta 60",
            "{bad:?}"
        );
    }
    for bad in ["abc", "", "99", "600", "-204", "2.5"] {
        assert_eq!(
            message(&[("network.check_status", bad)]),
            "network.check_status: introduce un código HTTP entre 100 y 599",
            "{bad:?}"
        );
    }
    for key in ["network.check_url", "proxy_authentication_url"] {
        for bad in ["ftp://x", "http://u:p@x", "x"] {
            assert_eq!(
                message(&[(key, bad)]),
                "Configura una URL HTTP/HTTPS sin credenciales",
                "{key} {bad}"
            );
        }
    }
    // `network.check_url` se valida siempre, también vacía.
    assert_eq!(
        message(&[("network.check_url", "")]),
        "Configura una URL HTTP/HTTPS sin credenciales"
    );
}

#[test]
fn el_timeout_admite_formas_de_float_de_python() {
    let timeout = |value: &str| {
        NetworkConfig::from_values(&values(&[("network.timeout_seconds", value)]))
            .unwrap()
            .timeout
    };
    assert_eq!(timeout("1e0"), Duration::from_secs(1));
    assert_eq!(timeout("6_0"), Duration::from_secs(60));
    assert_eq!(timeout(".5"), Duration::from_millis(500));
}

#[test]
fn el_sondeo_sigue_la_tabla_de_la_interfaz() {
    let table: [(Option<&str>, Option<u64>); 12] = [
        (None, Some(20)),
        (Some("20"), Some(20)),
        (Some("3"), Some(5)),
        (Some("5"), Some(5)),
        (Some("0"), None),
        (Some("-1"), None),
        (Some("inf"), None),
        (Some("1e400"), None),
        (Some("nan"), None),
        (Some("abc"), Some(20)),
        (Some(""), Some(20)),
        (Some("12,5"), Some(20)),
    ];
    for (value, expected) in table {
        let mut config = Values::new();
        if let Some(value) = value {
            config.insert("network.poll_seconds", value);
        }
        assert_eq!(
            poll_interval(&config),
            expected.map(Duration::from_secs),
            "{value:?}"
        );
    }
    let config = values(&[("network.poll_seconds", "12")]);
    assert_eq!(poll_interval(&config), Some(Duration::from_secs(12)));
    let huge = values(&[("network.poll_seconds", "1e300")]);
    assert!(poll_interval(&huge).is_some());
}

fn env(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn las_claves_de_proxy_sustituyen_al_entorno_y_el_vacio_se_conserva() {
    let config = values(&[
        ("http_proxy", "http://proxy.example:8080"),
        ("https_proxy", ""),
        ("no_proxy", "localhost"),
    ]);
    let mut result = env(&[("http_proxy", "old"), ("HTTPS_PROXY", "old"), ("PATH", "x")]);
    proxy_environment(&config, &mut result).unwrap();
    assert_eq!(result["HTTP_PROXY"], "http://proxy.example:8080");
    assert_eq!(result["HTTPS_PROXY"], "");
    assert_eq!(result["NO_PROXY"], "localhost");
    assert!(!result.contains_key("http_proxy"));
    assert_eq!(result["PATH"], "x");
}

#[test]
fn sin_claves_de_proxy_el_entorno_no_cambia() {
    let mut result = env(&[("http_proxy", "old")]);
    proxy_environment(&Values::new(), &mut result).unwrap();
    assert_eq!(result, env(&[("http_proxy", "old")]));
}

#[test]
fn un_proxy_con_credenciales_o_esquema_ajeno_es_un_error_antes_de_tocar_el_entorno() {
    for bad in ["http://user:secret@proxy.example", "ftp://p", "proxy:8080"] {
        let mut result = env(&[("HTTP_PROXY", "old")]);
        let error = proxy_environment(&values(&[("http_proxy", bad)]), &mut result).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Configura una URL HTTP/HTTPS sin credenciales"
        );
        assert_eq!(result, env(&[("HTTP_PROXY", "old")]));
    }
    // `no_proxy` es texto libre.
    let mut result = Env::new();
    proxy_environment(&values(&[("no_proxy", "cualquier cosa")]), &mut result).unwrap();
    assert_eq!(result["NO_PROXY"], "cualquier cosa");
}
