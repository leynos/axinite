//! MCP server and transport configuration types: transport variants,
//! per-server settings, OAuth configuration, and validation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ConfigError;

/// Transport configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpTransportConfig {
    /// HTTP/HTTPS transport (uses the `url` field on McpServerConfig).
    Http,
    /// Stdio transport — spawns a child process.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// Unix domain socket transport.
    Unix { socket_path: String },
}

/// Configuration for connecting to a remote MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique name for this server (e.g., "notion", "github").
    pub name: String,

    /// Server URL (must be HTTPS for remote servers).
    pub url: String,

    /// Transport configuration. If `None`, defaults to Http using `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransportConfig>,

    /// Custom headers to include in every HTTP request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,

    /// OAuth configuration (if server requires authentication).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthConfig>,

    /// Whether this server is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Optional description for the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub(super) fn default_true() -> bool {
    true
}

impl McpServerConfig {
    /// Create a new MCP server configuration.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            transport: None,
            headers: HashMap::new(),
            oauth: None,
            enabled: true,
            description: None,
        }
    }

    /// Create a new stdio transport MCP server configuration.
    pub fn new_stdio(
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Self {
        Self {
            name: name.into(),
            url: String::new(),
            transport: Some(McpTransportConfig::Stdio {
                command: command.into(),
                args,
                env,
            }),
            headers: HashMap::new(),
            oauth: None,
            enabled: true,
            description: None,
        }
    }

    /// Create a new Unix socket transport MCP server configuration.
    pub fn new_unix(name: impl Into<String>, socket_path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: String::new(),
            transport: Some(McpTransportConfig::Unix {
                socket_path: socket_path.into(),
            }),
            headers: HashMap::new(),
            oauth: None,
            enabled: true,
            description: None,
        }
    }

    /// Set OAuth configuration.
    pub fn with_oauth(mut self, oauth: OAuthConfig) -> Self {
        self.oauth = Some(oauth);
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set custom headers.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Get the effective transport type.
    pub fn effective_transport(&self) -> EffectiveTransport<'_> {
        match &self.transport {
            Some(McpTransportConfig::Http) | None => EffectiveTransport::Http,
            Some(McpTransportConfig::Stdio { command, args, env }) => {
                EffectiveTransport::Stdio { command, args, env }
            }
            Some(McpTransportConfig::Unix { socket_path }) => {
                EffectiveTransport::Unix { socket_path }
            }
        }
    }

    /// Validate the server configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        require_non_empty(&self.name, "Server name cannot be empty")?;

        match self.effective_transport() {
            EffectiveTransport::Http => self.validate_http_transport(),
            EffectiveTransport::Stdio { command, .. } => {
                require_non_empty(command, "Stdio transport command cannot be empty")
            }
            EffectiveTransport::Unix { socket_path } => {
                require_non_empty(socket_path, "Unix socket path cannot be empty")
            }
        }
    }

    /// Validate HTTP transport settings: the URL must be present, and remote
    /// servers must use HTTPS (localhost is allowed for development).
    fn validate_http_transport(&self) -> Result<(), ConfigError> {
        require_non_empty(&self.url, "Server URL cannot be empty")?;

        let url_lower = self.url.to_lowercase();
        let is_localhost = url_lower.contains("localhost") || url_lower.contains("127.0.0.1");
        if !is_localhost && !url_lower.starts_with("https://") {
            return Err(ConfigError::InvalidConfig {
                reason: "Remote MCP servers must use HTTPS".to_string(),
            });
        }

        Ok(())
    }

    /// Check if this server requires authentication.
    ///
    /// Returns true if OAuth is pre-configured OR if this is a remote HTTPS server
    /// (which likely supports Dynamic Client Registration even without pre-configured OAuth).
    ///
    /// Non-HTTP transports (stdio, unix) never require auth.
    pub fn requires_auth(&self) -> bool {
        // Non-HTTP transports don't use HTTP auth
        if !matches!(self.effective_transport(), EffectiveTransport::Http) {
            return false;
        }

        if self.oauth.is_some() {
            return true;
        }
        // Remote HTTPS servers need auth handling (DCR, token refresh, 401 detection).
        // Localhost/127.0.0.1 servers are assumed to be dev servers without auth.
        let url_lower = self.url.to_lowercase();
        let is_localhost = is_localhost_url(&url_lower);
        url_lower.starts_with("https://") && !is_localhost
    }

    /// Get the secret name used to store the access token.
    pub fn token_secret_name(&self) -> String {
        format!("mcp_{}_access_token", self.name)
    }

    /// Get the secret name used to store the refresh token.
    pub fn refresh_token_secret_name(&self) -> String {
        format!("mcp_{}_refresh_token", self.name)
    }

    /// Get the secret name used to store the DCR client ID.
    pub fn client_id_secret_name(&self) -> String {
        format!("mcp_{}_client_id", self.name)
    }
}

/// Require a non-empty configuration value, reporting `reason` otherwise.
fn require_non_empty(value: &str, reason: &str) -> Result<(), ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::InvalidConfig {
            reason: reason.to_string(),
        });
    }
    Ok(())
}

/// OAuth 2.1 configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// OAuth client ID.
    pub client_id: String,

    /// Authorization endpoint URL.
    /// If not provided, will be discovered from /.well-known/oauth-protected-resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,

    /// Token endpoint URL.
    /// If not provided, will be discovered from /.well-known/oauth-authorization-server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,

    /// Scopes to request.
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Whether to use PKCE (default: true, as required by OAuth 2.1).
    #[serde(default = "default_true")]
    pub use_pkce: bool,

    /// Extra parameters to include in the authorization request.
    #[serde(default)]
    pub extra_params: HashMap<String, String>,
}

impl OAuthConfig {
    /// Create a new OAuth configuration with just a client ID.
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            authorization_url: None,
            token_url: None,
            scopes: Vec::new(),
            use_pkce: true,
            extra_params: HashMap::new(),
        }
    }

    /// Set authorization and token URLs.
    pub fn with_endpoints(
        mut self,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        self.authorization_url = Some(authorization_url.into());
        self.token_url = Some(token_url.into());
        self
    }

    /// Set scopes.
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }
}

/// Check if a URL points to a loopback address (localhost, 127.0.0.1, [::1]).
///
/// Uses `url::Url` for proper parsing so edge cases (IPv6, userinfo, ports)
/// are handled correctly without manual string splitting.
pub(super) fn is_localhost_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Resolved transport type (borrows from config).
#[derive(Debug)]
pub enum EffectiveTransport<'a> {
    Http,
    Stdio {
        command: &'a str,
        args: &'a [String],
        env: &'a HashMap<String, String>,
    },
    Unix {
        socket_path: &'a str,
    },
}
