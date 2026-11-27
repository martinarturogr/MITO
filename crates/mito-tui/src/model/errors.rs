//! Texto de los errores del motor y datos de una descarga alterada.

use std::error::Error as StdError;

use mito_i18n::Translator;

/// Describe un error del motor para el usuario: si tiene un mensaje traducible se
/// traduce y, si no, se usa su texto (`str(exc)` del original).
pub fn describe(error: &mito_engine::Error, tr: &Translator) -> String {
    error
        .message()
        .map_or_else(|| error.to_string(), |message| tr.tr(&message))
}

/// Datos de una descarga alterada por la red (`DownloadIntegrityError`), para el
/// diálogo «DESCARGA ALTERADA POR LA RED».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityInfo {
    /// URL pedida, sin consulta ni fragmento.
    pub url: String,
    /// Bytes recibidos, si se conocen.
    pub size: Option<u64>,
    /// Servidor final.
    pub host: String,
    /// `true` si lo recibido parece un portal de autenticación de la red.
    pub portal: bool,
}

impl IntegrityInfo {
    /// Busca en la cadena `source()` del error un `mito_transfer::Error::Integrity`.
    pub fn from_error(error: &mito_engine::Error) -> Option<Self> {
        let mut node: Option<&(dyn StdError + 'static)> = Some(error);
        while let Some(current) = node {
            if let Some(found) = current
                .downcast_ref::<mito_transfer::Error>()
                .and_then(Self::from_transfer)
            {
                return Some(found);
            }
            // `Error::Transfer` es transparente: su `source()` salta al propio error de
            // transferencia, así que hay que mirarlo aquí.
            if let Some(mito_engine::Error::Transfer(inner)) =
                current.downcast_ref::<mito_engine::Error>()
                && let Some(found) = Self::from_transfer(inner)
            {
                return Some(found);
            }
            node = current.source();
        }
        None
    }

    fn from_transfer(error: &mito_transfer::Error) -> Option<Self> {
        match error {
            mito_transfer::Error::Integrity {
                url,
                size,
                host,
                portal,
                ..
            } => Some(Self {
                url: url.clone(),
                size: *size,
                host: host.clone(),
                portal: *portal,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use mito_core::Message;
    use mito_core::diagnostics;

    use super::*;

    fn integrity() -> mito_transfer::Error {
        mito_transfer::Error::Integrity {
            message: Box::new(Message::new("Descarga alterada")),
            url: "https://example.org/a.zip".to_owned(),
            size: Some(1234),
            host: "proxy.local".to_owned(),
            portal: true,
        }
    }

    fn info() -> IntegrityInfo {
        IntegrityInfo {
            url: "https://example.org/a.zip".to_owned(),
            size: Some(1234),
            host: "proxy.local".to_owned(),
            portal: true,
        }
    }

    #[test]
    fn describe_traduce_el_mensaje_o_usa_el_texto() {
        let tr = Translator::new("english");
        let domain = mito_engine::Error::domain(Message::new("{p1}").arg("p1", "dato"));
        assert_eq!(describe(&domain, &tr), "dato");
        let io = mito_engine::Error::io("x", std::io::Error::other("fallo"));
        assert_eq!(describe(&io, &tr), io.to_string());
    }

    #[test]
    fn encuentra_la_integridad_en_un_error_de_transferencia() {
        let error = mito_engine::Error::Transfer(integrity());
        assert_eq!(IntegrityInfo::from_error(&error), Some(info()));
    }

    #[test]
    fn encuentra_la_integridad_dentro_de_una_fase() {
        let root = tempfile::tempdir().unwrap();
        let layout = mito_core::Layout::new(root.path());
        let wrapped: Result<(), mito_engine::Error> =
            diagnostics::step(&layout, "install", "download", Some("main/tool"), || {
                Err::<(), _>(mito_engine::Error::Transfer(integrity()))
            })
            .map_err(mito_engine::Error::Step);
        let error = wrapped.unwrap_err();
        assert!(matches!(error, mito_engine::Error::Step(_)));
        assert_eq!(IntegrityInfo::from_error(&error), Some(info()));
    }

    #[test]
    fn otros_errores_no_son_de_integridad() {
        let error = mito_engine::Error::domain(Message::new("otro"));
        assert_eq!(IntegrityInfo::from_error(&error), None);
    }
}
