//! Partido de líneas de argumentos (`arguments.split` del original).
//!
//! El original usa `CommandLineToArgvW('mito ' + texto)` de Windows. Aquí se aplican sus
//! mismas reglas (comillas dobles, `\"`, barras invertidas antes de comillas y `""`
//! dentro de comillas) sin llamar al sistema.

/// Parte `value` como lo haría `CommandLineToArgvW`. Un texto vacío da una lista vacía.
pub fn split(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut args = Vec::new();
    let mut index = 0;
    loop {
        while chars.get(index).is_some_and(|c| matches!(c, ' ' | '\t')) {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let mut argument = String::new();
        let mut quoted = false;
        while let Some(&c) = chars.get(index) {
            match c {
                '\\' => {
                    let mut slashes = 0;
                    while chars.get(index) == Some(&'\\') {
                        slashes += 1;
                        index += 1;
                    }
                    if chars.get(index) == Some(&'"') {
                        argument.extend(std::iter::repeat_n('\\', slashes / 2));
                        if slashes % 2 == 1 {
                            argument.push('"');
                            index += 1;
                        }
                    } else {
                        argument.extend(std::iter::repeat_n('\\', slashes));
                    }
                }
                '"' => {
                    index += 1;
                    if quoted && chars.get(index) == Some(&'"') {
                        argument.push('"');
                        index += 1;
                    } else {
                        quoted = !quoted;
                    }
                }
                ' ' | '\t' if !quoted => break,
                other => {
                    argument.push(other);
                    index += 1;
                }
            }
        }
        args.push(argument);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parte_con_comillas_dobles() {
        assert_eq!(
            split("--name \"two words\" --flag"),
            ["--name", "two words", "--flag"]
        );
        assert_eq!(split(""), Vec::<String>::new());
        assert_eq!(split("   "), Vec::<String>::new());
    }

    #[test]
    fn barras_y_comillas_escapadas() {
        assert_eq!(split(r#"a\"b c"#), ["a\"b", "c"]);
        assert_eq!(split(r"C:\dir\ x"), [r"C:\dir\", "x"]);
        assert_eq!(split(r#""C:\dir\\" x"#), [r"C:\dir\", "x"]);
        assert_eq!(split(r#""a ""b"" c""#), ["a \"b\" c"]);
        assert_eq!(split(r#""""#), [""]);
    }
}
