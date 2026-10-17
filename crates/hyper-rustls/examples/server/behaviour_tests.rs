//! Exercise the example's echo routes through a generated loopback TLS peer.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use hyper::body::to_bytes;
use hyper::client;
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, StatusCode, Uri};
use hyper_rustls::HttpsConnectorBuilder;
use rstest::rstest;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::echo;
use crate::support::tls_test::tls_material;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Describe the status and payload returned by one example route.
struct ExpectedResponse {
    status: StatusCode,
    body: &'static str,
}

/// Serve the actual endpoint and check each public route over HTTPS.
#[rstest]
#[case::greeting(Method::GET, "/", "", ExpectedResponse {
    status: StatusCode::OK,
    body: "Try POST /echo\n",
})]
#[case::echo(Method::POST, "/echo", "loopback request", ExpectedResponse {
    status: StatusCode::OK,
    body: "loopback request",
})]
#[case::not_found(Method::GET, "/unknown", "", ExpectedResponse {
    status: StatusCode::NOT_FOUND,
    body: "",
})]
#[tokio::test]
async fn echo_routes_work_over_https(
    #[case] method: Method,
    #[case] path: &str,
    #[case] request_body: &str,
    #[case] expected: ExpectedResponse,
) -> Result<()> {
    let material = tls_material()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server_config = Arc::clone(&material.server);
    let server = async move {
        let (socket, _) = listener.accept().await?;
        let stream = TlsAcceptor::from(server_config)
            .accept(socket)
            .await?;
        Http::new()
            .http1_keep_alive(false)
            .serve_connection(stream, service_fn(echo))
            .await?;
        Ok::<(), anyhow::Error>(())
    };
    let request = async {
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(material.client)
            .https_or_http()
            .enable_http1()
            .build();
        let client: client::Client<_, Body> = client::Client::builder().build(connector);
        let uri = format!("https://localhost:{}{path}", address.port()).parse::<Uri>()?;
        let request = Request::builder()
            .method(method.clone())
            .uri(uri)
            .body(Body::from(request_body.to_owned()))?;
        let response = client.request(request).await?;
        let status = response.status();
        let body = String::from_utf8(
            to_bytes(response.into_body())
                .await?
                .to_vec(),
        )?;
        Ok::<_, anyhow::Error>((status, body))
    };

    let ((), (status, body)) =
        tokio::time::timeout(IO_TIMEOUT, async { tokio::try_join!(server, request) }).await??;

    assert_eq!(
        status, expected.status,
        "HTTPS {method} {path} returned an unexpected status"
    );
    assert_eq!(
        body, expected.body,
        "HTTPS {method} {path} returned an unexpected body"
    );
    Ok(())
}
