//! SHA-256 en hexadecimal minúscula.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// SHA-256 de `bytes` como 64 caracteres hexadecimales en minúsculas.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn el_hash_de_la_cadena_vacia_es_el_conocido() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
