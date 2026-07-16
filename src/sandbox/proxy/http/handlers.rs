//! Request handling for the sandbox HTTP proxy.
//!
//! Validates proxied requests against the network policy, injects
//! credentials, forwards plain HTTP requests, tunnels CONNECT traffic,
//! and provides response-building helpers.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::sandbox::proxy::policy::{NetworkDecision, NetworkRequest};
use crate::secrets::CredentialLocation;

use super::ProxyState;

/// Handle an incoming proxy request.
pub(super) async fn handle_request(
    req: Request<hyper::body::Incoming>,
    state: Arc<ProxyState>,
) -> std::result::Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    state
        .request_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // Handle CONNECT method for HTTPS tunnelling
    if req.method() == Method::CONNECT {
        return Ok(handle_connect(req, state).await);
    }

    // For HTTP requests, validate and forward
    let uri = req.uri().to_string();
    let method = req.method().to_string();

    let network_req = match NetworkRequest::from_url(&method, &uri) {
        Some(r) => r,
        None => {
            tracing::warn!("Proxy: invalid URL: {}", uri);
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "Invalid URL".to_string(),
            ));
        }
    };

    // Make policy decision
    let decision = state.decider.decide(&network_req).await;

    match decision {
        NetworkDecision::Deny { reason } => {
            tracing::info!("Proxy: blocked {} {} - {}", method, uri, reason);
            Ok(error_response(StatusCode::FORBIDDEN, reason))
        }
        NetworkDecision::Allow | NetworkDecision::AllowWithCredentials { .. } => {
            // Forward the request
            forward_request(req, decision, state).await
        }
    }
}

/// Handle CONNECT method for HTTPS tunnelling.
///
/// Establishes a bidirectional TCP tunnel between the client and the target host.
/// Returns 200 OK to signal the client to begin TLS over the upgraded connection.
///
/// NOTE: Credential injection is not possible through CONNECT tunnels since the proxy
/// cannot inspect or modify TLS-encrypted traffic without MITM. Containers that need
/// authenticated HTTPS should fetch credentials via the orchestrator's
/// `GET /worker/{id}/credentials` endpoint and set them as environment variables.
async fn handle_connect(
    req: Request<hyper::body::Incoming>,
    state: Arc<ProxyState>,
) -> Response<BoxBody<Bytes, Infallible>> {
    // Extract host:port from CONNECT target (e.g. "api.github.com:443")
    let authority = match req.uri().authority() {
        Some(a) => a.clone(),
        None => {
            return error_response(StatusCode::BAD_REQUEST, "Missing host".to_string());
        }
    };

    let host = authority.host().to_string();
    let target_addr = authority.as_str().to_string();

    // Check if host is allowed
    let network_req = NetworkRequest {
        method: "CONNECT".to_string(),
        url: format!("https://{}", host),
        host: host.clone(),
        path: "/".to_string(),
    };

    let decision = state.decider.decide(&network_req).await;

    if let NetworkDecision::Deny { reason } = decision {
        tracing::info!("Proxy: blocked CONNECT {} - {}", host, reason);
        return error_response(StatusCode::FORBIDDEN, reason);
    }

    tracing::debug!("Proxy: allowing CONNECT to {}", target_addr);

    // Spawn a fire-and-forget task to establish the tunnel after the upgrade
    // completes.  The 30-minute timeout guarantees every tunnel task terminates
    // even if the remote peer hangs, so no `JoinSet` tracking is needed.
    // On process exit these tasks are dropped by the runtime.
    let target = target_addr.clone();
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut client_stream = TokioIo::new(upgraded);
                match TcpStream::connect(&target).await {
                    Ok(mut server_stream) => {
                        let tunnel_timeout = std::time::Duration::from_secs(30 * 60);
                        match tokio::time::timeout(
                            tunnel_timeout,
                            tokio::io::copy_bidirectional(&mut client_stream, &mut server_stream),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => {
                                tracing::debug!("Proxy: tunnel to {} closed: {}", target, e);
                            }
                            Err(_) => {
                                tracing::info!(
                                    "Proxy: tunnel to {} timed out after 30m, closing",
                                    target
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Proxy: failed to connect to {}: {}", target, e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Proxy: upgrade failed for {}: {}", target, e);
            }
        }
    });

    // Return 200 OK so the client begins the TLS handshake over the upgraded connection
    make_response(StatusCode::OK, empty_body())
}

/// Forward a request to the target server.
async fn forward_request(
    req: Request<hyper::body::Incoming>,
    decision: NetworkDecision,
    state: Arc<ProxyState>,
) -> std::result::Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    let mut builder = base_forward_builder(&state, &req);
    builder = inject_credentials(builder, decision, &state).await;

    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            tracing::error!("Proxy: failed to read request body: {}", e);
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read body".to_string(),
            ));
        }
    };

    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes.to_vec());
    }

    Ok(send_and_relay(builder).await)
}

