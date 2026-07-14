//! Browser OAuth flow for WASM tools (callback listener and token exchange).

use std::sync::Arc;

use crate::extensions::{AuthResult, ExtensionKind};

use super::ExtensionManager;
use super::PendingAuth;

impl ExtensionManager {
    /// Start the OAuth browser flow for a WASM tool.
    ///
    /// Binds a callback listener, builds the authorization URL, spawns a background
    /// task to wait for the callback and exchange the code, then returns the auth URL
    /// immediately so the web UI can open it.
    pub(super) async fn start_wasm_oauth(
        &self,
        name: &str,
        auth: &crate::tools::wasm::AuthCapabilitySchema,
        oauth: &crate::tools::wasm::OAuthConfigSchema,
    ) -> Result<AuthResult, String> {
        use crate::cli::oauth_defaults;

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
            .resolve_oauth_credential(
                &oauth.client_id,
                &oauth.client_id_env,
                builtin.as_ref().map(|c| c.client_id),
                setup_client_id_name.as_deref(),
            )
            .await
            .ok_or_else(|| {
                let env_name = oauth
                    .client_id_env
                    .as_deref()
                    .unwrap_or("the client_id env var");
                let mut msg = format!(
                    "OAuth client_id not configured for '{}'. \
                     Enter it in the Setup tab or set {} env var",
                    name, env_name
                );
                // Only mention the Google-specific build flag for Google providers
                if auth.secret_name.to_lowercase().contains("google") {
                    msg.push_str(", or build with IRONCLAW_GOOGLE_CLIENT_ID");
                }
                msg.push('.');
                msg
            })?;

        // Resolve client_secret (optional for PKCE-only flows)
        let client_secret = self
            .resolve_oauth_credential(
                &oauth.client_secret,
                &oauth.client_secret_env,
                builtin.as_ref().map(|c| c.client_secret),
                setup_client_secret_name.as_deref(),
            )
            .await;

        // Cancel any existing pending auth for this tool (frees port 9876 in TCP mode)
        {
            let mut pending = self.pending_auth.write().await;
            if let Some(old) = pending.remove(name)
                && let Some(handle) = old.task_handle
            {
                handle.abort();
            }
        }
        // Also clean up any gateway-mode pending flows for this tool
        {
            let mut flows = self.pending_oauth_flows.write().await;
            flows.retain(|_, flow| flow.extension_name != name);
        }

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
        let auth_url = oauth_result.url.clone();
        let code_verifier = oauth_result.code_verifier;
        let expected_state = oauth_result.state;

        let display_name = auth
            .display_name
            .clone()
            .unwrap_or_else(|| name.to_string());

        if oauth_defaults::use_gateway_callback() {
            // Gateway mode: store pending flow state for the web gateway's
            // `/oauth/callback` handler to complete the exchange. No TCP listener
            // needed — the OAuth provider redirects to the gateway URL.
            oauth_defaults::sweep_expired_flows(&self.pending_oauth_flows).await;

            // Wrap the CSRF nonce with instance name for platform routing.
            // Nginx at auth.DOMAIN parses `instance:nonce` to route the callback
            // to the correct container. The flow is keyed by the raw nonce.
            let platform_state = oauth_defaults::build_platform_state(&expected_state);
            let auth_url = if platform_state != expected_state {
                auth_url.replace(
                    &format!("state={}", urlencoding::encode(&expected_state)),
                    &format!("state={}", urlencoding::encode(&platform_state)),
                )
            } else {
                auth_url
            };

            let flow = oauth_defaults::PendingOAuthFlow {
                extension_name: name.to_string(),
                display_name: display_name.clone(),
                token_url: oauth.token_url.clone(),
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                redirect_uri: redirect_uri.clone(),
                code_verifier,
                access_token_field: oauth.access_token_field.clone(),
                secret_name: auth.secret_name.clone(),
                provider: auth.provider.clone(),
                validation_endpoint: auth.validation_endpoint.clone(),
                scopes: merged_scopes,
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
                .insert(expected_state, flow);

            // Register pending auth without a task handle (gateway handles completion)
            self.pending_auth.write().await.insert(
                name.to_string(),
                PendingAuth {
                    _name: name.to_string(),
                    _kind: ExtensionKind::WasmTool,
                    created_at: std::time::Instant::now(),
                    task_handle: None,
                },
            );

            Ok(AuthResult::awaiting_authorization(
                name,
                ExtensionKind::WasmTool,
                auth_url,
                "gateway".to_string(),
            ))
        } else {
            // TCP listener mode: bind port 9876 and spawn a background task
            // to wait for the callback. This is the original flow for local/desktop use.
            let listener = oauth_defaults::bind_callback_listener()
                .await
                .map_err(|e| format!("Failed to start OAuth callback listener: {}", e))?;

            let token_url = oauth.token_url.clone();
            let access_token_field = oauth.access_token_field.clone();
            let secret_name = auth.secret_name.clone();
            let provider = auth.provider.clone();
            let validation_endpoint = auth.validation_endpoint.clone();
            let user_id = self.user_id.clone();
            let secrets = Arc::clone(&self.secrets);
            let sse_sender = self.sse_sender.read().await.clone();
            let ext_name = name.to_string();

            let task_handle = tokio::spawn(async move {
                let result: Result<(), String> = async {
                    let code = oauth_defaults::wait_for_callback(
                        listener,
                        "/callback",
                        "code",
                        &display_name,
                        Some(&expected_state),
                    )
                    .await
                    .map_err(|e| e.to_string())?;

                    let token_response = oauth_defaults::exchange_oauth_code(
                        &token_url,
                        &client_id,
                        client_secret.as_deref(),
                        &code,
                        &redirect_uri,
                        code_verifier.as_deref(),
                        &access_token_field,
                    )
                    .await
                    .map_err(|e| e.to_string())?;

                    // Validate the token before storing (catches wrong account, etc.)
                    if let Some(ref validation) = validation_endpoint {
                        oauth_defaults::validate_oauth_token(
                            &token_response.access_token,
                            validation,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    }

                    oauth_defaults::store_oauth_tokens(
                        secrets.as_ref(),
                        &user_id,
                        &secret_name,
                        provider.as_deref(),
                        &token_response.access_token,
                        token_response.refresh_token.as_deref(),
                        token_response.expires_in,
                        &merged_scopes,
                    )
                    .await
                    .map_err(|e| e.to_string())?;

                    Ok(())
                }
                .await;

                // Broadcast SSE event
                let (success, message) = match result {
                    Ok(()) => (true, format!("{} authenticated successfully", display_name)),
                    Err(ref e) => (
                        false,
                        format!("{} authentication failed: {}", display_name, e),
                    ),
                };

                match &result {
                    Ok(()) => {
                        tracing::info!(
                            tool = %ext_name,
                            "OAuth completed successfully"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            tool = %ext_name,
                            error = %e,
                            "WASM tool OAuth failed"
                        );
                    }
                }

                if let Some(ref sender) = sse_sender {
                    let _ = sender.send(crate::channels::web::types::SseEvent::AuthCompleted {
                        extension_name: ext_name,
                        success,
                        message,
                    });
                }
            });

            // Store pending auth with task handle
            self.pending_auth.write().await.insert(
                name.to_string(),
                PendingAuth {
                    _name: name.to_string(),
                    _kind: ExtensionKind::WasmTool,
                    created_at: std::time::Instant::now(),
                    task_handle: Some(task_handle),
                },
            );

            Ok(AuthResult::awaiting_authorization(
                name,
                ExtensionKind::WasmTool,
                auth_url,
                "local".to_string(),
            ))
        }
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
