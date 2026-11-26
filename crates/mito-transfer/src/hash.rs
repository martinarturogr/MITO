//! Hashes declarados en manifiestos Scoop (`hash_verifier` y `download_hashes`).
//!
//! Contrato: `docs/spec/modules/transfer.md` («Formatos de hash aceptados»). Este
//! módulo solo valida y normaliza; el cálculo de digests lo hace la descarga.

use digest::DynDigest;
use mito_core::Message;
use mito_core::error::{Result, bail};
use serde_json::Value;

/// Algoritmos de digest fijo admitidos con prefijo explícito (`algoritmo:digest`).
///
/// Equivale a los algoritmos de `hashlib` con tamaño fijo que se usan en la práctica.
/// Sin prefijo se infieren por longitud: 32 → MD5, 40 → SHA-1, 64 → SHA-256,
/// 128 → SHA-512.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs, reason = "los nombres de variante son los algoritmos")]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Sha512_224,
    Sha512_256,
    Sha3_224,
    Sha3_256,
    Sha3_384,
    Sha3_512,
    Blake2b,
    Blake2s,
}

impl HashAlgorithm {
    /// Nombre como en `hashlib` (`sha256`, `sha3_256`, `blake2b`…).
    pub fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha224 => "sha224",
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
            Self::Sha512_224 => "sha512_224",
            Self::Sha512_256 => "sha512_256",
            Self::Sha3_224 => "sha3_224",
            Self::Sha3_256 => "sha3_256",
            Self::Sha3_384 => "sha3_384",
            Self::Sha3_512 => "sha3_512",
            Self::Blake2b => "blake2b",
            Self::Blake2s => "blake2s",
        }
    }

    /// Tamaño del digest en bytes.
    pub fn digest_size(self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha224 | Self::Sha512_224 | Self::Sha3_224 => 28,
            Self::Sha256 | Self::Sha512_256 | Self::Sha3_256 | Self::Blake2s => 32,
            Self::Sha384 | Self::Sha3_384 => 48,
            Self::Sha512 | Self::Sha3_512 | Self::Blake2b => 64,
        }
    }

    /// Algoritmo por nombre de `hashlib`, en minúsculas.
    pub fn from_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|a| a.name() == name)
    }

    /// Algoritmo inferido por la longitud de un digest hexadecimal sin prefijo.
    pub fn from_hex_len(len: usize) -> Option<Self> {
        match len {
            32 => Some(Self::Md5),
            40 => Some(Self::Sha1),
            64 => Some(Self::Sha256),
            128 => Some(Self::Sha512),
            _ => None,
        }
    }
}

const ALL: [HashAlgorithm; 14] = [
    HashAlgorithm::Md5,
    HashAlgorithm::Sha1,
    HashAlgorithm::Sha224,
    HashAlgorithm::Sha256,
    HashAlgorithm::Sha384,
    HashAlgorithm::Sha512,
    HashAlgorithm::Sha512_224,
    HashAlgorithm::Sha512_256,
    HashAlgorithm::Sha3_224,
    HashAlgorithm::Sha3_256,
    HashAlgorithm::Sha3_384,
    HashAlgorithm::Sha3_512,
    HashAlgorithm::Blake2b,
    HashAlgorithm::Blake2s,
];

/// Algoritmos de `hashlib` de tamaño variable: se reconocen, pero `hashlib` les da
/// `digest_size == 0` y el original responde «Formato de hash incorrecto».
const VARIABLE: [&str; 2] = ["shake_128", "shake_256"];

/// Hash esperado y validado: algoritmo y digest hexadecimal en minúsculas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedHash {
    /// Algoritmo del digest.
    pub algorithm: HashAlgorithm,
    /// Digest hexadecimal en minúsculas, de `2 × digest_size` caracteres.
    pub digest: String,
}

/// ¿Equivale a «sin hash»? `null`, vacío, solo espacios o `skip` (sin mayúsculas).
fn is_absent(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || value.eq_ignore_ascii_case("skip")
}

/// Valida un hash declarado (`hash_verifier`). `None` significa «sin verificación».
///
/// # Errors
///
/// `Algoritmo de hash no soportado: {p1}` o `Formato de hash incorrecto`.
pub fn parse_hash(expected: Option<&str>) -> Result<Option<ExpectedHash>> {
    let Some(expected) = expected.filter(|e| !is_absent(e)) else {
        return Ok(None);
    };
    let expected = expected.trim().to_lowercase();
    let (name, digest) = match expected.split_once(':') {
        Some((name, digest)) => (name.to_owned(), digest),
        None => (
            HashAlgorithm::from_hex_len(expected.len())
                .map(|a| a.name().to_owned())
                .unwrap_or_default(),
            expected.as_str(),
        ),
    };
    let Some(algorithm) = HashAlgorithm::from_name(&name) else {
        if VARIABLE.contains(&name.as_str()) {
            return bail(Message::new("Formato de hash incorrecto"));
        }
        return bail(Message::new("Algoritmo de hash no soportado: {p1}").arg("p1", name));
    };
    let valid = digest.len() == algorithm.digest_size() * 2
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !valid {
        return bail(Message::new("Formato de hash incorrecto"));
    }
    Ok(Some(ExpectedHash {
        algorithm,
        digest: digest.to_owned(),
    }))
}

