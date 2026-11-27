//! `subprocess.list2cmdline` del original: reglas de entrecomillado de MSVCRT.

/// Une `arguments` en una línea de comandos de Windows igual que `list2cmdline`.
///
/// Un argumento se entrecomilla si está vacío o contiene espacios o tabuladores; las
/// comillas internas se escapan con barra invertida y las barras previas se duplican.
pub fn list2cmdline<S: AsRef<str>>(arguments: &[S]) -> String {
    let mut result = String::new();
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            result.push(' ');
        }
        let argument = argument.as_ref();
        let quote = argument.is_empty() || argument.contains([' ', '\t']);
        if quote {
            result.push('"');
        }
        let mut backslashes = 0_usize;
        for character in argument.chars() {
            match character {
                '\\' => backslashes += 1,
                '"' => {
                    result.push_str(&"\\".repeat(backslashes * 2));
                    backslashes = 0;
                    result.push_str("\\\"");
                }
                other => {
                    result.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                    result.push(other);
                }
            }
        }
        // `list2cmdline` conserva las barras finales (se duplican antes de la comilla
        // de cierre solo si hay comillas; el original las repite tal cual).
        result.push_str(&"\\".repeat(backslashes));
        if quote {
            result.push_str(&"\\".repeat(backslashes));
            result.push('"');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::list2cmdline;

    #[test]
    fn replica_las_reglas_de_python() {
        assert_eq!(list2cmdline(&["a", "b c", ""]), r#"a "b c" """#);
        assert_eq!(
            list2cmdline(&[r"C:\MITO\core\bin\mito.exe", "ui"]),
            r"C:\MITO\core\bin\mito.exe ui"
        );
        assert_eq!(
            list2cmdline(&[r"C:\Mi carpeta\mito.exe", "ui"]),
            r#""C:\Mi carpeta\mito.exe" ui"#
        );
        assert_eq!(list2cmdline(&[r#"say "hi""#]), r#""say \"hi\"""#);
        assert_eq!(list2cmdline(&[r#"a\"b"#]), r#"a\\\"b"#);
        assert_eq!(list2cmdline(&[r"dir\ x\"]), r#""dir\ x\\""#);
        assert_eq!(list2cmdline::<&str>(&[]), "");
    }
}
