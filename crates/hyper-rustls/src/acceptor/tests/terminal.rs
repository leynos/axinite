//! Contracts for flush and shutdown as the first acceptor-side TLS I/O.

use std::time::Duration;

use anyhow::{anyhow, ensure, Result};
use hyper::client::HttpConnector;
use hyper::service::Service as _;
use hyper::Uri;
use rstest::rstest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{
    accepted_stream, client_config, client_connector, server_acceptor, tls_material, HttpProtocol,
    TlsMaterial,
};
use crate::{HttpsConnector, MaybeHttpsStream, TlsAcceptor};

const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
enum TerminalOperation {
    Flush,
    Shutdown,
}

#[derive(Clone, Copy, Debug)]
enum RejectedPeer {
    MalformedRecord,
    UntrustedCertificate,
}

/// Flush the server stream first, then exchange one byte in each direction.
async fn flush_first_server_exchange(acceptor: TlsAcceptor) -> Result<()> {
    let mut stream = accepted_stream(acceptor).await?;
    ensure!(stream.connection().is_none(), "TLS should still be lazy");
    tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
    ensure!(
        stream.connection().is_some(),
        "flush should complete the TLS handshake"
    );

    let mut request = [0];
    tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut request)).await??;
    ensure!(request == [42], "unexpected client payload: {request:?}");
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[43])).await??;
    tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
    Ok(())
}

/// Connect with a trusted localhost certificate and exchange one byte each way.
async fn flush_first_client_exchange(
    mut connector: HttpsConnector<HttpConnector>,
    port: u16,
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

    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&[42])).await??;
    tokio::time::timeout(IO_TIMEOUT, stream.flush()).await??;
    let mut response = [0];
    tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut response)).await??;
    ensure!(response == [43], "unexpected server payload: {response:?}");
    Ok(())
}

/// Confirm `shutdown` completes a lazy handshake and sends TLS close_notify.
async fn shutdown_first_server(acceptor: TlsAcceptor) -> Result<()> {
    let mut stream = accepted_stream(acceptor).await?;
    ensure!(
        stream.io().is_some(),
        "accepted socket should be inspectable"
    );
    ensure!(stream.connection().is_none(), "TLS should still be lazy");
    tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await??;
    ensure!(
        stream.connection().is_some(),
        "shutdown should complete the TLS handshake"
    );
    Ok(())
}

/// Read the clean EOF sent by the server's TLS close_notify.
async fn client_observes_shutdown(
    mut connector: HttpsConnector<HttpConnector>,
    port: u16,
) -> Result<()> {
    let uri: Uri = format!("https://127.0.0.1:{port}/").parse()?;
    let mut stream = tokio::time::timeout(IO_TIMEOUT, connector.call(uri))
        .await
        .map_err(|error| anyhow!("connector handshake timed out: {error}"))?
        .map_err(anyhow::Error::from_boxed)?;
    let mut payload = [0];
    let count = tokio::time::timeout(IO_TIMEOUT, stream.read(&mut payload)).await??;
    ensure!(
        count == 0,
        "expected clean TLS shutdown, read {count} bytes"
    );
    Ok(())
}

/// Make the selected first I/O operation fail during a rejected TLS handshake.
async fn rejected_handshake_operation(
    acceptor: TlsAcceptor,
    operation: TerminalOperation,
) -> Result<()> {
    let mut stream = accepted_stream(acceptor).await?;
    ensure!(stream.connection().is_none(), "TLS should still be lazy");
    let result = match operation {
        TerminalOperation::Flush => tokio::time::timeout(IO_TIMEOUT, stream.flush()).await,
        TerminalOperation::Shutdown => tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await,
    };
    ensure!(
        matches!(&result, Ok(Err(_))),
        "rejected handshake should fail through {operation:?}: {result:?}"
    );
    ensure!(
        stream.connection().is_none(),
        "failed TLS must not expose a connection"
    );
    Ok(())
}

