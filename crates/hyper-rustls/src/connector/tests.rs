//! Loopback regressions for the TLS transport used by the unchanged libSQL SDK.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hyper::service::Service as _;
use rstest::{fixture, rstest};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::connection_server;
use crate::config::{loaded_native_roots, native_root_store};
use crate::{HttpsConnector, HttpsConnectorBuilder, MaybeHttpsStream};

/// Build a Ring-backed connector using only the supplied trust anchors.
fn connector_with_roots(
    roots: rustls::RootCertStore,
) -> Result<HttpsConnector<hyper::client::HttpConnector>> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);
    http.set_nodelay(true);
    Ok(HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .wrap_connector(http))
}

struct TlsMaterial {
    roots: rustls::RootCertStore,
    certificate: CertificateDer<'static>,
    server: Arc<rustls::ServerConfig>,
}

/// Create a localhost certificate and trust store for loopback tests.
#[fixture]
fn tls_material() -> Result<TlsMaterial> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let cert = cert.der().clone();
    let roots = native_root_store([cert.clone()])?;
    let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.clone()],
        PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )?;
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsMaterial {
        roots,
        certificate: cert,
        server: Arc::new(server),
    })
}

struct PeerRoute {
    host: &'static str,
    override_name: Option<&'static str>,
}

/// Verify HTTPS accepts trusted names and rejects untrusted or mismatched peers.
#[rstest]
#[case(PeerRoute { host: "localhost", override_name: None }, true, true)]
#[case(PeerRoute { host: "localhost", override_name: None }, false, false)]
#[case(PeerRoute { host: "127.0.0.1", override_name: None }, true, false)]
#[case(PeerRoute { host: "127.0.0.1", override_name: Some("localhost") }, true, true)]
#[tokio::test]
async fn https_verifies_the_chain_and_hostname(
    tls_material: Result<TlsMaterial>,
    #[case] peer: PeerRoute,
    #[case] is_trusted: bool,
    #[case] should_connect: bool,
) -> Result<()> {
    let material = tls_material?;
    let roots = if is_trusted {
        material.roots
    } else {
        rustls::RootCertStore::empty()
    };
    let mut connector = connector_with_roots(roots)?;
    connector.override_server_name = peer.override_name.map(str::to_owned);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let connection = tokio::time::timeout(
            Duration::from_secs(5),
            TlsAcceptor::from(material.server).accept(socket),
        )
        .await?;
        if !should_connect {
            assert!(
                connection.is_err(),
                "unverified peers must not complete TLS"
            );
            return Ok::<_, anyhow::Error>(());
        }
        let mut connection = connection?;
        assert_eq!(connection.get_ref().1.alpn_protocol(), None);
        let mut request = [0];
        connection
            .read_exact(&mut request)
            .await?;
        assert_eq!(request, [42]);
        connection.write_all(&[43]).await?;
        connection.shutdown().await?;
        Ok(())
    });
    let uri = format!("https://{}:{port}/", peer.host).parse()?;
    let connection = tokio::time::timeout(Duration::from_secs(5), connector.call(uri))
        .await?
        .map_err(anyhow::Error::from_boxed);
    assert_eq!(connection.is_ok(), should_connect);
    if should_connect {
        let mut connection = connection?;
        connection.write_all(&[42]).await?;
        let mut response = [0];
        connection
            .read_exact(&mut response)
            .await?;
        assert_eq!(response, [43]);
        connection.shutdown().await?;
    }
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    Ok(())
}

/// Verify HTTP stays plaintext and carries TCP_NODELAY on the socket.
#[tokio::test]
async fn http_preserves_plaintext_loopback_transport() -> Result<()> {
    let mut connector = connector_with_roots(rustls::RootCertStore::empty())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let uri = format!("http://{}/", listener.local_addr()?).parse()?;
    let server = tokio::spawn(async move {
        let (mut socket, _) =
            tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let mut request = [0];
        socket.read_exact(&mut request).await?;
        assert_eq!(request, [42]);
        socket.write_all(&[43]).await?;
        Ok::<_, anyhow::Error>(())
    });
    let mut connection = connector
        .call(uri)
        .await
        .map_err(anyhow::Error::from_boxed)?;
    match &connection {
        MaybeHttpsStream::Http(socket) => assert!(socket.nodelay()?),
        MaybeHttpsStream::Https(_) => panic!("HTTP must preserve plaintext routing"),
    }
    connection.write_all(&[42]).await?;
    let mut response = [0];
    connection
        .read_exact(&mut response)
        .await?;
    assert_eq!(response, [43]);
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    Ok(())
}

