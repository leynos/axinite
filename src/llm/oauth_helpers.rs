//! OAuth callback infrastructure used by the NEAR AI session login flow.
//!
//! These utilities (callback server, landing pages, hostname detection) were
//! originally in `cli/oauth_defaults.rs` and are moved here so the `llm`
//! module is self-contained. `cli/oauth_defaults` re-exports everything for
//! backward compatibility.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

mod oauth_landing;

pub use oauth_landing::landing_html;

/// Fixed port for the OAuth callback listener.
pub const OAUTH_CALLBACK_PORT: u16 = 9876;

/// Error from the OAuth callback listener.
#[derive(Debug, thiserror::Error)]
pub enum OAuthCallbackError {
    #[error("Port {0} is in use (another auth flow running?): {1}")]
    PortInUse(u16, String),

    #[error("Authorization denied by user")]
    Denied,

    #[error("Timed out waiting for authorization")]
    Timeout,

    #[error("CSRF state mismatch: expected {expected}, got {actual}")]
    StateMismatch { expected: String, actual: String },

    #[error("IO error: {0}")]
    Io(String),
}

/// Returns the OAuth callback base URL.
///
/// Checks `AXINITE_OAUTH_CALLBACK_URL` env var first (useful for remote/VPS
/// deployments where `127.0.0.1` is unreachable from the user's browser),
/// then falls back to `http://{callback_host()}:{OAUTH_CALLBACK_PORT}`.
pub fn callback_url() -> String {
    std::env::var("AXINITE_OAUTH_CALLBACK_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("http://{}:{}", callback_host(), OAUTH_CALLBACK_PORT))
}

/// Returns the hostname used in OAuth callback URLs.
///
/// Reads `OAUTH_CALLBACK_HOST` from the environment (default: `127.0.0.1`).
///
/// **Remote server usage:** set `OAUTH_CALLBACK_HOST` to the specific network
/// interface address you want to listen on (e.g. the server's LAN IP).
/// Wildcard addresses (`0.0.0.0`, `::`) are rejected — use a specific interface
/// IP to limit exposure. The callback listener will bind to that address so the
/// OAuth redirect can reach an external browser.
/// Note: this transmits the session token over plain HTTP — prefer SSH port
/// forwarding (`ssh -L 9876:127.0.0.1:9876 user@host`) when possible.
pub fn callback_host() -> String {
    std::env::var("OAUTH_CALLBACK_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Returns `true` if `host` is a loopback address that only accepts local connections.
///
/// Covers `localhost` (case-insensitive), the full `127.0.0.0/8` IPv4 loopback
/// range, and `::1` for IPv6.
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Returns `true` if `host` is a wildcard/unspecified address (`0.0.0.0` or `::`).
///
/// Wildcard binds accept connections on all interfaces, which is a security risk
/// for OAuth callbacks that carry session tokens over plain HTTP.
fn is_wildcard_host(host: &str) -> bool {
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_unspecified())
        .unwrap_or(false)
}

/// Map a `std::io::Error` from a bind attempt to an `OAuthCallbackError`.
fn bind_error(e: std::io::Error) -> OAuthCallbackError {
    if e.kind() == std::io::ErrorKind::AddrInUse {
        OAuthCallbackError::PortInUse(OAUTH_CALLBACK_PORT, e.to_string())
    } else {
        OAuthCallbackError::Io(e.to_string())
    }
}

/// Bind the OAuth callback listener on the fixed port.
///
/// When `OAUTH_CALLBACK_HOST` is a loopback address, binds to the configured
/// loopback host first. IPv6 loopback hosts also get a bracketed fallback so
/// callback URLs and listener binds stay aligned.
///
/// When `OAUTH_CALLBACK_HOST` is set to a remote address, binds to that
/// specific address so only connections directed to it are accepted.
pub async fn bind_callback_listener() -> Result<TcpListener, OAuthCallbackError> {
    bind_callback_listener_for_host(&callback_host()).await
}

async fn bind_callback_listener_for_host(host: &str) -> Result<TcpListener, OAuthCallbackError> {
    if is_wildcard_host(host) {
        return Err(OAuthCallbackError::Io(format!(
            "OAUTH_CALLBACK_HOST={host} is a wildcard address — this would accept \
             connections on all interfaces, exposing the session token. \
             Use a specific interface IP (e.g. 192.168.1.x) or SSH port forwarding instead."
        )));
    }

    if is_loopback_host(host) {
        let preferred_addr = format!("{host}:{}", OAUTH_CALLBACK_PORT);
        match TcpListener::bind(&preferred_addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(OAuthCallbackError::PortInUse(
                    OAUTH_CALLBACK_PORT,
                    e.to_string(),
                ));
            }
            Err(e) => {
                if host.parse::<std::net::Ipv6Addr>().is_err() {
                    return Err(bind_error(e));
                }
            }
        }

        let ipv6_addr = format!("[{host}]:{}", OAUTH_CALLBACK_PORT);
        TcpListener::bind(ipv6_addr).await.map_err(bind_error)
    } else if host.contains(':') {
        let addr = format!("[{host}]:{}", OAUTH_CALLBACK_PORT);
        TcpListener::bind(addr).await.map_err(bind_error)
    } else {
        let addr = format!("{host}:{}", OAUTH_CALLBACK_PORT);
        TcpListener::bind(addr).await.map_err(bind_error)
    }
}

/// What a `wait_for_callback` listener should match and how it should brand its
/// landing pages.
struct CallbackSpec {
    /// Request path the callback must start with (e.g. "/callback").
    path_prefix: String,
    /// Query parameter whose value is returned (e.g. "code" or "token").
    param_name: String,
    /// Provider name shown on the branded landing page.
    display_name: String,
    /// Expected CSRF `state`; when set, mismatches are rejected.
    expected_state: Option<String>,
}

/// Wait for an OAuth callback and extract a query parameter value.
///
/// Listens for a GET request matching `path_prefix` (e.g., "/callback" or "/auth/callback"),
/// extracts the value of `param_name` (e.g., "code" or "token"), and shows a branded
/// landing page using `display_name` (e.g., "Google", "Notion", "NEAR AI").
///
/// When `expected_state` is `Some`, the callback's `state` query parameter is validated
/// against it to prevent CSRF attacks. If the state doesn't match, the callback is
/// rejected with an error page.
///
/// Times out after 5 minutes.
pub async fn wait_for_callback(
    listener: TcpListener,
    path_prefix: &str,
    param_name: &str,
    display_name: &str,
    expected_state: Option<&str>,
) -> Result<String, OAuthCallbackError> {
    let spec = CallbackSpec {
        path_prefix: path_prefix.to_string(),
        param_name: param_name.to_string(),
        display_name: display_name.to_string(),
        expected_state: expected_state.map(String::from),
    };

    tokio::time::timeout(Duration::from_secs(300), async move {
        loop {
            if let Some(value) = serve_one_callback(&listener, &spec).await? {
                return Ok(value);
            }
        }
    })
    .await
    .map_err(|_| OAuthCallbackError::Timeout)?
}

/// Accept and handle a single connection.
///
/// Returns `Ok(Some(value))` once the target parameter is captured, `Ok(None)`
/// for requests that are not the awaited callback (a 404 is written and the
/// caller keeps listening), and `Err` for user denial, CSRF mismatch, or IO
/// failure.
async fn serve_one_callback(
    listener: &TcpListener,
    spec: &CallbackSpec,
) -> Result<Option<String>, OAuthCallbackError> {
    let (mut socket, _) = listener
        .accept()
        .await
        .map_err(|e| OAuthCallbackError::Io(e.to_string()))?;

    let request_line = read_request_line(&mut socket).await?;

    let Some(query) = callback_query(&request_line, &spec.path_prefix) else {
        write_not_found(&mut socket).await;
        return Ok(None);
    };

    if query.contains("error=") {
        write_landing_page(&mut socket, "400 Bad Request", &spec.display_name, false).await;
        return Err(OAuthCallbackError::Denied);
    }

    let params = parse_query_params(query);

    if let Some(ref expected) = spec.expected_state {
        let actual = params.get("state").cloned().unwrap_or_default();
        if actual != *expected {
            write_landing_page(&mut socket, "403 Forbidden", &spec.display_name, false).await;
            return Err(OAuthCallbackError::StateMismatch {
                expected: expected.clone(),
                actual,
            });
        }
    }

    if let Some(value) = params.get(spec.param_name.as_str()) {
        write_landing_page(&mut socket, "200 OK", &spec.display_name, true).await;
        let _ = socket.shutdown().await;
        return Ok(Some(value.clone()));
    }

    // Matched the path but not the target parameter — not our callback.
    write_not_found(&mut socket).await;
    Ok(None)
}

/// Read the HTTP request line (the first line) from `socket`.
async fn read_request_line(socket: &mut TcpStream) -> Result<String, OAuthCallbackError> {
    let mut reader = BufReader::new(socket);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| OAuthCallbackError::Io(e.to_string()))?;
    Ok(request_line)
}