/// Send either an invalid record or a TLS peer that does not trust the server.
async fn reject_server_handshake(
    peer: RejectedPeer,
    port: u16,
    untrusted_config: Option<rustls::ClientConfig>,
) -> Result<()> {
    match peer {
        RejectedPeer::MalformedRecord => {
            let mut stream =
                tokio::time::timeout(IO_TIMEOUT, TcpStream::connect(("127.0.0.1", port))).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.write_all(b"not TLS")).await??;
            tokio::time::timeout(IO_TIMEOUT, stream.shutdown()).await??;
        }
        RejectedPeer::UntrustedCertificate => {
            let config =
                untrusted_config.ok_or_else(|| anyhow!("missing untrusted client config"))?;
            let mut connector = client_connector(config, Some("localhost"), HttpProtocol::Http1);
            let uri: Uri = format!("https://127.0.0.1:{port}/").parse()?;
            let result = tokio::time::timeout(IO_TIMEOUT, connector.call(uri)).await;
            ensure!(
                matches!(&result, Ok(Err(_))),
                "client should reject the untrusted server certificate: {result:?}"
            );
        }
    }
    Ok(())
}

/// Verify each first-I/O operation completes TLS and preserves its own contract.
#[rstest]
#[case::flush(TerminalOperation::Flush)]
#[case::shutdown(TerminalOperation::Shutdown)]
#[tokio::test]
async fn public_acceptor_first_io_completes_lazy_handshake(
    tls_material: Result<TlsMaterial>,
    #[case] operation: TerminalOperation,
) -> Result<()> {
    let material = tls_material?;
    let config = client_config(&material, true)?;
    let connector = client_connector(config, Some("localhost"), HttpProtocol::Http1);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = server_acceptor(material, listener, HttpProtocol::Http1)?;
    let server = async {
        match operation {
            TerminalOperation::Flush => flush_first_server_exchange(acceptor).await,
            TerminalOperation::Shutdown => shutdown_first_server(acceptor).await,
        }
    };
    let client = async {
        match operation {
            TerminalOperation::Flush => flush_first_client_exchange(connector, port).await,
            TerminalOperation::Shutdown => client_observes_shutdown(connector, port).await,
        }
    };
    tokio::time::timeout(IO_TIMEOUT, async { tokio::try_join!(server, client) })
        .await
        .map_err(|error| anyhow!("{operation:?}-first exchange timed out: {error}"))??;
    Ok(())
}

/// Check both terminal operations propagate malformed and rejected handshakes.
#[rstest]
#[case::flush_malformed(TerminalOperation::Flush, RejectedPeer::MalformedRecord)]
#[case::shutdown_malformed(TerminalOperation::Shutdown, RejectedPeer::MalformedRecord)]
#[case::flush_untrusted(TerminalOperation::Flush, RejectedPeer::UntrustedCertificate)]
#[case::shutdown_untrusted(TerminalOperation::Shutdown, RejectedPeer::UntrustedCertificate)]
#[tokio::test]
async fn public_acceptor_terminal_io_propagates_rejected_handshakes(
    tls_material: Result<TlsMaterial>,
    #[case] operation: TerminalOperation,
    #[case] peer: RejectedPeer,
) -> Result<()> {
    let material = tls_material?;
    let untrusted_config = match peer {
        RejectedPeer::MalformedRecord => None,
        RejectedPeer::UntrustedCertificate => Some(client_config(&material, false)?),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = server_acceptor(material, listener, HttpProtocol::Http1)?;
    let server = rejected_handshake_operation(acceptor, operation);
    let client = reject_server_handshake(peer, port, untrusted_config);
    tokio::time::timeout(IO_TIMEOUT, async { tokio::try_join!(server, client) })
        .await
        .map_err(|error| anyhow!("rejected handshake case timed out: {error}"))??;
    Ok(())
}
