//! WASM tool auth: capabilities, OAuth scopes, and credential resolution.

use crate::extensions::{AuthResult, ExtensionError, ExtensionKind};
use crate::secrets::CreateSecretParams;
use crate::tools::wasm::discover_tools;

use super::ExtensionManager;

impl ExtensionManager {
    pub(super) async fn auth_wasm_tool(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<AuthResult, ExtensionError> {
        // Read the capabilities file to get auth config
        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));

        if !cap_path.exists() {
            return Ok(AuthResult::no_auth_required(name, ExtensionKind::WasmTool));
        }

        let cap_bytes = tokio::fs::read(&cap_path)
            .await
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        let cap_file = crate::tools::wasm::CapabilitiesFile::from_bytes(&cap_bytes)
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        let auth = match cap_file.auth {
            Some(auth) => auth,
            None => {
                return Ok(AuthResult::no_auth_required(name, ExtensionKind::WasmTool));
            }
        };

        // Check env var first
        if let Some(ref env_var) = auth.env_var
            && let Ok(value) = std::env::var(env_var)
        {
            // Store the env var value as a secret
            let params =
                CreateSecretParams::new(&auth.secret_name, &value).with_provider(name.to_string());
            self.secrets
                .create(&self.user_id, params)
                .await
                .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;

            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
        }

        // Check if already authenticated (with scope expansion detection)
        let token_exists = self
            .secrets
            .exists(&self.user_id, &auth.secret_name)
            .await
            .unwrap_or(false);

        if token_exists {
            // If this tool has OAuth config, check whether new scopes are needed
            let needs_reauth = if let Some(ref oauth) = auth.oauth {
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
                needs
            } else {
                false
            };

            if !needs_reauth {
                return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
            }
            // Fall through to OAuth branch for scope expansion
        }

        // If a token was provided, store it
        if let Some(token_value) = token {
            let params = CreateSecretParams::new(&auth.secret_name, token_value)
                .with_provider(name.to_string());
            self.secrets
                .create(&self.user_id, params)
                .await
                .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;

            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmTool));
        }

        // OAuth flow: if the tool has OAuth config, start the browser-based flow.
        // But only if credentials are available — if the tool has setup secrets
        // for client_id/secret that aren't configured yet, return needs_setup.
        if let Some(ref oauth) = auth.oauth {
            if self.needs_setup_credentials(name, &auth, oauth).await {
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

            return self
                .start_wasm_oauth(name, &auth, oauth)
                .await
                .map_err(|e| ExtensionError::AuthFailed(e.to_string()));
        }

        // Return instructions for manual token entry
        let display = auth.display_name.unwrap_or_else(|| name.to_string());
        let instructions = auth
            .instructions
            .unwrap_or_else(|| format!("Please provide your {} API token/key.", display));

        Ok(AuthResult::awaiting_token(
            name,
            ExtensionKind::WasmTool,
            instructions,
            auth.setup_url,
        ))
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
                .resolve_oauth_credential(inline, env, fallback, Some(setup_name))
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
        inline_value: &Option<String>,
        env_var_name: &Option<String>,
        builtin_value: Option<&str>,
        setup_secret_name: Option<&str>,
    ) -> Option<String> {
        // 1. Check secrets store (entered via Setup tab)
        if let Some(secret_name) = setup_secret_name
            && let Ok(secret) = self.secrets.get_decrypted(&self.user_id, secret_name).await
        {
            let val = secret.expose();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }

        // 2. Inline value from capabilities.json
        if let Some(val) = inline_value {
            return Some(val.clone());
        }

        // 3. Runtime environment variable
        if let Some(env) = env_var_name
            && let Ok(val) = std::env::var(env)
        {
            return Some(val);
        }

        // 4. Built-in defaults
        builtin_value.map(String::from)
    }
}
