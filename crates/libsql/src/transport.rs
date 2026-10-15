//! Adapt libSQL's Hyper 0.14 connector boundary to the patched rustls stack.
//!
//! The published SDK's TLS feature used rustls 0.22. This adapter preserves its
//! native roots, HTTP/HTTPS routing, empty ALPN and TCP_NODELAY behaviour while
//! keeping certificate and hostname verification in rustls.

mod stream;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::client::HttpConnector;
use hyper::Uri;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::TlsConnector;
use tower::Service;

use self::stream::Stream;

/// TLS transport supplied to the SDK's built-in HTTP connector factory.
#[derive(Clone)]
pub(crate) struct Connector {
    http: HttpConnector,
    tls: TlsConnector,
}

impl Connector {
    /// Load platform roots without installing a process-global crypto provider.
    pub(crate) fn new() -> io::Result<Self> {
        let native = rustls_native_certs::load_native_certs();
        let roots = native_root_store(native.certs).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{error}; native root errors: {:?}", native.errors),
            )
        })?;
        Self::with_roots(roots)
    }

    fn with_roots(roots: rustls::RootCertStore) -> io::Result<Self> {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        // The published HTTP/1-only connector leaves ALPN protocols empty.
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(true);
        Ok(Self {
            http,
            tls: TlsConnector::from(Arc::new(config)),
        })
    }
}

/// Keep usable platform roots even when other entries cannot be parsed.
fn native_root_store(
    certificates: impl IntoIterator<Item = CertificateDer<'static>>,
) -> io::Result<rustls::RootCertStore> {
    let mut roots = rustls::RootCertStore::empty();
    let (_, invalid) = roots.add_parsable_certificates(certificates);
    if roots.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no usable native TLS certificate roots ({invalid} invalid entries)"),
        ));
    }
    Ok(roots)
}

/// Choose TLS before opening the socket, so unsupported schemes cannot downgrade.
fn server_name(uri: &Uri) -> io::Result<Option<ServerName<'static>>> {
    match uri.scheme_str() {
        Some("http") => Ok(None),
        Some("https") => {
            let host = uri.host().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "HTTPS URI has no hostname")
            })?;
            let host = host
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(host);
            ServerName::try_from(host.to_owned())
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "libSQL transport requires an HTTP or HTTPS URI",
        )),
    }
}

impl Service<Uri> for Connector {
    type Response = Stream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = io::Result<Stream>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.http.poll_ready(cx).map_err(io::Error::other)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let server = server_name(&uri);
        let mut http = self.http.clone();
        let tls = self.tls.clone();
        Box::pin(async move {
            let server = server?;
            let socket = http.call(uri).await.map_err(io::Error::other)?;
            match server {
                Some(name) => tls.connect(name, socket).await.map(Stream::tls),
                None => Ok(Stream::Http(socket)),
            }
        })
    }
}

#[cfg(test)]
mod tests;
