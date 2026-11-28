//! Proxy del sistema (PAC, WPAD y manual) resuelto con PowerShell y .NET
//! (`system_proxy` de `network.py`) y sonda con las credenciales de Windows.

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Script del original (`SYSTEM_PROXY_SCRIPT`), sin cambios. Recibe la URL de destino en
/// la variable de entorno `MITO_PROXY_TARGET`.
pub const SYSTEM_PROXY_SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
$target = [Uri]$env:MITO_PROXY_TARGET
$registry = @{}
try { $registry = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' } catch { }
$result = @{ url = ''; source = 'wpad'; override = [string]$registry.ProxyOverride }
if ($registry.ProxyEnable -eq 1 -and $registry.ProxyServer) { $result.source = 'manual' }
elseif ($registry.AutoConfigURL) { $result.source = 'pac' }
$resolver = [Net.WebRequest]::GetSystemWebProxy()
if (-not $resolver.IsBypassed($target)) {
    $proxy = $resolver.GetProxy($target)
    if ($proxy -and $proxy.AbsoluteUri -ne $target.AbsoluteUri -and $proxy.Scheme -in @('http', 'https')) {
        $result.url = '{0}://{1}:{2}' -f $proxy.Scheme, $proxy.Host, $proxy.Port
    }
}
[Console]::OutputEncoding = New-Object Text.UTF8Encoding
$result | ConvertTo-Json -Compress
";

/// Ejecutor de PowerShell de [`system_proxy_with`]: script y destino; `(código, stdout)`.
pub trait PowerShell: Fn(&str, &str) -> Option<(i32, String)> {}
impl<F: Fn(&str, &str) -> Option<(i32, String)>> PowerShell for F {}

/// Ejecutor de [`probe_with_windows_credentials_with`]: script y variables de entorno.
pub trait PowerShellEnv: Fn(&str, &[(String, String)]) -> Option<(i32, String)> {}
impl<F: Fn(&str, &[(String, String)]) -> Option<(i32, String)>> PowerShellEnv for F {}

/// Tiempo máximo de PowerShell (`timeout=30`).
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(30);

/// Proxy que usaría Windows para un destino.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemProxy {
    /// URL del proxy (`http://host:puerto`).
    pub url: String,
    /// Origen de la configuración: `manual`, `pac` o `wpad`.
    pub source: String,
    /// `no_proxy` deducido de `ProxyOverride`.
    pub no_proxy: String,
}

/// `ProxyOverride` de Windows a `no_proxy` (`no_proxy_from_override`): solo nombres
/// exactos y sufijos; `<local>` y los comodines no representables se descartan.
pub fn no_proxy_from_override(proxy_override: &str) -> String {
    let mut items: Vec<String> = ["localhost", "127.0.0.1", "::1"]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    for entry in proxy_override.split(|c: char| c == ';' || c == ',' || c.is_whitespace()) {
        let mut entry = entry.trim().to_lowercase();
        if entry.is_empty() || entry == "<local>" {
            continue;
        }
        if entry.starts_with("*.") {
            entry.remove(0);
        }
        if entry.contains(['*', '/']) || items.contains(&entry) {
            continue;
        }
        items.push(entry);
    }
    items.join(",")
}

/// Proxy que usaría Windows para `target` (`system_proxy`), con PowerShell.
///
/// `None` si PowerShell no arranca, tarda más de 30 s, falla o no devuelve un proxy
/// HTTP/HTTPS válido (ruta directa).
pub fn system_proxy(target: &str) -> Option<SystemProxy> {
    system_proxy_with(target, &|script, target| {
        run_powershell(
            &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"],
            script,
            &[("MITO_PROXY_TARGET", target)],
            POWERSHELL_TIMEOUT,
        )
    })
}

