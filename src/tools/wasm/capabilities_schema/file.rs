//! Root capabilities-file schema: parsing, nested-wrapper resolution,
//! validation warnings, and conversion to runtime [`Capabilities`].

use serde::{Deserialize, Serialize};

use crate::tools::wasm::{
    Capabilities, SecretsCapability, ToolInvokeCapability, WorkspaceCapability,
};

use super::auth::{AuthCapabilitySchema, ToolSetupSchema};
use super::http::HttpCapabilitySchema;
use super::sections::{
    SecretsCapabilitySchema, ToolInvokeCapabilitySchema, WorkspaceCapabilitySchema,
};

/// Root schema for a capabilities JSON file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesFile {
    /// Extension version (semver).
    #[serde(default)]
    pub version: Option<String>,

    /// WIT interface version this extension was compiled against (semver).
    #[serde(default)]
    pub wit_version: Option<String>,

    /// HTTP request capability.
    #[serde(default)]
    pub http: Option<HttpCapabilitySchema>,

    /// Secret existence checks.
    #[serde(default)]
    pub secrets: Option<SecretsCapabilitySchema>,

    /// Tool invocation via aliases.
    #[serde(default)]
    pub tool_invoke: Option<ToolInvokeCapabilitySchema>,

    /// Workspace file read access.
    #[serde(default)]
    pub workspace: Option<WorkspaceCapabilitySchema>,

    /// Authentication setup instructions.
    /// Used by `ironclaw config` to guide users through auth setup.
    #[serde(default)]
    pub auth: Option<AuthCapabilitySchema>,

    /// Setup schema: secrets the user must provide before the tool can be used.
    /// Mirrors the channel `setup.required_secrets` pattern.
    #[serde(default)]
    pub setup: Option<ToolSetupSchema>,

    /// Nested capabilities wrapper for channel-level JSON compatibility.
    ///
    /// Channel capabilities files nest tool capabilities under a `"capabilities"` key.
    /// This allows `from_json()`/`from_bytes()` to also parse channel-level JSON;
    /// inner fields are promoted into top-level fields by `resolve_nested()`.
    /// Always `None` after construction via the public parse methods.
    #[serde(default, skip_serializing)]
    pub capabilities: Option<Box<CapabilitiesFile>>,
}

impl CapabilitiesFile {
    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Self>(json).map(Self::resolve_nested)
    }

    /// Parse from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice::<Self>(bytes).map(Self::resolve_nested)
    }

    /// Merge nested `capabilities` wrapper into top-level fields.
    ///
    /// Channel-level JSON nests tool capabilities under `"capabilities"`.
    /// This promotes the inner fields so callers can access them uniformly.
    fn resolve_nested(mut self) -> Self {
        if let Some(inner) = self.capabilities.take() {
            let inner = inner.resolve_nested();
            self.http = self.http.or(inner.http);
            self.secrets = self.secrets.or(inner.secrets);
            self.tool_invoke = self.tool_invoke.or(inner.tool_invoke);
            self.workspace = self.workspace.or(inner.workspace);
            self.auth = self.auth.or(inner.auth);
            self.setup = self.setup.or(inner.setup);
        }
        self
    }

    /// Validate the capabilities file and emit warnings for common misconfigurations.
    ///
    /// Called once at load time to catch issues early. Warnings are emitted via
    /// `tracing::warn` so they show up in startup logs without blocking loading.
    pub fn validate(&self, name: &str) {
        const MIN_PROMPT_LENGTH: usize = 30;

        // setup.required_secrets present but no auth section → auth card won't display
        if let Some(setup) = &self.setup {
            if !setup.required_secrets.is_empty() && self.auth.is_none() {
                tracing::warn!(
                    tool = name,
                    "setup.required_secrets defined but no 'auth' section — \
                     chat-based auth card will not display for this tool"
                );
            }

            // Check for short prompts
            for secret in &setup.required_secrets {
                if secret.prompt.len() < MIN_PROMPT_LENGTH {
                    tracing::warn!(
                        tool = name,
                        secret = secret.name,
                        prompt = secret.prompt,
                        "setup.required_secrets prompt is shorter than {} chars — \
                         consider a more descriptive prompt that tells the user where to find this value",
                        MIN_PROMPT_LENGTH
                    );
                }
            }
        }

        // Manual auth (no OAuth) checks
        if let Some(auth) = &self.auth
            && auth.oauth.is_none()
        {
            if auth.setup_url.is_none() {
                tracing::warn!(
                    tool = name,
                    "auth section has no OAuth and no setup_url — \
                     user has no link to obtain credentials"
                );
            }
            if auth.instructions.is_none() {
                tracing::warn!(
                    tool = name,
                    "auth section has no OAuth and no instructions — \
                     user has no guidance on how to obtain credentials"
                );
            }
        }
    }

    /// Convert to runtime Capabilities.
    pub fn to_capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::default();

        if let Some(http) = &self.http {
            caps.http = Some(http.to_http_capability());
        }

        if let Some(secrets) = &self.secrets {
            caps.secrets = Some(SecretsCapability {
                allowed_names: secrets.allowed_names.clone(),
            });
        }

        if let Some(tool_invoke) = &self.tool_invoke {
            caps.tool_invoke = Some(ToolInvokeCapability {
                aliases: tool_invoke.aliases.clone(),
                rate_limit: tool_invoke
                    .rate_limit
                    .as_ref()
                    .map(|r| r.to_rate_limit_config())
                    .unwrap_or_default(),
            });
        }

        if let Some(workspace) = &self.workspace {
            caps.workspace_read = Some(WorkspaceCapability {
                allowed_prefixes: workspace.allowed_prefixes.clone(),
                reader: None, // Injected at runtime
            });
        }

        caps
    }
}
