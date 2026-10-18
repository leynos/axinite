//! Loopback contracts for the public lazy TLS acceptor.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, ensure, Result};
use hyper::client::connect::Connection;
use hyper::server::accept::Accept;
use hyper::server::conn::AddrIncoming;
use hyper::service::Service as _;
use hyper::{client::HttpConnector, Uri};
use rstest::{fixture, rstest};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::TlsAcceptor;
use crate::{HttpsConnector, HttpsConnectorBuilder, MaybeHttpsStream};

const IO_TIMEOUT: Duration = Duration::from_secs(5);

struct TlsMaterial {
    certificate: CertificateDer<'static>,
    private_key: rustls::pki_types::PrivateKeyDer<'static>,
}

/// Create a self-signed localhost certificate for loopback TLS tests.
#[fixture]
fn tls_material() -> Result<TlsMaterial> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    Ok(TlsMaterial {
        certificate: cert.der().clone(),
        private_key: PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    })
}

#[derive(Clone, Copy)]
enum HttpProtocol {
    Http1,
    #[cfg(feature = "http2")]
    Http2,
}

impl HttpProtocol {
    /// Return the ALPN protocol advertised by the test server.
    fn server_alpn(self) -> Option<&'static [u8]> {
        match self {
            Self::Http1 => None,
            #[cfg(feature = "http2")]
            Self::Http2 => Some(b"h2"),
        }
    }

    /// Report whether the selected protocol should negotiate HTTP/2.
    fn expects_h2(self) -> bool {
        self.server_alpn() == Some(b"h2")
    }
}

#[derive(Clone, Copy)]
enum HandshakeDriver {
    Read,
    Write,
}

/// Build a lazy TLS acceptor configured for the selected protocol.
fn server_acceptor(
    material: TlsMaterial,
    listener: TcpListener,
    protocol: HttpProtocol,
) -> Result<TlsAcceptor> {
    let incoming = AddrIncoming::from_listener(listener)?;
    let builder = TlsAcceptor::builder()
        .with_single_cert(vec![material.certificate], material.private_key)?;
    let builder = match protocol {
        HttpProtocol::Http1 => builder.with_alpn_protocols(Vec::new()),
        #[cfg(feature = "http2")]
        HttpProtocol::Http2 => builder.with_http2_alpn(),
    };
    Ok(builder.with_incoming(incoming))
}

/// Create a client config that trusts the test certificate when requested.
fn client_config(material: &TlsMaterial, trust_server: bool) -> Result<ClientConfig> {
    let mut roots = RootCertStore::empty();
    if trust_server {
        roots.add(material.certificate.clone())?;
    }
    Ok(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// Build a connector with the requested peer name and ALPN mode.
fn client_connector(
    config: ClientConfig,
    server_name: Option<&str>,
    protocol: HttpProtocol,
) -> HttpsConnector<HttpConnector> {
    let builder = HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http();
    let builder = match server_name {
        Some(name) => builder.with_server_name(name.to_owned()),
        None => builder,
    };
    match protocol {
        HttpProtocol::Http1 => builder.enable_http1().build(),
        #[cfg(feature = "http2")]
        HttpProtocol::Http2 => builder.enable_http2().build(),
    }
}

/// Accept one loopback socket while bounding the accept wait.
async fn accepted_stream(mut acceptor: TlsAcceptor) -> Result<super::TlsStream> {
    let accepted = tokio::time::timeout(
        IO_TIMEOUT,
        poll_fn(|cx| Pin::new(&mut acceptor).poll_accept(cx)),
    )
    .await
    .map_err(|error| anyhow!("accept timed out: {error}"))?
    .ok_or_else(|| anyhow!("TLS acceptor closed before accepting a connection"))??;
    Ok(accepted)
}

/// Drive TLS from the acceptor side and verify ALPN and the byte exchange.
async fn acceptor_io_exchange(
    acceptor: TlsAcceptor,
    driver: HandshakeDriver,
    expected_alpn: Option<&'static [u8]>,
) -> Result<()> {
    let mut stream = accepted_stream(acceptor).await?;
    ensure!(
        stream.io().is_some(),
        "accepted socket should be inspectable"
    );
    ensure!(
        stream.connection().is_none(),
        "the TLS connection should remain lazy until I/O"
    );

    match driver {
        HandshakeDriver::Read => {
            let mut request = [0];
            tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut request)).await??;
            ensure!(request == [42], "unexpected client payload: {request:?}");
        }
        HandshakeDriver::Write => {
            tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[42])).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
        }
    }

    ensure!(
        stream.io().is_some(),
        "socket should remain inspectable after TLS"
    );
    let connection = stream
        .connection()
        .ok_or_else(|| anyhow!("TLS I/O should expose the completed connection"))?;
    ensure!(
        connection.alpn_protocol() == expected_alpn,
        "unexpected server-side ALPN: {:?}",
        connection.alpn_protocol()
    );

    match driver {
        HandshakeDriver::Read => {
            tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[43])).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
        }
        HandshakeDriver::Write => {
            let mut response = [0];
            tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut response)).await??;
            ensure!(response == [43], "unexpected client response: {response:?}");
        }
    }
    tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await??;
    Ok(())
}