/// Decode an `&`-separated query string into a map, percent-decoding values.
fn parse_query_params(query: &str) -> HashMap<&str, String> {
    query
        .split('&')
        .filter_map(|p| {
            let mut parts = p.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((
                key,
                urlencoding::decode(val)
                    .unwrap_or_else(|_| val.into())
                    .into_owned(),
            ))
        })
        .collect()
}

/// Write a branded landing page with the given HTTP status line.
async fn write_landing_page(
    socket: &mut TcpStream,
    status_line: &str,
    display_name: &str,
    success: bool,
) {
    let html = landing_html(display_name, success);
    let response = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status_line, html
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

/// Write a bare `404 Not Found` response for requests that are not the callback.
async fn write_not_found(socket: &mut TcpStream) {
    let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
    let _ = socket.write_all(response.as_bytes()).await;
}

/// Extract the query string from a request line whose path matches the
/// expected callback prefix; `None` for any other request.
fn callback_query<'a>(request_line: &'a str, path_prefix: &str) -> Option<&'a str> {
    let path = request_line.split_whitespace().nth(1)?;
    if !path.starts_with(path_prefix) {
        return None;
    }
    path.split('?').nth(1)
}

#[cfg(test)]
mod tests {
    //! Unit tests for OAuth loopback detection and callback listener
    //! binding.

    use rstest::rstest;

    use super::*;

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.2")); // full 127.0.0.0/8 range
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(!is_loopback_host("::"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn wildcard_detection() {
        assert!(is_wildcard_host("0.0.0.0"));
        assert!(is_wildcard_host("::"));
        assert!(!is_wildcard_host("127.0.0.1"));
        assert!(!is_wildcard_host("192.168.1.1"));
        assert!(!is_wildcard_host("::1"));
        assert!(!is_wildcard_host("localhost"));
    }

    #[rstest]
    #[case("0.0.0.0")]
    #[case("::")]
    #[tokio::test]
    async fn bind_rejects_wildcard_hosts(#[case] host: &str) {
        let err = bind_callback_listener_for_host(host)
            .await
            .expect_err("wildcard hosts must be rejected");
        assert!(
            err.to_string().contains("wildcard"),
            "error should mention wildcard: {err}"
        );
    }
}
