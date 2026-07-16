//! Browser OAuth flow for WASM tools (callback listener and token exchange).

use std::sync::Arc;

use crate::cli::oauth_defaults;
use crate::extensions::{AuthResult, ExtensionKind};

use super::ExtensionManager;
use super::PendingAuth;
use super::auth_wasm::OAuthCredentialSources;
use super::oauth_launch::{
    OAuthLaunchPlan, broadcast_auth_result, complete_local_oauth, missing_client_id_error,
    rewrite_state_for_platform,
};

impl ExtensionManager {
    /// Start the OAuth browser flow for a WASM tool.
    ///
    /// Resolves credentials, builds the authorization URL, then dispatches to
    /// either the gateway or the local TCP-listener launch branch, returning the
    /// auth URL immediately so the web UI can open it.
    pub(super) async fn start_wasm_oauth(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
        oauth: &crate::tools::wasm::OAuthConfigSchema,
    ) -> Result<AuthResult, String> {
        let (client_id, client_secret) = self.resolve_oauth_credentials(name, auth, oauth).await?;

        self.cancel_pending_auth(name).await;

        let redirect_uri = format!("{}/callback", oauth_defaults::callback_url());

        // Merge scopes from all tools sharing this provider
        let merged_scopes = self
            .collect_shared_scopes(&auth.secret_name, &oauth.scopes)
            .await;

        // Build authorization URL with CSRF state
        let oauth_result = oauth_defaults::build_oauth_url(
            &oauth.authorization_url,
            &client_id,
            &redirect_uri,
            &merged_scopes,
            oauth.use_pkce,
            &oauth.extra_params,
        )
        .map_err(|e| e.to_string())?;

        let display_name = auth
            .display_name
            .clone()
            .unwrap_or_else(|| name.to_string());

        let plan = OAuthLaunchPlan {
            name: name.to_string(),
            display_name,
            redirect_uri,
            merged_scopes,
            client_id,
            client_secret,
            code_verifier: oauth_result.code_verifier,
            expected_state: oauth_result.state,
            auth_url: oauth_result.url,
            token_url: oauth.token_url.clone(),
            access_token_field: oauth.access_token_field.clone(),
            secret_name: auth.secret_name.clone(),
            provider: auth.provider.clone(),
            validation_endpoint: auth.validation_endpoint.clone(),
        };

        if oauth_defaults::use_gateway_callback() {
            Ok(self.launch_gateway_oauth(plan).await)
        } else {
            self.launch_local_oauth(plan).await
        }
    }

    /// Resolve the OAuth `client_id` (required) and `client_secret` (optional
    /// for PKCE-only flows), consulting setup secrets, inline config, env vars,
    /// and builtin defaults in turn.
    async fn resolve_oauth_credentials(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
        oauth: &crate::tools::wasm::OAuthConfigSchema,
    ) -> Result<(String, Option<String>), String> {
        let builtin = oauth_defaults::builtin_credentials(&auth.secret_name);

        // Find setup secret names for client_id and client_secret from capabilities.
        // These are the actual names used in the Setup tab (e.g., "google_oauth_client_id"),
        // which may differ from "{secret_name}_client_id".
        let (setup_client_id_entry, setup_client_secret_entry) =
            self.find_setup_credential_names(name).await;
        let setup_client_id_name = setup_client_id_entry.map(|(n, _)| n);
        let setup_client_secret_name = setup_client_secret_entry.map(|(n, _)| n);

        // Resolve client_id: setup secrets → inline → env var → builtin
        let client_id = self
            .resolve_oauth_credential(OAuthCredentialSources {
                setup_secret_name: setup_client_id_name.as_deref(),
                inline_value: &oauth.client_id,
                env_var_name: &oauth.client_id_env,
                builtin_value: builtin.as_ref().map(|c| c.client_id),
            })
            .await
            .ok_or_else(|| missing_client_id_error(name, auth, oauth))?;

        // Resolve client_secret (optional for PKCE-only flows)
        let client_secret = self
            .resolve_oauth_credential(OAuthCredentialSources {
                setup_secret_name: setup_client_secret_name.as_deref(),
                inline_value: &oauth.client_secret,
                env_var_name: &oauth.client_secret_env,
                builtin_value: builtin.as_ref().map(|c| c.client_secret),
            })
            .await;

        Ok((client_id, client_secret))
    }

    /// Cancel any in-flight OAuth for `name`: abort a pending local listener
    /// task (freeing port 9876) and drop any gateway-mode pending flows.
    async fn cancel_pending_auth(&self, name: &str) {
        {
            let mut pending = self.pending_auth.write().await;
            if let Some(old) = pending.remove(name)
                && let Some(handle) = old.task_handle
            {
                handle.abort();
            }
        }
        let mut flows = self.pending_oauth_flows.write().await;
        flows.retain(|_, flow| flow.extension_name != name);
    }