/// Start building the forwarded request: method, URI, and all headers
/// except hop-by-hop ones.
fn base_forward_builder(
    state: &ProxyState,
    req: &Request<hyper::body::Incoming>,
) -> reqwest::RequestBuilder {
    let mut builder = state.http_client.request(
        reqwest::Method::from_bytes(req.method().as_str().as_bytes())
            .unwrap_or(reqwest::Method::GET),
        req.uri().to_string(),
    );

    for (name, value) in req.headers() {
        if !is_hop_by_hop_header(name.as_str())
            && let Ok(v) = value.to_str()
        {
            builder = builder.header(name.as_str(), v);
        }
    }

    builder
}

/// Inject credentials into the forwarded request when the policy decision
/// requires them. Missing credentials are logged and skipped.
async fn inject_credentials(
    builder: reqwest::RequestBuilder,
    decision: NetworkDecision,
    state: &ProxyState,
) -> reqwest::RequestBuilder {
    let NetworkDecision::AllowWithCredentials {
        secret_name,
        location,
    } = decision
    else {
        return builder;
    };

    let Some(credential) = state.credential_resolver.resolve(&secret_name).await else {
        tracing::warn!("Proxy: credential {} not found", secret_name);
        return builder;
    };

    let builder = apply_credential_location(builder, location, credential);
    tracing::debug!("Proxy: injected credential for {}", secret_name);
    builder
}

/// Place a resolved credential at its configured location on the request.
fn apply_credential_location(
    builder: reqwest::RequestBuilder,
    location: CredentialLocation,
    credential: String,
) -> reqwest::RequestBuilder {
    match location {
        CredentialLocation::AuthorizationBearer => {
            builder.header("Authorization", format!("Bearer {}", credential))
        }
        CredentialLocation::Header { name, prefix } => {
            let value = match prefix {
                Some(p) => format!("{}{}", p, credential),
                None => credential.clone(),
            };
            builder.header(name, value)
        }
        CredentialLocation::QueryParam { name } => builder.query(&[(name, credential)]),
        // Known limitation: AuthorizationBasic requires the proxy to
        // construct a Base64 username:password pair from a single secret,
        // and UrlPath requires rewriting the request URI. Neither is
        // implemented yet. Containers needing these auth styles should
        // fetch credentials via the orchestrator's GET /worker/{id}/credentials
        // endpoint and set them directly.
        CredentialLocation::AuthorizationBasic { .. } | CredentialLocation::UrlPath { .. } => {
            tracing::warn!(
                "Proxy: credential location {:?} not supported for forward proxy, skipping",
                location
            );
            builder
        }
    }
}

/// Send the forwarded request and relay the upstream response, translating
/// failures into gateway errors.
async fn send_and_relay(builder: reqwest::RequestBuilder) -> Response<BoxBody<Bytes, Infallible>> {
    let response = match builder.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("Proxy: request failed: {}", e);
            return error_response(StatusCode::BAD_GATEWAY, format!("Request failed: {}", e));
        }
    };

    let status = response.status();
    let headers = response.headers().clone();

    let body = match response.bytes().await {
        Ok(body) => body,
        Err(e) => {
            tracing::error!("Proxy: failed to read response body: {}", e);
            return error_response(
                StatusCode::BAD_GATEWAY,
                "Failed to read response".to_string(),
            );
        }
    };

    let mut resp_builder = Response::builder().status(status.as_u16());
    for (name, value) in headers.iter() {
        if !is_hop_by_hop_header(name.as_str()) {
            resp_builder = resp_builder.header(name.as_str(), value.as_bytes());
        }
    }

    make_response_from_builder(resp_builder, full_body(body))
}

/// Check if a header is hop-by-hop (should not be forwarded).
pub(super) fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// Build a response with guaranteed success (valid status + simple body cannot fail).
pub(super) fn make_response(
    status: StatusCode,
    body: BoxBody<Bytes, Infallible>,
) -> Response<BoxBody<Bytes, Infallible>> {
    Response::builder()
        .status(status)
        .body(body)
        .unwrap_or_else(|_| {
            let mut resp = Response::new(
                Full::new(Bytes::from("Internal error"))
                    .map_err(|_| unreachable!())
                    .boxed(),
            );
            *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            resp
        })
}

/// Finalize a partially-built response, falling back to 500 on builder error.
fn make_response_from_builder(
    builder: hyper::http::response::Builder,
    body: BoxBody<Bytes, Infallible>,
) -> Response<BoxBody<Bytes, Infallible>> {
    builder.body(body).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(full_body(Bytes::from("Response build error")))
            .unwrap_or_else(|_| {
                Response::new(
                    Full::new(Bytes::from("Internal error"))
                        .map_err(|_| unreachable!())
                        .boxed(),
                )
            })
    })
}

/// Create an error response.
pub(super) fn error_response(
    status: StatusCode,
    message: String,
) -> Response<BoxBody<Bytes, Infallible>> {
    make_response_from_builder(
        Response::builder()
            .status(status)
            .header("Content-Type", "text/plain"),
        full_body(Bytes::from(message)),
    )
}

/// Create an empty body.
pub(super) fn empty_body() -> BoxBody<Bytes, Infallible> {
    Empty::<Bytes>::new().map_err(|_| unreachable!()).boxed()
}

/// Create a body from bytes.
fn full_body(bytes: Bytes) -> BoxBody<Bytes, Infallible> {
    Full::new(bytes).map_err(|_| unreachable!()).boxed()
}
