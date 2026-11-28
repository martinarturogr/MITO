//! Tablas de `getproxies`, `proxy_open` y las exclusiones. Ninguna lee el entorno ni el
//! registro reales.

use std::cell::Cell;

use super::*;

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn url(text: &str) -> reqwest::Url {
    reqwest::Url::parse(text).unwrap()
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "abrevia los literales Some(String) de las tablas"
)]
fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn el_entorno_usa_nombres_en_mayusculas_y_ignora_los_vacios() {
    let proxies = Proxies::from_environment(&env(&[
        ("http_proxy", "http://a:1"),
        ("HTTPS_PROXY", "http://b:2"),
        ("No_Proxy", "x.test"),
        ("FTP_PROXY", "http://f:3"),
        ("PATH", "C:\\x"),
    ]));
    assert_eq!(proxies.http, some("http://a:1"));
    assert_eq!(proxies.https, some("http://b:2"));
    assert_eq!(proxies.no_proxy, some("x.test"));
    assert_eq!(proxies.registry_override, None);

    let empty = Proxies::from_environment(&env(&[("HTTP_PROXY", ""), ("https_proxy", "")]));
    assert_eq!(empty, Proxies::default());
}

#[test]
fn con_dos_nombres_que_solo_difieren_en_mayusculas_gana_el_ultimo() {
    let proxies = Proxies::from_environment(&env(&[
        ("http_proxy", "http://old:1"),
        ("HTTP_PROXY", "http://new:2"),
    ]));
    assert_eq!(proxies.http, some("http://new:2"));
}

#[test]
fn request_method_descarta_http_pero_no_https() {
    let proxies = Proxies::from_environment(&env(&[
        ("REQUEST_METHOD", "GET"),
        ("HTTP_PROXY", "http://a:1"),
        ("HTTPS_PROXY", "http://b:2"),
    ]));
    assert_eq!(proxies.http, None);
    assert_eq!(proxies.https, some("http://b:2"));
}

#[test]
fn el_sistema_usa_el_registro_solo_si_el_entorno_no_define_proxies() {
    let calls = Cell::new(0);
    let registry = || {
        calls.set(calls.get() + 1);
        RegistrySettings {
            enable: true,
            server: Some("reg:80".to_owned()),
            proxy_override: "<local>".to_owned(),
        }
    };
    let from_env = Proxies::system_with(&env(&[("NO_PROXY", "solo")]), &registry);
    assert_eq!(
        calls.get(),
        0,
        "NO_PROXY solo basta para ignorar el registro"
    );
    assert_eq!(from_env.http, None);
    assert_eq!(from_env.no_proxy, some("solo"));

    let from_registry = Proxies::system_with(&env(&[("HTTP_PROXY", "")]), &registry);
    assert_eq!(calls.get(), 1);
    assert_eq!(from_registry.http, some("http://reg:80"));
    assert_eq!(from_registry.registry_override, some("<local>"));
}

#[test]
fn el_registro_acepta_un_servidor_unico_o_una_lista_por_protocolo() {
    let single = Proxies::from_registry(true, "proxy.corp:8080", "");
    assert_eq!(single.http, some("http://proxy.corp:8080"));
    assert_eq!(single.https, some("http://proxy.corp:8080"));
    assert_eq!(single.registry_override, None);

    let split = Proxies::from_registry(true, "http=a:1;https=https://b:2;ftp=c:3", "x;y");
    assert_eq!(split.http, some("http://a:1"));
    assert_eq!(split.https, some("https://b:2"));
    assert_eq!(split.registry_override, some("x;y"));

    assert_eq!(
        Proxies::from_registry(false, "proxy:1", "<local>"),
        Proxies::default()
    );
}

#[test]
fn el_registro_rellena_http_y_https_con_socks_y_tolera_entradas_rotas() {
    let socks = Proxies::from_registry(true, "socks=s:1080;http=h:80", "");
    assert_eq!(socks.http, some("http://h:80"));
    assert_eq!(socks.https, some("socks4://s:1080"));

    // `p.split('=', 1)` falla en la segunda entrada: se conserva lo ya leído.
    let broken = Proxies::from_registry(true, "http=a:1;roto;https=b:2", "");
    assert_eq!(broken.http, some("http://a:1"));
    assert_eq!(broken.https, None);

    // Un servidor vacío con el proxy activo da la dirección vacía `http://` del original.
    assert_eq!(Proxies::from_registry(true, "", "").http, some("http://"));
}

#[test]
fn el_entorno_de_opener_sustituye_al_sistema_y_el_vacio_lo_anula() {
    let system = Proxies::from_registry(true, "reg:80", "<local>");
    let result = system.overridden(&env(&[
        ("HTTP_PROXY", "http://cfg:8080"),
        ("https_proxy", ""),
        ("OTRA", "x"),
    ]));
    assert_eq!(result.http, some("http://cfg:8080"));
    assert_eq!(result.https, some(""));
    assert_eq!(result.no_proxy, None);
    assert_eq!(result.registry_override, some("<local>"));
    assert_eq!(result.proxy_for(&url("https://example.test/")), None);
}

#[test]
fn proxy_for_distingue_esquema_entrada_vacia_y_ausente() {
    let proxies = Proxies {
        http: some("http://p:1"),
        https: some(""),
        ..Proxies::default()
    };
    assert_eq!(
        proxies.proxy_for(&url("http://a.test/")),
        some("http://p:1")
    );
    assert_eq!(proxies.proxy_for(&url("https://a.test/")), None);
    assert_eq!(Proxies::default().proxy_for(&url("http://a.test/")), None);
    assert_eq!(proxies.proxy_for(&url("ftp://a.test/")), None);
}

