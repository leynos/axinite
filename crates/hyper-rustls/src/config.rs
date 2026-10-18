//! Configure native or Mozilla trust anchors without weakening certificate verification.

#[cfg(any(feature = "rustls-native-certs", feature = "webpki-roots"))]
use rustls::client::WantsClientCert;
use rustls::{ClientConfig, ConfigBuilder, WantsVerifier};

/// Methods for configuring roots
///
/// This adds methods (gated by crate features) for easily configuring
/// TLS server roots a rustls ClientConfig will trust.
pub trait ConfigBuilderExt {
    /// This configures the platform's trusted certs, as implemented by
    /// rustls-native-certs
    ///
    /// This will return an error if no valid certs were found. In that case,
    /// it's recommended to use `with_webpki_roots`.
    #[cfg(feature = "rustls-native-certs")]
    fn with_native_roots(self) -> std::io::Result<ConfigBuilder<ClientConfig, WantsClientCert>>;

    /// This configures the webpki roots, which are Mozilla's set of
    /// trusted roots as packaged by webpki-roots.
    #[cfg(feature = "webpki-roots")]
    fn with_webpki_roots(self) -> ConfigBuilder<ClientConfig, WantsClientCert>;
}

impl ConfigBuilderExt for ConfigBuilder<ClientConfig, WantsVerifier> {
    #[cfg(feature = "rustls-native-certs")]
    fn with_native_roots(self) -> std::io::Result<ConfigBuilder<ClientConfig, WantsClientCert>> {
        let roots = loaded_native_roots(rustls_native_certs::load_native_certs())?;
        Ok(self.with_root_certificates(roots))
    }

    #[cfg(feature = "webpki-roots")]
    fn with_webpki_roots(self) -> ConfigBuilder<ClientConfig, WantsClientCert> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(
            webpki_roots::TLS_SERVER_ROOTS
                .iter()
                .cloned(),
        );
        crate::log::debug!("with_webpki_roots processed {} roots", roots.len());
        self.with_root_certificates(roots)
    }
}

/// Keep usable platform roots despite malformed entries or partial loader errors.
#[cfg(feature = "rustls-native-certs")]
pub(crate) fn native_root_store(
    certificates: impl IntoIterator<Item = pki_types::CertificateDer<'static>>,
) -> std::io::Result<rustls::RootCertStore> {
    let mut roots = rustls::RootCertStore::empty();
    let (_valid, invalid) = roots.add_parsable_certificates(certificates);
    crate::log::debug!("with_native_roots processed {_valid} valid and {invalid} invalid certs");
    if roots.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no usable native TLS certificate roots ({invalid} invalid entries)"),
        ));
    }
    Ok(roots)
}

/// Preserve partial platform-loader results while diagnosing an unusable store.
#[cfg(feature = "rustls-native-certs")]
pub(crate) fn loaded_native_roots(
    native: rustls_native_certs::CertificateResult,
) -> std::io::Result<rustls::RootCertStore> {
    native_root_store(native.certs).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("{error}; native root errors: {:?}", native.errors),
        )
    })
}

#[cfg(all(test, feature = "rustls-native-certs"))]
mod tests;
