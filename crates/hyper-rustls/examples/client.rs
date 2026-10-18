//! Fetch one URL over HTTP or HTTPS with an optional PEM trust store.

use std::{env, error::Error, io, sync::Arc};

use cap_std::fs::File;
use hyper::{body::to_bytes, client, Body, Uri};
use hyper_rustls::ConfigBuilderExt;
use rustls::{crypto::ring, ClientConfig, RootCertStore};

mod support;

use support::read_certificates;

/// Start the current-thread runtime used by the HTTP client example.
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_client())
}

/// Parse the URL and optional CA path, then print the response from one request.
async fn run_client() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut output = io::stdout();
    run_client_with_arguments(env::args().skip(1), &mut output).await
}

/// Parse the command-line arguments and write one response to `output`.
async fn run_client_with_arguments(
    mut arguments: impl Iterator<Item = String>,
    output: &mut (impl io::Write + Send),
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: client <url> [ca_store]",
        )
    })?;
    let url = url.parse::<Uri>()?;
    let tls = client_config(arguments.next().as_deref())?;

    request_and_render(url, tls, output).await
}

/// Fetch a URL and write the response status, headers and body.
async fn request_and_render(
    url: Uri,
    tls: ClientConfig,
    output: &mut (impl io::Write + Send),
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .build();
    let client: client::Client<_, Body> = client::Client::builder().build(connector);
    let response = client.get(url).await?;

    writeln!(output, "Status:\n{}", response.status())?;
    writeln!(output, "Headers:\n{:#?}", response.headers())?;

    let body = to_bytes(response.into_body()).await?;
    writeln!(output, "Body:\n{}", String::from_utf8_lossy(&body))?;
    Ok(())
}

/// Build client trust from the selected PEM file or the platform's native
/// roots.
fn client_config(
    custom_ca_store: Option<&str>,
) -> Result<ClientConfig, Box<dyn Error + Send + Sync>> {
    let provider = Arc::new(ring::default_provider());
    let builder =
        ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions()?;

    let config = if let Some(path) = custom_ca_store {
        // The user selected this PEM path; ambient access preserves its usual
        // absolute or working-directory-relative command-line meaning.
        let file = File::open_ambient(path, cap_std::ambient_authority())?;
        let certificates = read_certificates(file)?;
        let mut roots = RootCertStore::empty();
        let (valid, _) = roots.add_parsable_certificates(certificates);
        if valid == 0 {
            return Err(io::Error::other(format!(
                "the PEM trust store {path} contains no usable certificates"
            ))
            .into());
        }
        builder
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        builder
            .with_native_roots()?
            .with_no_client_auth()
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    //! Verify trust-store errors and rendered output from a local HTTPS peer.

    use std::{io, net::SocketAddr, sync::Arc, time::Duration};

    use hyper::server::conn::Http;
    use hyper::service::service_fn;
    use hyper::{Body, Response, StatusCode, Uri};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::{client_config, request_and_render, run_client_with_arguments};
    use crate::support::tls_test::tls_material;

    const IO_TIMEOUT: Duration = Duration::from_secs(5);

    /// A PEM trust store containing a certificate can configure the client.
    #[test]
    fn sample_pem_trust_store_loads() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        client_config(Some(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/sample.pem"
        )))?;
        Ok(())
    }

    /// Reject selected PEM files that contain no usable certificate anchors.
    #[rstest::rstest]
    #[case::private_key("examples/sample.rsa")]
    #[case::empty("examples/support/empty.pem")]
    fn unusable_pem_trust_store_is_rejected(#[case] relative_path: &str) {
        let path = format!("{}/{relative_path}", env!("CARGO_MANIFEST_DIR"));
        let error = client_config(Some(&path))
            .expect_err("PEM input without certificates cannot supply trust anchors");

        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::Other)
        );
    }

    /// Preserve malformed-certificate PEM as an `InvalidData` I/O error.
    #[test]
    fn malformed_pem_retains_the_io_error_type() {
        let error = client_config(Some(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/support/malformed-certificate.pem"
        )))
        .expect_err("malformed PEM must fail before a request is made");
        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::InvalidData)
        );
    }

    /// Report the response from a trusted loopback HTTPS server in CLI format.
    #[tokio::test]
    async fn request_renders_status_headers_and_body_over_https() -> anyhow::Result<()> {
        let material = tls_material()?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address: SocketAddr = listener.local_addr()?;
        let server_config = Arc::clone(&material.server);
        let tls = material.client;
        let server = async move {
            let (socket, _) = listener.accept().await?;
            let stream = TlsAcceptor::from(server_config)
                .accept(socket)
                .await?;
            let service = service_fn(|_| async {
                let mut response = Response::new(Body::from("served over HTTPS"));
                *response.status_mut() = StatusCode::CREATED;
                response.headers_mut().insert(
                    "x-loopback",
                    hyper::header::HeaderValue::from_static("test-value"),
                );
                Ok::<_, hyper::Error>(response)
            });
            Http::new()
                .http1_keep_alive(false)
                .serve_connection(stream, service)
                .await?;
            Ok::<_, anyhow::Error>(())
        };
        let uri = format!("https://localhost:{}/", address.port()).parse::<Uri>()?;
        let mut output = Vec::new();
        let request = async {
            request_and_render(uri, tls, &mut output)
                .await
                .map_err(anyhow::Error::from_boxed)
        };

        tokio::time::timeout(IO_TIMEOUT, async {
            tokio::try_join!(server, request)?;
            Ok::<_, anyhow::Error>(())
        })
        .await??;

        let output = String::from_utf8(output)?;
        assert!(
            output.contains("Status:\n201 Created"),
            "rendered output should include the HTTPS response status: {output:?}"
        );
        assert!(
            output
                .to_ascii_lowercase()
                .contains("x-loopback"),
            "rendered output should include response headers: {output:?}"
        );
        assert!(
            output.contains("test-value"),
            "rendered output should preserve the response header value: {output:?}"
        );
        assert!(
            output.contains("Body:\nserved over HTTPS"),
            "rendered output should include the response body: {output:?}"
        );
        Ok(())
    }

    /// Report a missing URL and unusable trust store through the argument path.
    #[tokio::test]
    async fn invalid_arguments_and_trust_store_return_input_errors() {
        let mut output = Vec::new();
        let missing_url = run_client_with_arguments(std::iter::empty(), &mut output)
            .await
            .expect_err("a URL is required before a request can be made");
        assert_eq!(
            missing_url
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::InvalidInput),
            "the missing URL should retain the command-line input error"
        );

        let malformed_store = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/support/malformed-certificate.pem"
        );
        let arguments = ["https://localhost/".to_owned(), malformed_store.to_owned()];
        let invalid_store = run_client_with_arguments(arguments.into_iter(), &mut output)
            .await
            .expect_err("malformed trust-store PEM must fail before connecting");
        assert_eq!(
            invalid_store
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::InvalidData),
            "malformed trust-store PEM should retain the invalid-data error"
        );
        assert!(
            output.is_empty(),
            "argument errors should not render a response"
        );
    }
}
