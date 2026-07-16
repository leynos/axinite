//! `McpClient` construction, accessors, and cloning.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::RwLock;

use crate::secrets::SecretsStore;
use crate::tools::mcp::config::McpServerConfig;
use crate::tools::mcp::http_transport::HttpMcpTransport;
use crate::tools::mcp::protocol::McpTool;
use crate::tools::mcp::session::McpSessionManager;
use crate::tools::mcp::transport::McpTransport;

/// MCP client for communicating with MCP servers.
///
/// Supports multiple transport types:
/// - HTTP: For remote MCP servers (created via `new`, `new_with_name`, `new_authenticated`)
/// - Stdio/Unix: Via `new_with_transport` with a custom `McpTransport` implementation
pub struct McpClient {
    /// Transport for sending requests.
    pub(super) transport: Arc<dyn McpTransport>,

    /// Server URL (kept for accessor compatibility).
    pub(super) server_url: String,

    /// Server name (for logging and session management).
    pub(super) server_name: String,

    /// Request ID counter.
    pub(super) next_id: AtomicU64,

    /// Cached tools.
    pub(super) tools_cache: RwLock<Option<Vec<McpTool>>>,

    /// Session manager (shared across clients).
    pub(super) session_manager: Option<Arc<McpSessionManager>>,

    /// Secrets store for retrieving access tokens.
    pub(super) secrets: Option<Arc<dyn SecretsStore + Send + Sync>>,

    /// User ID for secrets lookup.
    pub(super) user_id: String,

    /// Server configuration (for token secret name lookup).
    pub(super) server_config: Option<McpServerConfig>,

    /// Custom headers to include in every request.
    pub(super) custom_headers: HashMap<String, String>,
}

/// Construction inputs for [`McpClient::new_with_transport`].
///
/// Groups the transport, identity, and optional auth collaborators so the
/// constructor does not take an excess of positional arguments.
pub struct TransportClientOptions {
    /// Server name for logging and session management.
    pub server_name: String,
    /// Transport implementation (stdio, UDS, or other non-HTTP).
    pub transport: Arc<dyn McpTransport>,
    /// Shared session manager, when sessions are in use.
    pub session_manager: Option<Arc<McpSessionManager>>,
    /// Secrets store for retrieving access tokens, when authentication applies.
    pub secrets: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// User ID for secrets lookup.
    pub user_id: String,
    /// Server configuration, when one is available.
    pub server_config: Option<McpServerConfig>,
}

/// Grouped construction inputs for [`McpClient`].
///
/// Collects the fields that vary between constructors so each `new_*`
/// variant only spells out what differs from the unauthenticated default.
struct McpClientParts {
    transport: Arc<dyn McpTransport>,
    server_url: String,
    server_name: String,
    session_manager: Option<Arc<McpSessionManager>>,
    secrets: Option<Arc<dyn SecretsStore + Send + Sync>>,
    user_id: String,
    server_config: Option<McpServerConfig>,
    custom_headers: HashMap<String, String>,
}

impl McpClientParts {
    /// Parts for an unauthenticated HTTP client with default identity.
    fn http_unauthenticated(server_name: String, server_url: String) -> Self {
        let transport = Arc::new(HttpMcpTransport::new(
            server_url.clone(),
            server_name.clone(),
        ));
        Self {
            transport,
            server_url,
            server_name,
            session_manager: None,
            secrets: None,
            user_id: "default".to_string(),
            server_config: None,
            custom_headers: HashMap::new(),
        }
    }
}

impl From<McpClientParts> for McpClient {
    fn from(parts: McpClientParts) -> Self {
        Self {
            transport: parts.transport,
            server_url: parts.server_url,
            server_name: parts.server_name,
            next_id: AtomicU64::new(1),
            tools_cache: RwLock::new(None),
            session_manager: parts.session_manager,
            secrets: parts.secrets,
            user_id: parts.user_id,
            server_config: parts.server_config,
            custom_headers: parts.custom_headers,
        }
    }
}

impl McpClient {
    /// Create a new simple MCP client (no authentication).
    ///
    /// Use this for local development servers or servers that don't require auth.
    pub fn new(server_url: impl Into<String>) -> Self {
        let url: String = server_url.into();
        let name = extract_server_name(&url);
        McpClientParts::http_unauthenticated(name, url).into()
    }

    /// Create a new simple MCP client with a specific name.
    ///
    /// Use this when you have a configured server name but no authentication.
    pub fn new_with_name(server_name: impl Into<String>, server_url: impl Into<String>) -> Self {
        McpClientParts::http_unauthenticated(server_name.into(), server_url.into()).into()
    }

    /// Create a new simple MCP client from an HTTP server configuration (no authentication).
    ///
    /// Use this when you have an `McpServerConfig` with custom headers but no OAuth.
    /// The config must use HTTP transport (the default); for stdio/UDS use `new_with_transport`.
    pub fn new_with_config(config: McpServerConfig) -> Self {
        assert!(
            matches!(
                config.effective_transport(),
                crate::tools::mcp::config::EffectiveTransport::Http
            ),
            "new_with_config only supports HTTP transport; use new_with_transport for stdio/UDS"
        );
        let mut parts =
            McpClientParts::http_unauthenticated(config.name.clone(), config.url.clone());
        parts.custom_headers = config.headers.clone();
        parts.server_config = Some(config);
        parts.into()
    }

    /// Create a new authenticated MCP client.
    ///
    /// Use this for hosted MCP servers that require OAuth authentication.
    pub fn new_authenticated(
        config: McpServerConfig,
        session_manager: Arc<McpSessionManager>,
        secrets: Arc<dyn SecretsStore + Send + Sync>,
        user_id: impl Into<String>,
    ) -> Self {
        let transport = Arc::new(
            HttpMcpTransport::new(config.url.clone(), config.name.clone())
                .with_session_manager(session_manager.clone()),
        );

        McpClientParts {
            transport,
            server_url: config.url.clone(),
            server_name: config.name.clone(),
            session_manager: Some(session_manager),
            secrets: Some(secrets),
            user_id: user_id.into(),
            custom_headers: config.headers.clone(),
            server_config: Some(config),
        }
        .into()
    }

    /// Create a new MCP client with a custom transport.
    ///
    /// Use this for stdio, UDS, or other non-HTTP transports.
    pub fn new_with_transport(options: TransportClientOptions) -> Self {
        let server_url = options
            .server_config
            .as_ref()
            .map(|c| c.url.clone())
            .unwrap_or_default();
        let custom_headers = options
            .server_config
            .as_ref()
            .map(|c| c.headers.clone())
            .unwrap_or_default();

        McpClientParts {
            transport: options.transport,
            server_url,
            server_name: options.server_name,
            session_manager: options.session_manager,
            secrets: options.secrets,
            user_id: options.user_id,
            server_config: options.server_config,
            custom_headers,
        }
        .into()
    }

    /// Get the server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Get the server URL.
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Get the next request ID.
    pub(super) fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl Clone for McpClient {
    fn clone(&self) -> Self {
        Self {
            transport: self.transport.clone(),
            server_url: self.server_url.clone(),
            server_name: self.server_name.clone(),
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
            tools_cache: RwLock::new(None),
            session_manager: self.session_manager.clone(),
            secrets: self.secrets.clone(),
            user_id: self.user_id.clone(),
            server_config: self.server_config.clone(),
            custom_headers: self.custom_headers.clone(),
        }
    }
}

/// Extract a server name from a URL for logging/display purposes.
pub(super) fn extract_server_name(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
        .replace('.', "_")
}
