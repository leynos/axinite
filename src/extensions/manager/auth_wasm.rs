//! WASM tool auth: capabilities, OAuth scopes, and credential resolution.

use crate::extensions::{AuthResult, ExtensionError, ExtensionKind};
use crate::secrets::CreateSecretParams;
use crate::tools::wasm::discover_tools;

use super::ExtensionManager;

/// Candidate sources for a single OAuth credential, tried in priority order:
/// secrets store → inline → env var → builtin default.
pub(super) struct OAuthCredentialSources<'a> {
    /// Setup-tab secret name to look up in the secrets store first.
    pub setup_secret_name: Option<&'a str>,
    /// Inline value from `capabilities.json`.
    pub inline_value: &'a Option<String>,
    /// Runtime environment variable name.
    pub env_var_name: &'a Option<String>,
    /// Built-in default value.
    pub builtin_value: Option<&'a str>,
}

/// Value of the tool's configured env var, when one is declared and set.
fn env_var_token(auth: &crate::tools::wasm::AuthCapabilitySchema) -> Option<String> {
    let env_var = auth.env_var.as_ref()?;
    std::env::var(env_var).ok()
}

/// Instructions for manual token entry when a tool has no OAuth config.
fn manual_token_prompt(name: &str, auth: crate::tools::wasm::AuthCapabilitySchema) -> AuthResult {
    let display = auth.display_name.unwrap_or_else(|| name.to_string());
    let instructions = auth
        .instructions
        .unwrap_or_else(|| format!("Please provide your {} API token/key.", display));

    AuthResult::awaiting_token(name, ExtensionKind::WasmTool, instructions, auth.setup_url)
}

