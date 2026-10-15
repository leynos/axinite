//! Loopback regressions for verified TLS and the legacy libSQL socket boundary.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rstest::{fixture, rstest};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service as _;

use super::{native_root_store, server_name, Connector};

struct TlsMaterial {
    roots: rustls::RootCertStore,
    certificate: CertificateDer<'static>,
    server: Arc<rustls::ServerConfig>,
}

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

#[rstest]
#[case("localhost", true, true)]
#[case("localhost", false, false)]
#[case("127.0.0.1", true, false)]
#[tokio::test]
async fn https_verifies_the_chain_and_hostname(
    tls_material: Result<TlsMaterial>,
    #[case] host: &str,
    #[case] is_trusted: bool,
    #[case] should_connect: bool,
) -> Result<()> {
    let material = tls_material?;
    let roots = if is_trusted {
        material.roots
    } else {
        rustls::RootCertStore::empty()
    };
    let mut connector = Connector::with_roots(roots)?;
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
        connection.read_exact(&mut request).await?;
        assert_eq!(request, [42]);
        connection.write_all(&[43]).await?;
        connection.shutdown().await?;
        Ok(())
    });
    let uri = format!("https://{host}:{port}/").parse()?;
    let connection = tokio::time::timeout(Duration::from_secs(5), connector.call(uri)).await?;
    assert_eq!(connection.is_ok(), should_connect);
    if should_connect {
        let mut connection = connection?;
        connection.write_all(&[42]).await?;
        let mut response = [0];
        connection.read_exact(&mut response).await?;
        assert_eq!(response, [43]);
        connection.shutdown().await?;
    }
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    Ok(())
}

#[tokio::test]
async fn http_preserves_plaintext_loopback_transport() -> Result<()> {
    let mut connector = Connector::with_roots(rustls::RootCertStore::empty())?;
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
    let mut connection = connector.call(uri).await?;
    match &connection {
        super::stream::Stream::Http(socket) => assert!(socket.nodelay()?),
        super::stream::Stream::Https(_) => panic!("HTTP must preserve plaintext routing"),
    }
    connection.write_all(&[42]).await?;
    let mut response = [0];
    connection.read_exact(&mut response).await?;
    assert_eq!(response, [43]);
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    Ok(())
}

#[rstest]
#[case("https://localhost/", "localhost")]
#[case("https://127.0.0.1/", "127.0.0.1")]
#[case("https://[::1]/", "::1")]
fn https_names_accept_dns_and_ip_literals(#[case] uri: &str, #[case] host: &str) -> Result<()> {
    let expected = rustls::pki_types::ServerName::try_from(host.to_owned())?;
    assert_eq!(server_name(&uri.parse()?)?, Some(expected));
    Ok(())
}

#[rstest]
#[case("ftp://localhost/")]
#[case("localhost:443")]
fn unsupported_schemes_are_rejected(#[case] uri: &str) -> Result<()> {
    let error = server_name(&uri.parse()?).expect_err("non-HTTP schemes must be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    Ok(())
}

#[cfg(feature = "replication")]
#[tokio::test]
async fn replica_connection_failures_return_errors_with_builtin_tls() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let database = crate::Builder::new_remote_replica(
        &directory.path().join("replica.db"),
        "http://127.0.0.1:0".to_owned(),
        "test-token".to_owned(),
    )
    .build()
    .await?;
    // Creating a replica is lazy; sync exercises the built-in connector.
    let result = tokio::time::timeout(Duration::from_secs(5), database.sync()).await?;
    assert!(
        result.is_err(),
        "unreachable replica must return a connection error"
    );
    Ok(())
}

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