    /// Launch the gateway-mode flow: register a pending flow keyed by the CSRF
    /// nonce for the web gateway's `/oauth/callback` handler to complete. No TCP
    /// listener is needed — the OAuth provider redirects to the gateway URL.
    async fn launch_gateway_oauth(&self, plan: OAuthLaunchPlan) -> AuthResult {
        oauth_defaults::sweep_expired_flows(&self.pending_oauth_flows).await;

        // Wrap the CSRF nonce with instance name for platform routing.
        // Nginx at auth.DOMAIN parses `instance:nonce` to route the callback
        // to the correct container. The flow is keyed by the raw nonce.
        let auth_url = rewrite_state_for_platform(&plan.auth_url, &plan.expected_state);

        let flow = oauth_defaults::PendingOAuthFlow {
            extension_name: plan.name.clone(),
            display_name: plan.display_name,
            token_url: plan.token_url,
            client_id: plan.client_id,
            client_secret: plan.client_secret,
            redirect_uri: plan.redirect_uri,
            code_verifier: plan.code_verifier,
            access_token_field: plan.access_token_field,
            secret_name: plan.secret_name,
            provider: plan.provider,
            validation_endpoint: plan.validation_endpoint,
            scopes: plan.merged_scopes,
            user_id: self.user_id.clone(),
            secrets: Arc::clone(&self.secrets),
            sse_sender: self.sse_sender.read().await.clone(),
            gateway_token: self.gateway_token.clone(),
            created_at: std::time::Instant::now(),
        };

        // Key by raw nonce (without instance prefix) — the callback handler
        // strips the prefix before lookup.
        self.pending_oauth_flows
            .write()
            .await
            .insert(plan.expected_state, flow);

        // Register pending auth without a task handle (gateway handles completion)
        self.register_pending_auth(&plan.name, None).await;

        AuthResult::awaiting_authorization(
            &plan.name,
            ExtensionKind::WasmTool,
            auth_url,
            "gateway".to_string(),
        )
    }

    /// Launch the local flow: bind port 9876 and spawn a background task that
    /// waits for the callback, exchanges the code, and stores the tokens. This
    /// is the original flow for local/desktop use.
    async fn launch_local_oauth(&self, plan: OAuthLaunchPlan) -> Result<AuthResult, String> {
        let listener = oauth_defaults::bind_callback_listener()
            .await
            .map_err(|e| format!("Failed to start OAuth callback listener: {}", e))?;

        // Capture the values needed after the spawn before `plan` moves into the task.
        let ext_name = plan.name.clone();
        let auth_url = plan.auth_url.clone();
        let secrets = Arc::clone(&self.secrets);
        let user_id = self.user_id.clone();
        let sse_sender = self.sse_sender.read().await.clone();

        let task_handle = tokio::spawn(async move {
            let result = complete_local_oauth(listener, &plan, secrets, &user_id).await;
            broadcast_auth_result(sse_sender, &plan.name, &plan.display_name, result);
        });

        self.register_pending_auth(&ext_name, Some(task_handle))
            .await;

        Ok(AuthResult::awaiting_authorization(
            &ext_name,
            ExtensionKind::WasmTool,
            auth_url,
            "local".to_string(),
        ))
    }

    /// Record a pending auth entry for `name`, with an optional listener task
    /// handle (present only for the local flow).
    async fn register_pending_auth(
        &self,
        name: &str,
        task_handle: Option<tokio::task::JoinHandle<()>>,
    ) {
        self.pending_auth.write().await.insert(
            name.to_string(),
            PendingAuth {
                _name: name.to_string(),
                _kind: ExtensionKind::WasmTool,
                created_at: std::time::Instant::now(),
                task_handle,
            },
        );
    }

    /// Returns `true` if a setup secret is an OAuth credential (client_id or client_secret)
    /// that can be resolved without user input — via inline capabilities, env var, or
    /// builtin defaults.
    ///
    /// Used by `check_tool_auth_status()` and `get_setup_schema()` to hide setup fields
    /// that the user doesn't need to fill (e.g., Google tools with builtin credentials).
    pub(super) fn is_auto_resolved_oauth_field(
        secret_name: &str,
        cap_file: &crate::tools::wasm::CapabilitiesFile,
    ) -> bool {
        let Some(field) = classify_oauth_field(secret_name) else {
            return false;
        };
        let Some(ref auth) = cap_file.auth else {
            return false;
        };
        let Some(ref oauth) = auth.oauth else {
            return false;
        };
        let has_builtin =
            crate::cli::oauth_defaults::builtin_credentials(&auth.secret_name).is_some();

        match field {
            OAuthField::ClientId => {
                credential_auto_resolved(&oauth.client_id, &oauth.client_id_env, has_builtin)
            }
            OAuthField::ClientSecret => credential_auto_resolved(
                &oauth.client_secret,
                &oauth.client_secret_env,
                has_builtin,
            ),
        }
    }
}

/// OAuth client credential fields that may be auto-resolved.
enum OAuthField {
    ClientId,
    ClientSecret,
}

/// Classify a setup secret name as an OAuth client credential field, if it
/// is one. (`ends_with` also covers the exact-match case.)
fn classify_oauth_field(secret_name: &str) -> Option<OAuthField> {
    let lower = secret_name.to_lowercase();
    if lower.ends_with("client_id") {
        return Some(OAuthField::ClientId);
    }
    if lower.ends_with("client_secret") {
        return Some(OAuthField::ClientSecret);
    }
    None
}

/// Whether an OAuth credential can be resolved without user input: inline
/// value, resolvable environment variable, or builtin default.
fn credential_auto_resolved(
    inline: &Option<String>,
    env_var: &Option<String>,
    has_builtin: bool,
) -> bool {
    if inline.is_some() || has_builtin {
        return true;
    }
    env_var.as_ref().is_some_and(|e| std::env::var(e).is_ok())
}
