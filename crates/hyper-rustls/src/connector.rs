//! Route Hyper sockets through verified TLS while preserving connection metadata.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{fmt, io};

use hyper::client::connect::Connection;
use hyper::service::Service;
use hyper::Uri;
use pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsConnector;

use crate::stream::MaybeHttpsStream;

pub(crate) mod builder;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ConnectFuture<T> = Pin<Box<dyn Future<Output = Result<MaybeHttpsStream<T>, BoxError>> + Send>>;

/// Validate the scheme and peer identity before invoking the socket connector.
fn connection_server(
    uri: &Uri,
    force_https: bool,
    override_name: Option<&str>,
) -> io::Result<Option<ServerName<'static>>> {
    match (uri.scheme_str(), force_https) {
        (Some("http"), false) => Ok(None),
        (Some("https"), _) => tls_server_name(uri, override_name).map(Some),
        (Some(scheme), _) => Err(io::Error::other(format!("unsupported scheme {scheme}"))),
        (None, _) => Err(io::Error::other("missing scheme")),
    }
}

/// Select the override or URI host and normalize bracketed IPv6 literals.
///
/// Invalid TLS names are returned as `InvalidInput` errors.
fn tls_server_name(uri: &Uri, override_name: Option<&str>) -> io::Result<ServerName<'static>> {
    let hostname = override_name.unwrap_or_else(|| uri.host().unwrap_or_default());
    // URI authorities wrap IPv6 literals in brackets; certificate names do not.
    let hostname = hostname
        .strip_prefix('[')
        .and_then(|name| name.strip_suffix(']'))
        .unwrap_or(hostname);
    ServerName::try_from(hostname.to_owned())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// A Connector for the `https` scheme.
#[derive(Clone)]
pub struct HttpsConnector<T> {
    force_https: bool,
    http: T,
    tls_config: Arc<rustls::ClientConfig>,
    override_server_name: Option<String>,
}

impl<T> HttpsConnector<T> {
    /// Force the use of HTTPS when connecting.
    ///
    /// If a URL is not `https` when connecting, an error is returned.
    pub fn enforce_https(&mut self) {
        self.force_https = true;
    }
}

impl<T> Service<Uri> for HttpsConnector<T>
where
    T: Service<Uri>,
    T::Response: Connection + AsyncRead + AsyncWrite + Send + Unpin + 'static,
    T::Future: Send + 'static,
    T::Error: Into<BoxError>,
{
    type Response = MaybeHttpsStream<T::Response>;
    type Error = BoxError;

    type Future = ConnectFuture<T::Response>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.http
            .poll_ready(cx)
            .map_err(Into::into)
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let server =
            match connection_server(&dst, self.force_https, self.override_server_name.as_deref()) {
                Ok(server) => server,
                Err(error) => return Box::pin(async move { Err(error.into()) }),
            };
        let config = self.tls_config.clone();
        let connecting = self.http.call(dst);
        Box::pin(async move {
            let socket = connecting.await.map_err(Into::into)?;
            match server {
                Some(name) => Ok(MaybeHttpsStream::Https(
                    TlsConnector::from(config)
                        .connect(name, socket)
                        .await?,
                )),
                None => Ok(MaybeHttpsStream::Http(socket)),
            }
        })
    }
}

impl<H, C> From<(H, C)> for HttpsConnector<H>
where
    C: Into<Arc<rustls::ClientConfig>>,
{
    fn from((http, cfg): (H, C)) -> Self {
        Self {
            force_https: false,
            http,
            tls_config: cfg.into(),
            override_server_name: None,
        }
    }
}

impl<T> fmt::Debug for HttpsConnector<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("HttpsConnector")
            .field("force_https", &self.force_https)
            .finish()
    }
}

#[cfg(all(test, feature = "native-tokio", feature = "http1", feature = "ring"))]
mod tests;

#[cfg(all(
    test,
    feature = "native-tokio",
    feature = "http1",
    feature = "ring",
    feature = "webpki-roots"
))]
mod provider_tests;
