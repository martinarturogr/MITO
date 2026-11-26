//! Partes de una ruta como las ve `pathlib` en Windows, sin consultar el disco.

/// Último componente de la ruta (`PurePath.name`): ignora separadores finales y `.`.
fn name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty() && *part != ".")
        .unwrap_or_default()
}

/// Extensión de la ruta (`PurePath.suffix`): vacía para nombres sin punto, con punto
/// inicial único o con punto final.
pub fn suffix(path: &str) -> &str {
    let name = name(path);
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => &name[index..],
        _ => "",
    }
}

/// Nombre sin extensión (`PurePath.stem`).
pub fn stem(path: &str) -> &str {
    let name = name(path);
    &name[..name.len() - suffix(path).len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_y_nombre_como_pathlib() {
        assert_eq!(suffix("a/b/tool.tar.gz"), ".gz");
        assert_eq!(suffix("tool"), "");
        assert_eq!(suffix(".zip"), "");
        assert_eq!(suffix("tool."), "");
        assert_eq!(suffix("dir\\Setup.PS1"), ".PS1");
        assert_eq!(suffix("icon.dll,0"), ".dll,0");
        assert_eq!(stem("a/b/tool.exe"), "tool");
        assert_eq!(stem("bin\\my tool.exe"), "my tool");
        assert_eq!(stem("dir/"), "dir");
        assert_eq!(stem(""), "");
        assert_eq!(stem("noext"), "noext");
    }
}
