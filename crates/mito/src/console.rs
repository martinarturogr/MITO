//! Entrada y salida de la consola (`print` e `input` del original).

use std::cell::RefCell;
use std::io::{self, BufRead, IsTerminal as _, Write};

use zeroize::Zeroizing;

/// Colores de texto de la consola (los que usa la bienvenida del bootstrap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Cian (título).
    Cyan,
    /// Amarillo (aviso).
    Yellow,
    /// Verde (éxito).
    Green,
    /// Rojo (error).
    Red,
}

impl Color {
    fn escape(self) -> &'static str {
        match self {
            Self::Cyan => "\x1b[36m",
            Self::Yellow => "\x1b[33m",
            Self::Green => "\x1b[32m",
            Self::Red => "\x1b[31m",
        }
    }
}

/// Salidas estándar y entrada de un comando. Los métodos toman `&self` para poder
/// usarse desde los callbacks del motor.
pub struct Console {
    out: RefCell<Box<dyn Write>>,
    err: RefCell<Box<dyn Write>>,
    input: RefCell<Box<dyn BufRead>>,
    color: bool,
    /// ¿Se oculta lo que se escribe al leer un secreto? Solo en la consola real.
    hide_secrets: bool,
}

impl Console {
    /// Consola sobre `stdout`, `stderr` y `stdin` del proceso.
    pub fn stdio() -> Self {
        let mut console = Self::new(
            Box::new(io::stdout()),
            Box::new(io::stderr()),
            Box::new(io::BufReader::new(io::stdin())),
        );
        // Colores solo en una consola real que admita secuencias ANSI y sin `NO_COLOR`.
        console.color = std::env::var_os("NO_COLOR").is_none()
            && io::stdout().is_terminal()
            && mito_win::console::enable_virtual_terminal();
        console.hide_secrets = true;
        console
    }

    /// Consola con flujos propios (pruebas).
    pub fn new(out: Box<dyn Write>, err: Box<dyn Write>, input: Box<dyn BufRead>) -> Self {
        Self {
            out: RefCell::new(out),
            err: RefCell::new(err),
            input: RefCell::new(input),
            color: false,
            hide_secrets: false,
        }
    }

    /// Escribe una línea en stdout con `color` si la consola lo admite.
    pub fn colored_line(&self, text: impl AsRef<str>, color: Color) {
        if self.color {
            self.line(format!("{}{}\x1b[0m", color.escape(), text.as_ref()));
        } else {
            self.line(text);
        }
    }

    /// Fuerza los colores (pruebas).
    #[cfg(test)]
    pub fn with_color(mut self) -> Self {
        self.color = true;
        self
    }

    /// Escribe una línea en stdout. Un fallo de escritura (tubería cerrada) se ignora,
    /// como hace `print` de Python con `BrokenPipeError` al terminar.
    pub fn line(&self, text: impl AsRef<str>) {
        let mut out = self.out.borrow_mut();
        let _ = writeln!(out, "{}", text.as_ref());
        let _ = out.flush();
    }

    /// Escribe una línea en stderr.
    pub fn error_line(&self, text: impl AsRef<str>) {
        let mut err = self.err.borrow_mut();
        let _ = writeln!(err, "{}", text.as_ref());
        let _ = err.flush();
    }

    /// `input(prompt)`: escribe `prompt` sin salto de línea y lee una línea. `None` en
    /// EOF o si la lectura falla.
    pub fn read_line(&self, prompt: &str) -> Option<String> {
        {
            let mut out = self.out.borrow_mut();
            let _ = write!(out, "{prompt}");
            let _ = out.flush();
        }
        let mut line = String::new();
        match self.input.borrow_mut().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }

    /// `Read-Host -AsSecureString`: escribe `prompt` y lee una línea sin eco (en la
    /// consola real). El texto se borra de memoria al soltarse. `None` en EOF o si la
    /// lectura falla.
    pub fn read_secret(&self, prompt: &str) -> Option<Zeroizing<String>> {
        {
            let mut out = self.out.borrow_mut();
            let _ = write!(out, "{prompt}");
            let _ = out.flush();
        }
        // El guardián restaura el eco al soltarse, también si la lectura falla.
        let hidden = if self.hide_secrets {
            mito_win::console::HiddenInput::new()
        } else {
            None
        };
        let mut line = Zeroizing::new(String::new());
        let read = self.input.borrow_mut().read_line(&mut line);
        let silent = hidden.is_some();
        drop(hidden);
        if silent {
            // Sin eco no hay salto de línea visible: lo escribe la consola.
            self.line("");
        }
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let text = line.trim_end_matches(['\r', '\n']).to_owned();
                Some(Zeroizing::new(text))
            }
        }
    }

    /// Pregunta de sí o no: solo `s` (sin importar mayúsculas ni espacios) confirma.
    /// EOF cuenta como «no».
    pub fn confirm(&self, prompt: &str) -> bool {
        self.read_line(prompt)
            .is_some_and(|answer| answer.trim().to_lowercase() == "s")
    }

    /// Espera Intro sin fallar en EOF.
    pub fn pause(&self, prompt: &str) {
        let _ = self.read_line(prompt);
    }
}

#[cfg(test)]
pub mod testing {
    //! Consola en memoria para pruebas.

    use std::cell::RefCell;
    use std::io::{self, Cursor, Write};
    use std::rc::Rc;

    use super::Console;

    /// Búfer compartido que se puede inspeccionar tras ejecutar.
    #[derive(Clone, Default)]
    pub struct Shared(Rc<RefCell<Vec<u8>>>);

    impl Shared {
        /// Texto escrito hasta ahora.
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.borrow()).into_owned()
        }
    }

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Consola con `input` como stdin; devuelve también stdout y stderr.
    pub fn console(input: &str) -> (Console, Shared, Shared) {
        let (out, err) = (Shared::default(), Shared::default());
        let console = Console::new(
            Box::new(out.clone()),
            Box::new(err.clone()),
            Box::new(Cursor::new(input.as_bytes().to_vec())),
        );
        (console, out, err)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::console;

    #[test]
    fn solo_la_s_confirma() {
        for (answer, expected) in [
            ("s\n", true),
            ("  S \n", true),
            ("n\n", false),
            ("si\n", false),
            ("\n", false),
            ("", false),
        ] {
            let (console, out, _) = console(answer);
            assert_eq!(
                console.confirm("¿Continuar? [s/N] "),
                expected,
                "{answer:?}"
            );
            assert_eq!(out.text(), "¿Continuar? [s/N] ");
        }
    }

    #[test]
    fn el_secreto_se_lee_sin_saltos_de_linea_y_el_eof_es_none() {
        let (console, out, _) = console("clave con espacios \r\nresto\n");
        let secret = console.read_secret("Contraseña: ").unwrap();
        assert_eq!(secret.as_str(), "clave con espacios ");
        assert_eq!(out.text(), "Contraseña: ");
        assert!(console.read_secret("Contraseña: ").is_some());
        assert!(console.read_secret("Contraseña: ").is_none());
    }

    #[test]
    fn la_pausa_no_falla_en_eof() {
        let (console, out, _) = console("");
        console.pause("Pulsa Intro para cerrar esta pestaña.");
        assert_eq!(out.text(), "Pulsa Intro para cerrar esta pestaña.");
    }
}