impl ExtensionManager {
    /// Read a WASM tool's auth configuration from its capabilities file.
    ///
    /// Returns `None` when the tool has no capabilities file or declares no
    /// auth section; read and parse failures are propagated.
    async fn load_tool_auth(
        &self,
        name: &str,
    ) -> Result<Option<crate::tools::wasm::AuthCapabilitySchema>, ExtensionError> {
        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));

        if !cap_path.exists() {
            return Ok(None);
        }

        let cap_bytes = tokio::fs::read(&cap_path)
            .await
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        let cap_file = crate::tools::wasm::CapabilitiesFile::from_bytes(&cap_bytes)
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        Ok(cap_file.auth)
    }

    /// Store a credential value for the tool's secret name.
    // Tool name, secret name, and secret value are all free-form identifiers and opaque token text with no invariant a newtype could enforce.
    // @codescene(disable:"String Heavy Function Arguments")
    async fn store_tool_secret(
        &self,
        name: &str,
        secret_name: &str,
        value: &str,
    ) -> Result<(), ExtensionError> {
        let params = CreateSecretParams::new(secret_name, value).with_provider(name.to_string());
        self.secrets
            .create(&self.user_id, params)
            .await
            .map(|_| ())
            .map_err(|e| ExtensionError::AuthFailed(e.to_string()))
    }

    /// Whether a stored token exists and still covers all required OAuth
    /// scopes (scope expansion forces re-auth).
    async fn tool_token_valid(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
    ) -> bool {
        let token_exists = self
            .secrets
            .exists(&self.user_id, &auth.secret_name)
            .await
            .unwrap_or(false);
        if !token_exists {
            return false;
        }

        // If this tool has OAuth config, check whether new scopes are needed
        let Some(ref oauth) = auth.oauth else {
            return true;
        };
        let merged = self
            .collect_shared_scopes(&auth.secret_name, &oauth.scopes)
            .await;
        let needs = self.needs_scope_expansion(&auth.secret_name, &merged).await;
        tracing::debug!(
            tool = name,
            secret_name = %auth.secret_name,
            merged_scopes = ?merged,
            needs_reauth = needs,
            "Scope expansion check"
        );
        !needs
    }

    /// Start the browser-based OAuth flow, or report that client credentials
    /// must be configured in the Setup tab first.
    async fn oauth_or_needs_setup(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
        oauth: &crate::tools::wasm::OAuthConfigSchema,
    ) -> Result<AuthResult, ExtensionError> {
        if self.needs_setup_credentials(name, auth, oauth).await {
            let display = auth.display_name.as_deref().unwrap_or(name);
            return Ok(AuthResult::needs_setup(
                name,
                ExtensionKind::WasmTool,
                format!(
                    "Configure OAuth credentials for {} in the Setup tab.",
                    display
                ),
                auth.setup_url.clone(),
            ));
        }

        self.start_wasm_oauth(name, auth, oauth)
            .await
            .map_err(|e| ExtensionError::AuthFailed(e.to_string()))
    }

    pub(super) async fn auth_wasm_tool(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<AuthResult, ExtensionError> {
        let Some(auth) = self.load_tool_auth(name).await? else {
            return Ok(AuthResult::no_auth_required(name, ExtensionKind::WasmTool));
        };

        // Check env var first: store its value as a secret
        if let Some(value) = env_var_token(&auth) {
            self.store_tool_secret(name, &auth.secret_name, &value)
                .await?;
            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
        }

        // Check if already authenticated (with scope expansion detection);
        // an expired scope set falls through to the OAuth branch below.
        if self.tool_token_valid(name, &auth).await {
            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
        }

        // If a token was provided, store it
        if let Some(token_value) = token {
            self.store_tool_secret(name, &auth.secret_name, token_value)
                .await?;
            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
        }

        // OAuth flow: if the tool has OAuth config, start the browser-based flow.
        // But only if credentials are available — if the tool has setup secrets
        // for client_id/secret that aren't configured yet, return needs_setup.
        if let Some(ref oauth) = auth.oauth {
            return self.oauth_or_needs_setup(name, &auth, oauth).await;
        }

        // Return instructions for manual token entry
        Ok(manual_token_prompt(name, auth))
    }

    /// Load and parse a WASM tool's capabilities file.
    ///
    /// Returns `None` if the file doesn't exist or can't be parsed.
    pub(super) async fn load_tool_capabilities(
        &self,
        name: &str,
    ) -> Option<crate::tools::wasm::CapabilitiesFile> {
        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));
        let cap_bytes = tokio::fs::read(&cap_path).await.ok()?;
        crate::tools::wasm::CapabilitiesFile::from_bytes(&cap_bytes).ok()
    }

    /// Collect merged OAuth scopes from all installed tools sharing the same secret_name.
    ///
    /// When multiple tools share an OAuth provider (e.g., google-calendar and google-drive
    /// both use `google_oauth_token`), we request all their scopes in a single OAuth flow
    /// so one login covers everything.
    pub(super) async fn collect_shared_scopes(
        &self,
        secret_name: &str,
        base_scopes: &[String],
    ) -> Vec<String> {
        let mut all_scopes: std::collections::BTreeSet<String> =
            base_scopes.iter().cloned().collect();

        if let Ok(tools) = discover_tools(&self.wasm_tools_dir).await {
            for tool_name in tools.keys() {
                if let Some(scopes) = self.oauth_scopes_for_secret(tool_name, secret_name).await {
                    all_scopes.extend(scopes);
                }
            }
        }

        all_scopes.into_iter().collect()
    }

    /// OAuth scopes declared by `tool_name` when its auth shares `secret_name`.
    ///
    /// Returns `None` when the tool has no capabilities file, declares no
    /// auth, uses a different secret, or has no OAuth configuration.
    pub(super) async fn oauth_scopes_for_secret(
        &self,
        tool_name: &str,
        secret_name: &str,
    ) -> Option<Vec<String>> {
        let cap = self.load_tool_capabilities(tool_name).await?;
        let auth = cap.auth.as_ref()?;
        if auth.secret_name != secret_name {
            return None;
        }
        let oauth = auth.oauth.as_ref()?;
        Some(oauth.scopes.clone())
    }

    /// Check whether the stored scopes are insufficient for the merged scopes.
    pub(super) async fn needs_scope_expansion(
        &self,
        secret_name: &str,
        merged_scopes: &[String],
    ) -> bool {
        if merged_scopes.is_empty() {
            return false;
        }

        let scopes_key = format!("{}_scopes", secret_name);
        let stored_scopes: std::collections::HashSet<String> =
            match self.secrets.get_decrypted(&self.user_id, &scopes_key).await {
                Ok(secret) => {
                    let scopes: std::collections::HashSet<String> = secret
                        .expose()
                        .split_whitespace()
                        .map(String::from)
                        .collect();
                    tracing::debug!(
                        secret_name,
                        stored_scopes = ?scopes,
                        "Loaded stored scopes for expansion check"
                    );
                    scopes
                }
                Err(_) => {
                    // No stored scopes record — this is a legacy token created before
                    // scope tracking. Force re-auth to ensure all required scopes are granted.
                    tracing::debug!(
                        secret_name,
                        "No stored scopes record, forcing re-auth for legacy token"
                    );
                    return true;
                }
            };

        // Check if any merged scope is missing from stored scopes
        merged_scopes
            .iter()
            .any(|scope| !stored_scopes.contains(scope))
    }

    /// Find the setup secret names for OAuth client_id and client_secret.
    ///
    /// Scans `setup.required_secrets` for names containing "client_id" and "client_secret".
    /// Returns `(Option<(name, optional)>, Option<(name, optional)>)`.
    pub(super) async fn find_setup_credential_names(
        &self,
        tool_name: &str,
    ) -> (Option<(String, bool)>, Option<(String, bool)>) {
        let Some(cap) = self.load_tool_capabilities(tool_name).await else {
            return (None, None);
        };
        let Some(setup) = &cap.setup else {
            return (None, None);
        };

        let mut client_id_entry = None;
        let mut client_secret_entry = None;
        for secret in &setup.required_secrets {
            let lower = secret.name.to_lowercase();
            if lower.ends_with("client_id") || lower == "client_id" {
                client_id_entry = Some((secret.name.clone(), secret.optional));
            } else if lower.ends_with("client_secret") || lower == "client_secret" {
                client_secret_entry = Some((secret.name.clone(), secret.optional));
            }
        }
        (client_id_entry, client_secret_entry)
    }

    /// Check if OAuth client credentials (client_id / client_secret) require
    /// user input via the Setup tab. Returns `true` when at least one required
    /// credential cannot be resolved through the full chain:
    /// secrets store → inline → env var → builtin.
    pub(super) async fn needs_setup_credentials(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
        oauth: &crate::tools::wasm::OAuthConfigSchema,
    ) -> bool {
        let builtin = crate::cli::oauth_defaults::builtin_credentials(&auth.secret_name);
        let (id_entry, secret_entry) = self.find_setup_credential_names(name).await;

        for (entry, inline, env, fallback) in [
            (
                &id_entry,
                &oauth.client_id,
                &oauth.client_id_env,
                builtin.as_ref().map(|c| c.client_id),
            ),
            (
                &secret_entry,
                &oauth.client_secret,
                &oauth.client_secret_env,
                builtin.as_ref().map(|c| c.client_secret),
            ),
        ] {
            let Some((ref setup_name, optional)) = *entry else {
                continue;
            };
            if optional {
                continue;
            }
            let resolved = self
                .resolve_oauth_credential(OAuthCredentialSources {
                    setup_secret_name: Some(setup_name),
                    inline_value: inline,
                    env_var_name: env,
                    builtin_value: fallback,
                })
                .await
                .is_some();
            if !resolved {
                return true;
            }
        }
        false
    }

    /// Resolve an OAuth credential value via: secrets store → inline → env var → builtin.
    ///
    /// For web gateway users, the secrets store is checked first because client_id/secret
    /// may have been entered via the Setup tab (stored as setup secrets).
    pub(super) async fn resolve_oauth_credential(
        &self,
        sources: OAuthCredentialSources<'_>,
    ) -> Option<String> {
        // 1. Check secrets store (entered via Setup tab)
        if let Some(secret_name) = sources.setup_secret_name
            && let Ok(secret) = self.secrets.get_decrypted(&self.user_id, secret_name).await
        {
            let val = secret.expose();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }

        // 2. Inline value from capabilities.json
        if let Some(val) = sources.inline_value {
            return Some(val.clone());
        }

        // 3. Runtime environment variable
        if let Some(env) = sources.env_var_name
            && let Ok(val) = std::env::var(env)
        {
            return Some(val);
        }

        // 4. Built-in defaults
        sources.builtin_value.map(String::from)
    }
}
