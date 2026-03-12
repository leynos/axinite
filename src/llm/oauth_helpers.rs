//! OAuth callback infrastructure used by the NEAR AI session login flow.
//!
//! These utilities (callback server, landing pages, hostname detection) were
//! originally in `cli/oauth_defaults.rs` and are moved here so the `llm`
//! module is self-contained. `cli/oauth_defaults` re-exports everything for
//! backward compatibility.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

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
/// Checks `IRONCLAW_OAUTH_CALLBACK_URL` env var first (useful for remote/VPS
/// deployments where `127.0.0.1` is unreachable from the user's browser),
/// then falls back to `http://{callback_host()}:{OAUTH_CALLBACK_PORT}`.
pub fn callback_url() -> String {
    std::env::var("IRONCLAW_OAUTH_CALLBACK_URL")
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
    let path_prefix = path_prefix.to_string();
    let param_name = param_name.to_string();
    let display_name = display_name.to_string();
    let expected_state = expected_state.map(String::from);

    tokio::time::timeout(Duration::from_secs(300), async move {
        loop {
            let (mut socket, _) = listener
                .accept()
                .await
                .map_err(|e| OAuthCallbackError::Io(e.to_string()))?;

            let mut reader = BufReader::new(&mut socket);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .await
                .map_err(|e| OAuthCallbackError::Io(e.to_string()))?;

            if let Some(path) = request_line.split_whitespace().nth(1)
                && path.starts_with(&path_prefix)
                && let Some(query) = path.split('?').nth(1)
            {
                // Check for error first
                if query.contains("error=") {
                    let html = landing_html(&display_name, false);
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {}",
                        html
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    return Err(OAuthCallbackError::Denied);
                }

                // Parse all query params into a map for validation
                let params: HashMap<&str, String> = query
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
                    .collect();

                // Validate CSRF state parameter
                if let Some(ref expected) = expected_state {
                    let actual = params.get("state").cloned().unwrap_or_default();
                    if actual != *expected {
                        let html = landing_html(&display_name, false);
                        let response = format!(
                            "HTTP/1.1 403 Forbidden\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {}",
                            html
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        return Err(OAuthCallbackError::StateMismatch {
                            expected: expected.clone(),
                            actual,
                        });
                    }
                }

                // Look for the target parameter
                if let Some(value) = params.get(param_name.as_str()) {
                    let html = landing_html(&display_name, true);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {}",
                        html
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;

                    return Ok(value.clone());
                }
            }

            // Not the callback we're looking for
            let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
            let _ = socket.write_all(response.as_bytes()).await;
        }
    })
    .await
    .map_err(|_| OAuthCallbackError::Timeout)?
}

#[cfg(test)]
mod tests {
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