/// Igual que [`system_proxy`], con el ejecutor inyectado: recibe el script y el valor de
/// `MITO_PROXY_TARGET`, y devuelve `(código, stdout)` o `None` si no arrancó o superó 30 s.
pub fn system_proxy_with(target: &str, run: &dyn PowerShell) -> Option<SystemProxy> {
    let (code, output) = run(SYSTEM_PROXY_SCRIPT, target)?;
    let data: serde_json::Value = if code == 0 && !output.trim().is_empty() {
        serde_json::from_str(&output).ok()?
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };
    let data = data.as_object()?;
    let text = |key: &str| data.get(key).and_then(serde_json::Value::as_str);
    let url = text("url").unwrap_or_default();
    super::validate_url(url).ok()?;
    Some(SystemProxy {
        url: url.to_owned(),
        source: text("source").unwrap_or("wpad").to_owned(),
        no_proxy: no_proxy_from_override(text("override").unwrap_or_default()),
    })
}

/// Script de la sonda con las credenciales de Windows. Recibe todo por variables de
/// entorno (`MITO_PROBE_URL`, `MITO_PROBE_PROXY`, `MITO_PROBE_TIMEOUT`): nada se
/// interpola en el texto.
const CREDENTIALS_PROBE_SCRIPT: &str = r"$ErrorActionPreference = 'Stop'
$arguments = @{ Uri = $env:MITO_PROBE_URL; UseBasicParsing = $true; MaximumRedirection = 0; TimeoutSec = [int]$env:MITO_PROBE_TIMEOUT }
if ($env:MITO_PROBE_PROXY) { $arguments.Proxy = $env:MITO_PROBE_PROXY; $arguments.ProxyUseDefaultCredentials = $true }
try {
    [int](Invoke-WebRequest @arguments).StatusCode
} catch {
    if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { 0 }
}
";

/// Solo bootstrap: ¿responde `expected` la sonda con las credenciales de Windows
/// (NTLM/Kerberos)? Se hace con PowerShell (`-ProxyUseDefaultCredentials`), porque
/// `reqwest` no las admite.
pub fn probe_with_windows_credentials(
    url: &str,
    proxy: &str,
    timeout: Duration,
    expected: u16,
) -> bool {
    probe_with_windows_credentials_with(url, proxy, timeout, expected, &|script, env| {
        let borrowed: Vec<(&str, &str)> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        run_powershell(
            &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"],
            script,
            &borrowed,
            timeout + Duration::from_secs(10),
        )
    })
}

/// Igual que [`probe_with_windows_credentials`], con el ejecutor inyectado: recibe el
/// script y las variables de entorno, y devuelve `(código, stdout)`.
pub fn probe_with_windows_credentials_with(
    url: &str,
    proxy: &str,
    timeout: Duration,
    expected: u16,
    run: &dyn PowerShellEnv,
) -> bool {
    let seconds = timeout.as_secs().max(1);
    let env = [
        ("MITO_PROBE_URL".to_owned(), url.to_owned()),
        ("MITO_PROBE_PROXY".to_owned(), proxy.to_owned()),
        ("MITO_PROBE_TIMEOUT".to_owned(), seconds.to_string()),
    ];
    let Some((0, output)) = run(CREDENTIALS_PROBE_SCRIPT, &env) else {
        return false;
    };
    output
        .lines()
        .rev()
        .find_map(|line| line.trim().parse::<u16>().ok())
        .is_some_and(|status| status == expected)
}

/// Ejecuta `powershell.exe` con el script por la entrada estándar, sin ventana, y
/// devuelve `(código, stdout)`; `None` si no arranca o supera `timeout`.
fn run_powershell(
    arguments: &[&str],
    script: &str,
    env: &[(&str, &str)],
    timeout: Duration,
) -> Option<(i32, String)> {
    let mut command = Command::new("powershell.exe");
    command
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (name, value) in env {
        command.env(name, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        /// `CREATE_NO_WINDOW`.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let script = script.to_owned();
    let writer = std::thread::spawn(move || {
        // Si PowerShell termina antes de leer todo el script, no hay nada que añadir.
        let _ = stdin.write_all(script.as_bytes());
    });
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                // Se abandona: un fallo al matar el proceso no cambia el resultado.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let _ = writer.join();
    let bytes = reader.join().ok()?;
    Some((
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

#[cfg(test)]
mod tests;