/// Connect to the loopback acceptor and verify TLS selection and connection metadata.
async fn connector_io_exchange(
    connector: &mut HttpsConnector<HttpConnector>,
    port: u16,
    driver: HandshakeDriver,
    expect_h2: bool,
) -> Result<()> {
    let uri: Uri = format!("https://127.0.0.1:{port}/").parse()?;
    let mut stream = tokio::time::timeout(IO_TIMEOUT, connector.call(uri))
        .await
        .map_err(|error| anyhow!("connector handshake timed out: {error}"))?
        .map_err(anyhow::Error::from_boxed)?;
    ensure!(
        matches!(&stream, MaybeHttpsStream::Https(_)),
        "HTTPS URI should yield a TLS stream"
    );
    ensure!(
        stream.connected().is_negotiated_h2() == expect_h2,
        "connector metadata should reflect the negotiated ALPN"
    );

    match driver {
        HandshakeDriver::Read => {
            tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[42])).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await??;
            let mut response = [0];
            tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut response)).await??;
            ensure!(response == [43], "unexpected server payload: {response:?}");
        }
        HandshakeDriver::Write => {
            let mut request = [0];
            tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut request)).await??;
            ensure!(request == [42], "unexpected server payload: {request:?}");
            tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[43])).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await??;
        }
    }
    Ok(())
}

/// Run a bidirectional TLS exchange for one handshake driver and protocol.
async fn exercise_acceptor(
    material: TlsMaterial,
    driver: HandshakeDriver,
    protocol: HttpProtocol,
) -> Result<()> {
    let config = client_config(&material, true)?;
    let mut connector = client_connector(config, Some("localhost"), protocol);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = server_acceptor(material, listener, protocol)?;
    let server = tokio::spawn(acceptor_io_exchange(
        acceptor,
        driver,
        protocol.server_alpn(),
    ));

    connector_io_exchange(&mut connector, port, driver, protocol.expects_h2()).await?;
    tokio::time::timeout(IO_TIMEOUT, server)
        .await
        .map_err(|error| anyhow!("acceptor exchange timed out: {error}"))???;
    Ok(())
}

/// Verify read- and write-driven I/O complete the lazy HTTP/1 handshake.
#[rstest]
#[case::read_drives_handshake(HandshakeDriver::Read)]
#[case::write_drives_handshake(HandshakeDriver::Write)]
#[tokio::test]
async fn public_acceptor_completes_lazy_http1_handshake(
    tls_material: Result<TlsMaterial>,
    #[case] driver: HandshakeDriver,
) -> Result<()> {
    exercise_acceptor(tls_material?, driver, HttpProtocol::Http1).await
}

/// Verify an HTTP/2 handshake is reflected in connector metadata.
#[cfg(feature = "http2")]
#[rstest]
#[tokio::test]
async fn public_connector_reports_negotiated_http2_metadata(
    tls_material: Result<TlsMaterial>,
) -> Result<()> {
    exercise_acceptor(tls_material?, HandshakeDriver::Read, HttpProtocol::Http2).await
}

/// Confirm a rejected handshake never exposes a completed TLS connection.
async fn acceptor_reports_rejected_peer(acceptor: TlsAcceptor) -> Result<()> {
    let mut stream = accepted_stream(acceptor).await?;
    ensure!(
        stream.io().is_some(),
        "accepted socket should be inspectable"
    );
    ensure!(stream.connection().is_none(), "TLS must still be lazy");
    let mut payload = [0];
    let result = tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut payload)).await;
    ensure!(
        matches!(&result, Ok(Err(_))),
        "rejected client handshake should surface as acceptor I/O failure: {result:?}"
    );
    ensure!(
        stream.connection().is_none(),
        "a rejected handshake must not expose a TLS connection"
    );
    Ok(())
}

/// Ensure client trust and hostname failures surface as acceptor I/O errors.
#[rstest]
#[case::untrusted_certificate(false, Some("localhost"))]
#[case::hostname_mismatch(true, None)]
#[tokio::test]
async fn public_acceptor_io_propagates_client_verification_failures(
    tls_material: Result<TlsMaterial>,
    #[case] trust_server: bool,
    #[case] server_name: Option<&str>,
) -> Result<()> {
    let material = tls_material?;
    let config = client_config(&material, trust_server)?;
    let mut connector = client_connector(config, server_name, HttpProtocol::Http1);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = server_acceptor(material, listener, HttpProtocol::Http1)?;
    let server = tokio::spawn(acceptor_reports_rejected_peer(acceptor));
    let uri: Uri = format!("https://127.0.0.1:{port}/").parse()?;
    let result = tokio::time::timeout(IO_TIMEOUT, connector.call(uri)).await;
    ensure!(
        matches!(&result, Ok(Err(_))),
        "client should reject the untrusted or mismatched server certificate: {result:?}"
    );
    tokio::time::timeout(IO_TIMEOUT, server)
        .await
        .map_err(|error| anyhow!("acceptor rejection timed out: {error}"))???;
    Ok(())
}

mod terminal;