/// Digest incremental de un [`HashAlgorithm`] (equivale al `hashlib.new(...)` del original).
pub struct Hasher {
    algorithm: HashAlgorithm,
    inner: Box<dyn DynDigest>,
}

impl std::fmt::Debug for Hasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hasher")
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

impl Hasher {
    /// Crea un digest vacío del algoritmo.
    pub fn new(algorithm: HashAlgorithm) -> Self {
        let inner: Box<dyn DynDigest> = match algorithm {
            HashAlgorithm::Md5 => Box::new(md5::Md5::default()),
            HashAlgorithm::Sha1 => Box::new(sha1::Sha1::default()),
            HashAlgorithm::Sha224 => Box::new(sha2::Sha224::default()),
            HashAlgorithm::Sha256 => Box::new(sha2::Sha256::default()),
            HashAlgorithm::Sha384 => Box::new(sha2::Sha384::default()),
            HashAlgorithm::Sha512 => Box::new(sha2::Sha512::default()),
            HashAlgorithm::Sha512_224 => Box::new(sha2::Sha512_224::default()),
            HashAlgorithm::Sha512_256 => Box::new(sha2::Sha512_256::default()),
            HashAlgorithm::Sha3_224 => Box::new(sha3::Sha3_224::default()),
            HashAlgorithm::Sha3_256 => Box::new(sha3::Sha3_256::default()),
            HashAlgorithm::Sha3_384 => Box::new(sha3::Sha3_384::default()),
            HashAlgorithm::Sha3_512 => Box::new(sha3::Sha3_512::default()),
            HashAlgorithm::Blake2b => Box::new(blake2::Blake2b512::default()),
            HashAlgorithm::Blake2s => Box::new(blake2::Blake2s256::default()),
        };
        Self { algorithm, inner }
    }

