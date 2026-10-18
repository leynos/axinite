//! Build isolated localhost TLS material for the executable example tests.

use std::sync::Arc;

use anyhow::Result;
use pki_types::PrivatePkcs8KeyDer;
use rustls::{crypto::ring, ClientConfig, RootCertStore, ServerConfig};

/// Keep the generated identity and matching client trust configuration together.
pub(crate) struct TlsMaterial {
    pub(crate) client: ClientConfig,
    pub(crate) server: Arc<ServerConfig>,
}

/// Generate a self-signed `localhost` identity without shared provider state.
pub(crate) fn tls_material() -> Result<TlsMaterial> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let certificate = cert.der().clone();
    let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into();

    let mut roots = RootCertStore::empty();
    roots.add(certificate.clone())?;
    let client = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let mut server = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private_key)?;
    server.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(TlsMaterial {
        client,
        server: Arc::new(server),
    })
}