#[test]
fn proxy_for_normaliza_la_direccion_como_urllib() {
    let table = [
        ("http://p:1", Some("http://p:1")),
        ("https://p:1", Some("http://p:1")),
        ("HTTPS://p:1", Some("http://p:1")),
        ("p:8080", Some("http://p:8080")),
        ("p", Some("http://p")),
        ("http://u:s@p:1", Some("http://u:s@p:1")),
        ("socks5://s:1", Some("socks5://s:1")),
        ("http:/p", None),
    ];
    for (input, expected) in table {
        let proxies = Proxies {
            http: some(input),
            ..Proxies::default()
        };
        assert_eq!(
            proxies.proxy_for(&url("http://a.test/")),
            expected.map(str::to_owned),
            "{input}"
        );
    }
}

#[test]
fn no_proxy_excluye_hosts_y_subdominios() {
    let table = [
        ("localhost", "http://localhost/", true),
        ("localhost", "http://LOCALHOST:8080/", true),
        ("corp.test", "http://a.corp.test/", true),
        (".corp.test", "http://a.corp.test/", true),
        (".corp.test", "http://corp.test/", true),
        ("corp.test", "http://xcorp.test/", false),
        ("a, b ,corp.test", "http://x.corp.test/", true),
        ("*", "http://cualquiera.test/", true),
        ("", "http://a.test/", false),
        (",,", "http://a.test/", false),
        ("127.0.0.1", "http://127.0.0.1:9/", true),
        ("a.test:8080", "http://a.test:8080/", true),
        // F5: `*.dominio` equivale a `.dominio`; los demás comodines no existen.
        ("*.corp.test", "http://a.corp.test/", true),
        ("*.corp.test", "http://corp.test/", true),
        ("10.*", "http://10.1.2.3/", false),
        ("*corp.test", "http://a.corp.test/", false),
    ];
    for (list, address, bypass) in table {
        let proxies = Proxies {
            http: some("http://p:1"),
            https: some("http://p:1"),
            no_proxy: some(list),
            ..Proxies::default()
        };
        assert_eq!(
            proxies.proxy_for(&url(address)).is_none(),
            bypass,
            "{list} -> {address}"
        );
    }
}

#[test]
fn el_no_proxy_del_entorno_del_sistema_se_suma_al_de_la_configuracion() {
    let proxies = Proxies::from_environment(&env(&[
        ("HTTP_PROXY", "http://p:1"),
        ("NO_PROXY", "interno.test"),
    ]))
    .overridden(&env(&[("NO_PROXY", "config.test")]));
    assert_eq!(proxies.no_proxy, some("config.test"));
    assert_eq!(proxies.proxy_for(&url("http://a.config.test/")), None);
    assert_eq!(proxies.proxy_for(&url("http://a.interno.test/")), None);
    assert_eq!(
        proxies.proxy_for(&url("http://otro.test/")),
        some("http://p:1")
    );
}

#[test]
fn el_override_del_registro_aplica_fnmatch_y_local() {
    let proxies = Proxies::from_registry(
        true,
        "p:1",
        "intranet;*.corp.test; 192.168.*;<local>;h?st;[ab].t",
    );
    let table = [
        ("http://intranet/", true),
        ("http://x.corp.test/", true),
        ("http://corp.test/", false),
        ("http://192.168.1.5/", true),
        ("http://192.169.1.5/", false),
        ("http://sinpunto/", true),
        ("http://host.example/", false),
        ("http://hxst/", true),
        ("http://a.t/", true),
        ("http://c.t/", false),
        ("http://EXTERNO.test:8080/", false),
        ("http://INTRANET:8080/", true),
    ];
    for (address, bypass) in table {
        assert_eq!(
            proxies.proxy_for(&url(address)).is_none(),
            bypass,
            "{address}"
        );
    }
}

#[test]
fn sin_proxy_activado_el_registro_no_excluye_nada() {
    let proxies = Proxies::from_registry(false, "p:1", "<local>")
        .overridden(&env(&[("HTTP_PROXY", "http://cfg:1")]));
    assert_eq!(
        proxies.proxy_for(&url("http://sinpunto/")),
        some("http://cfg:1")
    );
}

#[test]
fn single_no_aplica_ninguna_exclusion_del_sistema() {
    // F7: la sonda usa el candidato aunque el host figure en el `ProxyOverride`.
    let single = Proxies::single("http://cand:9");
    assert_eq!(single.registry_override, None);
    assert_eq!(
        single.proxy_for(&url("http://localhost/")),
        some("http://cand:9")
    );
    assert_eq!(
        single.proxy_for(&url("https://localhost/")),
        some("http://cand:9")
    );
    assert_eq!(Proxies::single("").proxy_for(&url("http://a.test/")), None);
}

#[test]
fn split_port_y_fnmatch_siguen_a_python() {
    assert_eq!(split_port("host:80"), "host");
    assert_eq!(split_port("host:"), "host");
    assert_eq!(split_port("host"), "host");
    assert_eq!(split_port("[::1]"), "[::1]");
    assert_eq!(split_port("[::1]:80"), "[::1]");
    assert_eq!(split_port("host:abc"), "host:abc");
    assert!(fnmatch("abc", "a*c"));
    assert!(fnmatch("ac", "a*c"));
    assert!(fnmatch("a]c", "a]c"));
    assert!(fnmatch("a[c", "a[c"));
    assert!(!fnmatch("abc", "ab"));
    assert!(fnmatch("b", "[!a]"));
    assert!(!fnmatch("a", "[!a]"));
    assert!(fnmatch("-", "[a-]"));
}