/// Accept DNS, IPv4 and bracketed IPv6 URI hosts as TLS server names.
#[rstest]
#[case("https://localhost/", "localhost")]
#[case("https://127.0.0.1/", "127.0.0.1")]
#[case("https://[::1]/", "::1")]
fn https_names_accept_dns_and_ip_literals(#[case] uri: &str, #[case] host: &str) -> Result<()> {
    let expected = rustls::pki_types::ServerName::try_from(host.to_owned())?;
    assert_eq!(
        connection_server(&uri.parse()?, false, None)?,
        Some(expected)
    );
    Ok(())
}

/// Reject non-HTTP schemes and URIs without a scheme.
#[rstest]
#[case("ftp://localhost/")]
#[case("localhost:443")]
fn unsupported_schemes_are_rejected(#[case] uri: &str) -> Result<()> {
    let error = connection_server(&uri.parse()?, false, None)
        .expect_err("non-HTTP schemes must be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    Ok(())
}

/// Retain valid root certificates when other entries are malformed.
#[rstest]
#[case(false)]
#[case(true)]
fn native_roots_keep_usable_certificates(
    tls_material: Result<TlsMaterial>,
    #[case] has_invalid_entry: bool,
) -> Result<()> {
    let material = tls_material?;
    let mut certificates = Vec::new();
    if has_invalid_entry {
        certificates.push(CertificateDer::from(vec![0, 1, 2]));
    }
    certificates.push(material.certificate);
    let roots = native_root_store(certificates)?;
    assert_eq!(roots.len(), 1);
    Ok(())
}

/// Return NotFound when native trust input contains no usable certificate.
#[rstest]
#[case(false)]
#[case(true)]
fn native_roots_reject_stores_without_usable_certificates(#[case] has_invalid_entry: bool) {
    let certificates = if has_invalid_entry {
        vec![CertificateDer::from(vec![0, 1, 2])]
    } else {
        Vec::new()
    };
    let error = native_root_store(certificates).expect_err("a usable trust anchor is required");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

/// Reject HTTP before dialling the underlying socket in either HTTPS-only mode.
#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn https_only_rejects_http_before_opening_a_socket(#[case] use_builder: bool) -> Result<()> {
    let mut connector = connector_with_roots(rustls::RootCertStore::empty())?;
    if use_builder {
        connector = HttpsConnectorBuilder::new()
            .with_tls_config((*connector.tls_config).clone())
            .https_only()
            .enable_http1()
            .build();
    } else {
        connector.enforce_https();
    }
    // A listening socket rules out an unrelated connection-refused error.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let uri = format!("http://{}/", listener.local_addr()?).parse()?;
    let error = connector
        .call(uri)
        .await
        .expect_err("HTTPS-only policy must reject plaintext HTTP");
    assert_eq!(error.to_string(), "unsupported scheme http");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "rejected HTTP must never reach the socket connector"
    );
    Ok(())
}

/// Use the explicit TLS peer name instead of the URI host.
#[test]
fn explicit_peer_name_overrides_the_uri_host() -> Result<()> {
    let server = connection_server(&"https://127.0.0.1/".parse()?, true, Some("localhost"))?;
    assert_eq!(
        server,
        Some(rustls::pki_types::ServerName::try_from("localhost")?)
    );
    Ok(())
}

/// Retain valid roots alongside loader errors and reject error-only stores.
#[rstest]
#[case(true)]
#[case(false)]
fn native_loader_errors_do_not_discard_valid_roots(
    tls_material: Result<TlsMaterial>,
    #[case] has_certificate: bool,
) -> Result<()> {
    // Loading a path below a regular file deterministically produces an I/O
    // error without changing process environment or the system trust store.
    let blocked_path = std::path::Path::new(file!()).join("not-a-directory.pem");
    let mut loaded = rustls_native_certs::load_certs_from_paths(Some(&blocked_path), None);
    assert!(!loaded.errors.is_empty());
    if has_certificate {
        loaded
            .certs
            .push(tls_material?.certificate);
        assert_eq!(loaded_native_roots(loaded)?.len(), 1);
    } else {
        let error = loaded_native_roots(loaded).expect_err("loader errors cannot supply trust");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error
            .to_string()
            .contains("native root errors:"));
    }
    Ok(())
}
