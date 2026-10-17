//! Regression contracts for connector builder policy and protocol selection.

/// Build an HTTPS-only connector with explicit Ring and WebPKI roots.
#[test]
#[cfg(all(feature = "webpki-roots", feature = "http1", feature = "ring"))]
fn test_builder() {
    let _connector = super::ConnectorBuilder::new()
        .with_provider_and_webpki_roots(rustls::crypto::ring::default_provider())
        .expect("the explicit Ring provider supports safe defaults")
        .https_only()
        .enable_http1()
        .build();
}

/// Reject a TLS config whose ALPN protocols are already set.
#[test]
#[cfg(all(feature = "http1", feature = "ring"))]
#[should_panic(expected = "ALPN protocols should not be pre-defined")]
fn test_reject_predefined_alpn() {
    let roots = rustls::RootCertStore::empty();
    let mut config_with_alpn = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("the explicit Ring provider supports safe defaults")
    .with_root_certificates(roots)
    .with_no_client_auth();
    config_with_alpn.alpn_protocols = vec![b"fancyprotocol".to_vec()];
    let _connector = super::ConnectorBuilder::new()
        .with_tls_config(config_with_alpn)
        .https_only()
        .enable_http1()
        .build();
}

/// Select empty, h2, or h2 plus HTTP/1 ALPN from builder protocol choices.
#[test]
#[cfg(all(feature = "http1", feature = "http2", feature = "ring"))]
fn test_alpn() {
    let roots = rustls::RootCertStore::empty();
    let tls_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("the explicit Ring provider supports safe defaults")
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config.clone())
        .https_only()
        .enable_http1()
        .build();
    assert!(connector
        .tls_config
        .alpn_protocols
        .is_empty());
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config.clone())
        .https_only()
        .enable_http2()
        .build();
    assert_eq!(&connector.tls_config.alpn_protocols, &[b"h2".to_vec()]);
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config.clone())
        .https_only()
        .enable_http1()
        .enable_http2()
        .build();
    assert_eq!(
        &connector.tls_config.alpn_protocols,
        &[b"h2".to_vec(), b"http/1.1".to_vec()]
    );
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_only()
        .enable_all_versions()
        .build();
    assert_eq!(
        &connector.tls_config.alpn_protocols,
        &[b"h2".to_vec(), b"http/1.1".to_vec()]
    );
}

/// Select h2 ALPN when HTTP/1 support is disabled.
#[test]
#[cfg(all(not(feature = "http1"), feature = "http2", feature = "ring"))]
fn test_alpn_http2() {
    let roots = rustls::RootCertStore::empty();
    let tls_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("the explicit Ring provider supports safe defaults")
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config.clone())
        .https_only()
        .enable_http2()
        .build();
    assert_eq!(&connector.tls_config.alpn_protocols, &[b"h2".to_vec()]);
    let connector = super::ConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_only()
        .enable_all_versions()
        .build();
    assert_eq!(&connector.tls_config.alpn_protocols, &[b"h2".to_vec()]);
}