    /// Algoritmo de este digest.
    pub fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }

    /// Añade bytes al digest.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Digest hexadecimal en minúsculas de lo recibido hasta ahora (sin consumirlo).
    pub fn hex_digest(&self) -> String {
        use std::fmt::Write as _;
        let bytes = self.inner.box_clone().finalize();
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in &bytes {
            // Escribir en un String no puede fallar.
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}

impl ExpectedHash {
    /// Digest vacío del algoritmo esperado.
    pub fn hasher(&self) -> Hasher {
        Hasher::new(self.algorithm)
    }
}

/// Valida un valor JSON de hash individual (texto, `null` o `skip`).
///
/// # Errors
///
/// `El hash debe ser texto, null o skip` para otros tipos, y los de [`parse_hash`].
pub fn parse_hash_value(expected: &Value) -> Result<Option<ExpectedHash>> {
    match expected {
        Value::Null => Ok(None),
        Value::String(text) => parse_hash(Some(text)),
        _ => bail(Message::new("El hash debe ser texto, null o skip")),
    }
}

/// Reparte el campo `hash` de un manifiesto entre sus `url_count` URL
/// (`download_hashes`).
///
/// Devuelve, por URL, `None` si no se verifica o el texto **original** del hash
/// declarado (sin normalizar, como el original).
///
/// # Errors
///
/// `La cantidad de hashes debe coincidir con las URL` y los de [`parse_hash_value`].
pub fn download_hashes(url_count: usize, value: Option<&Value>) -> Result<Vec<Option<String>>> {
    let value = value.unwrap_or(&Value::Null);
    let absent = match value {
        Value::Null => true,
        Value::String(text) => is_absent(text),
        _ => false,
    };
    if absent {
        return Ok(vec![None; url_count]);
    }
    let hashes = match value {
        Value::Array(items) => items.iter().collect::<Vec<_>>(),
        other => vec![other],
    };
    if hashes.len() != url_count {
        return bail(Message::new(
            "La cantidad de hashes debe coincidir con las URL",
        ));
    }
    hashes
        .into_iter()
        .map(|expected| {
            Ok(parse_hash_value(expected)?.and_then(|_| expected.as_str().map(str::to_owned)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn error(result: Result<impl std::fmt::Debug>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn prefijos_explicitos_e_inferencia_por_longitud() {
        let md5 = parse_hash(Some(&format!("MD5:{}", "A".repeat(32))))
            .unwrap()
            .unwrap();
        assert_eq!(md5.algorithm, HashAlgorithm::Md5);
        assert_eq!(md5.digest, "a".repeat(32));
        for (len, algorithm) in [
            (32, HashAlgorithm::Md5),
            (40, HashAlgorithm::Sha1),
            (64, HashAlgorithm::Sha256),
            (128, HashAlgorithm::Sha512),
        ] {
            let parsed = parse_hash(Some(&"0".repeat(len))).unwrap().unwrap();
            assert_eq!(parsed.algorithm, algorithm);
        }
        let sha3 = parse_hash(Some(&format!("sha3_256:{}", "f".repeat(64))))
            .unwrap()
            .unwrap();
        assert_eq!(sha3.algorithm, HashAlgorithm::Sha3_256);
    }

    #[test]
    fn digests_de_todos_los_algoritmos() {
        // Vectores de "abc" (FIPS 180, RFC 1321, RFC 7693) y de la cadena vacía.
        let vectors = [
            (HashAlgorithm::Md5, "900150983cd24fb0d6963f7d28e17f72"),
            (
                HashAlgorithm::Sha1,
                "a9993e364706816aba3e25717850c26c9cd0d89d",
            ),
            (
                HashAlgorithm::Sha224,
                "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
            ),
            (
                HashAlgorithm::Sha256,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                HashAlgorithm::Sha3_256,
                "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
            ),
            (
                HashAlgorithm::Blake2s,
                "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982",
            ),
        ];
        for (algorithm, expected) in vectors {
            let mut hasher = Hasher::new(algorithm);
            hasher.update(b"a");
            hasher.update(b"bc");
            assert_eq!(hasher.hex_digest(), expected, "{}", algorithm.name());
            assert_eq!(hasher.hex_digest().len(), algorithm.digest_size() * 2);
        }
        for algorithm in ALL {
            let hasher = Hasher::new(algorithm);
            assert_eq!(hasher.hex_digest().len(), algorithm.digest_size() * 2);
            assert_eq!(hasher.algorithm(), algorithm);
        }
        assert_eq!(
            Hasher::new(HashAlgorithm::Md5).hex_digest(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            Hasher::new(HashAlgorithm::Sha1).hex_digest(),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn el_hash_esperado_crea_su_hasher() {
        let expected = parse_hash(Some(&"0".repeat(64))).unwrap().unwrap();
        assert_eq!(expected.hasher().algorithm(), HashAlgorithm::Sha256);
    }

    #[test]
    fn valores_sin_hash() {
        for absent in [None, Some(""), Some("   "), Some("skip"), Some(" SKIP ")] {
            assert_eq!(parse_hash(absent).unwrap(), None, "{absent:?}");
        }
    }

    #[test]
    fn errores_de_formato_y_algoritmo() {
        assert_eq!(
            error(parse_hash(Some("md5:invalid"))),
            "Formato de hash incorrecto"
        );
        assert_eq!(
            error(parse_hash(Some(&"a".repeat(63)))),
            "Algoritmo de hash no soportado: "
        );
        assert_eq!(
            error(parse_hash(Some("whirlpool:00"))),
            "Algoritmo de hash no soportado: whirlpool"
        );
        assert_eq!(
            error(parse_hash(Some("shake_128:00"))),
            "Formato de hash incorrecto"
        );
        assert_eq!(
            error(parse_hash_value(&json!(42))),
            "El hash debe ser texto, null o skip"
        );
    }

    #[test]
    fn reparto_de_hashes_por_url() {
        let md5 = format!("md5:{}", "a".repeat(32));
        assert_eq!(download_hashes(2, None).unwrap(), [None, None]);
        assert_eq!(download_hashes(1, Some(&json!("skip"))).unwrap(), [None]);
        assert_eq!(
            download_hashes(2, Some(&json!(["skip", md5]))).unwrap(),
            [None, Some(md5.clone())]
        );
        // Se conserva el texto original, sin normalizar.
        let upper = format!("  MD5:{}", "A".repeat(32));
        assert_eq!(
            download_hashes(1, Some(&json!(upper))).unwrap(),
            [Some(upper)]
        );
        assert_eq!(
            error(download_hashes(2, Some(&json!("a".repeat(64))))),
            "La cantidad de hashes debe coincidir con las URL"
        );
        assert_eq!(
            error(download_hashes(1, Some(&json!([7])))),
            "El hash debe ser texto, null o skip"
        );
        assert_eq!(
            error(download_hashes(1, Some(&json!(7)))),
            "El hash debe ser texto, null o skip"
        );
    }
}
