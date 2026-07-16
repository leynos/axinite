//! HTTP proxy server for sandboxed network access.
//!
//! This proxy runs on the host and handles all network requests from containers.
//! It validates requests against the allowlist and injects credentials when needed.
//!
//! ```text
//! Container ──► http_proxy=host.docker.internal:PORT ──► This Proxy ──► Internet
//!                                                             │
//!                                                             ├─► Validate domain
//!                                                             ├─► Inject credentials
//!                                                             └─► Log requests
//! ```

use core::future::Future;
use core::pin::Pin;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::sandbox::error::{Result, SandboxError};
use crate::sandbox::proxy::policy::NetworkPolicyDecider;

/// State shared across proxy connections.
struct ProxyState {
    /// Policy decider for network requests.
    decider: Arc<dyn NetworkPolicyDecider>,
    /// Credential resolver (maps secret names to values).
    credential_resolver: Arc<dyn CredentialResolver>,
    /// Shared HTTP client for forwarding requests.
    http_client: reqwest::Client,
    /// Request counter for logging.
    request_count: std::sync::atomic::AtomicU64,
    /// Whether the proxy is running.
    running: std::sync::atomic::AtomicBool,
}

/// Boxed future used at the dyn credential-resolver boundary.
pub type CredentialResolverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Resolves secret names to their values.
pub trait CredentialResolver: Send + Sync {
    /// Get the value of a secret by name.
    fn resolve<'a>(&'a self, name: &'a str) -> CredentialResolverFuture<'a, Option<String>>;
}

/// Native async sibling trait for concrete credential-resolver implementations.
pub trait NativeCredentialResolver: Send + Sync {
    /// See [`CredentialResolver::resolve`].
    fn resolve<'a>(&'a self, name: &'a str) -> impl Future<Output = Option<String>> + Send + 'a;
}

impl<T> CredentialResolver for T
where
    T: NativeCredentialResolver + Send + Sync,
{
    fn resolve<'a>(&'a self, name: &'a str) -> CredentialResolverFuture<'a, Option<String>> {
        Box::pin(NativeCredentialResolver::resolve(self, name))
    }
}

/// A credential resolver that uses environment variables.
pub struct EnvCredentialResolver;

impl NativeCredentialResolver for EnvCredentialResolver {
    async fn resolve<'a>(&'a self, name: &'a str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// A credential resolver that returns nothing (for testing).
pub struct NoCredentialResolver;

impl NativeCredentialResolver for NoCredentialResolver {
    async fn resolve<'a>(&'a self, _name: &'a str) -> Option<String> {
        None
    }
}

/// HTTP proxy server.
pub struct HttpProxy {
    state: Arc<ProxyState>,
    addr: RwLock<Option<SocketAddr>>,
    shutdown_tx: RwLock<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl HttpProxy {
    /// Create a new HTTP proxy.
    pub fn new(
        decider: Arc<dyn NetworkPolicyDecider>,
        credential_resolver: Arc<dyn CredentialResolver>,
    ) -> Self {
        Self {
            state: Arc::new(ProxyState {
                decider,
                credential_resolver,
                http_client: reqwest::Client::new(),
                request_count: std::sync::atomic::AtomicU64::new(0),
                running: std::sync::atomic::AtomicBool::new(false),
            }),
            addr: RwLock::new(None),
            shutdown_tx: RwLock::new(None),
        }
    }

    /// Start the proxy server on the given port (0 for auto-assign).
    pub async fn start(&self, port: u16) -> Result<SocketAddr> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
            .await
            .map_err(|e| SandboxError::ProxyError {
                reason: format!("failed to bind: {}", e),
            })?;

        let addr = listener
            .local_addr()
            .map_err(|e| SandboxError::ProxyError {
                reason: format!("failed to get local addr: {}", e),
            })?;

        *self.addr.write().await = Some(addr);

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        self.state
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let state = self.state.clone();

        tokio::spawn(async move {
            tracing::info!("Sandbox proxy started on {}", addr);

            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let state = state.clone();

                                tokio::spawn(async move {
                                    let service = service_fn(move |req| {
                                        let state = state.clone();
                                        async move { handle_request(req, state).await }
                                    });

                                    if let Err(e) = http1::Builder::new()
                                        .preserve_header_case(true)
                                        .title_case_headers(true)
                                        .serve_connection(io, service)
                                        .with_upgrades()
                                        .await
                                    {
                                        tracing::debug!("Proxy connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("Proxy accept error: {}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        tracing::debug!("Sandbox proxy shutting down");
                        break;
                    }
                }
            }

            state
                .running
                .store(false, std::sync::atomic::Ordering::SeqCst);
        });

        Ok(addr)
    }

    /// Stop the proxy server.
    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }
    }

    /// Get the address the proxy is listening on.
    pub async fn addr(&self) -> Option<SocketAddr> {
        *self.addr.read().await
    }

    /// Check if the proxy is running.
    pub fn is_running(&self) -> bool {
        self.state.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the number of requests handled.
    pub fn request_count(&self) -> u64 {
        self.state
            .request_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
mod handlers;

#[cfg(test)]
mod tests;

use handlers::handle_request;
