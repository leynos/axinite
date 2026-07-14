//! Vendor-specific provider setup flows: NEAR AI, Anthropic (API key
//! and OAuth), and AWS Bedrock.

use super::provider_flows::ApiKeyProviderSpec;
use super::*;

impl SetupWizard {
    /// NEAR AI provider setup (extracted from the old step_authentication).
    pub(super) async fn setup_nearai(&mut self) -> Result<(), SetupError> {
        self.settings.llm_backend = Some("nearai".to_string());

        // Check if we already have a session
        if self.reuse_valid_nearai_session().await {
            return Ok(());
        }

        let session = self.get_or_create_session_manager();

        // Trigger authentication flow
        session
            .ensure_authenticated()
            .await
            .map_err(|e| SetupError::Auth(e.to_string()))?;

        self.session_manager = Some(Arc::clone(&session));

        // Persist session token to the database so the runtime can load it
        // via `attach_store()` → `load_session_from_db()` without the
        // backwards-compat fallback. The session manager saved to disk but
        // doesn't have a DB store attached during onboarding.
        self.persist_session_to_db().await;

        self.persist_nearai_api_key(&session).await;

        print_success("NEAR AI configured");
        Ok(())
    }

    /// Validate an existing NEAR AI session, reporting the outcome.
    ///
    /// Returns `true` when the current session is valid and can be reused.
    async fn reuse_valid_nearai_session(&self) -> bool {
        let Some(ref session) = self.session_manager else {
            return false;
        };
        if !session.has_token().await {
            return false;
        }

        print_info("Existing session found. Validating...");
        match session.ensure_authenticated().await {
            Ok(()) => {
                print_success("NEAR AI session valid");
                true
            }
            Err(e) => {
                print_info(&format!("Session invalid: {}. Re-authenticating...", e));
                false
            }
        }
    }

    /// Return the current session manager, creating a default one when none
    /// exists yet.
    fn get_or_create_session_manager(&self) -> Arc<SessionManager> {
        if let Some(ref s) = self.session_manager {
            Arc::clone(s)
        } else {
            let config = SessionConfig {
                session_path: crate::config::llm::default_session_path(),
                ..SessionConfig::default()
            };
            Arc::new(SessionManager::new(config))
        }
    }

    /// Sync the NEAR AI API key with the encrypted secrets store so
    /// `inject_llm_keys_from_secrets()` can load it on future runs.
    ///
    /// Saves the key when the user chose the API key path, and clears any
    /// stale key otherwise. Failures are logged, not fatal.
    async fn persist_nearai_api_key(&mut self, session: &Arc<SessionManager>) {
        let Ok(ctx) = self.init_secrets_context().await else {
            return;
        };
        if let Some(api_key) = session.get_api_key().await {
            if let Err(e) = ctx.save_secret("llm_nearai_api_key", &api_key).await {
                tracing::warn!("Failed to persist NEARAI_API_KEY to secrets: {}", e);
            }
        } else if let Err(e) = ctx.delete_secret("llm_nearai_api_key").await {
            tracing::warn!("Failed to clear stale NEARAI_API_KEY secret: {}", e);
        }
    }

    /// Anthropic provider setup: API key or OAuth token from `claude login`.
    pub(super) async fn setup_anthropic(&mut self) -> Result<(), SetupError> {
        let options = &["Direct API Key", "OAuth Token (from `claude login`)"];
        let choice = select_one("How do you want to authenticate with Anthropic?", options)
            .map_err(SetupError::Io)?;

        if choice == 0 {
            // Standard API key flow
            self.setup_api_key_provider(ApiKeyProviderSpec {
                backend: "anthropic",
                env_var: "ANTHROPIC_API_KEY",
                secret_name: "llm_anthropic_api_key",
                prompt_label: "Anthropic API key",
                hint_url: "https://console.anthropic.com/settings/keys",
                override_display_name: None,
            })
            .await
        } else {
            // OAuth token flow
            self.setup_anthropic_oauth().await
        }
    }

    /// Anthropic OAuth setup: extract token from `claude login` credentials.
    async fn setup_anthropic_oauth(&mut self) -> Result<(), SetupError> {
        // Clear model only when switching providers (old model may be invalid)
        if self.settings.llm_backend.as_deref() != Some("anthropic") {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some("anthropic".to_string());

        // Try to extract existing OAuth token from Claude Code credentials
        if let Some(token) = crate::config::ClaudeCodeConfig::extract_oauth_token() {
            print_info(&format!("Found OAuth token: {}", mask_api_key(&token)));
            if confirm("Use this token?", true).map_err(SetupError::Io)? {
                return self.save_anthropic_oauth_token(&token).await;
            }
        } else {
            print_info("No OAuth token found from `claude login`.");
            print_info("Run `claude login` in a terminal to authenticate, then retry.");
            println!();

            if confirm("Retry after running `claude login`?", true).map_err(SetupError::Io)? {
                // Block until the user has run `claude login` in another terminal
                input("Press Enter after running `claude login` in another terminal...")
                    .map_err(SetupError::Io)?;
                if let Some(token) = crate::config::ClaudeCodeConfig::extract_oauth_token() {
                    print_info(&format!("Found OAuth token: {}", mask_api_key(&token)));
                    return self.save_anthropic_oauth_token(&token).await;
                }
                print_error("Still no OAuth token found.");
            }
        }

        // Fallback: let user paste the token manually, or switch to API key
        print_info("You can paste your OAuth token directly (starts with sk-ant-oat01-).");
        print_info("Or press Enter with no input to switch to the API key flow.");
        let token = secret_input("Anthropic OAuth token").map_err(SetupError::Io)?;
        let token_str = token.expose_secret();
        if token_str.is_empty() {
            print_info("Switching to API key flow...");
            return self
                .setup_api_key_provider(ApiKeyProviderSpec {
                    backend: "anthropic",
                    env_var: "ANTHROPIC_API_KEY",
                    secret_name: "llm_anthropic_api_key",
                    prompt_label: "Anthropic API key",
                    hint_url: "https://console.anthropic.com/settings/keys",
                    override_display_name: None,
                })
                .await;
        }
        self.save_anthropic_oauth_token(token_str).await
    }

    /// Save an Anthropic OAuth token to secrets and set env for immediate use.
    async fn save_anthropic_oauth_token(&mut self, token: &str) -> Result<(), SetupError> {
        // Validate token format to catch accidentally pasted API keys
        if !token.starts_with("sk-ant-oat") {
            print_error("Token doesn't look like an OAuth token (expected prefix: sk-ant-oat).");
            print_info("If you have an API key instead, use the 'Direct API Key' option.");
            return Err(SetupError::Config("Invalid OAuth token format".to_string()));
        }

        // Store in secrets if available
        if let Ok(ctx) = self.init_secrets_context().await {
            let key = SecretString::from(token.to_string());
            ctx.save_secret("llm_anthropic_oauth_token", &key)
                .await
                .map_err(|e| SetupError::Config(format!("Failed to save OAuth token: {e}")))?;
            print_success("OAuth token encrypted and saved");
        } else {
            print_info("Secrets not available. Set ANTHROPIC_OAUTH_TOKEN in your environment.");
        }

        // Make the token visible to `optional_env()` for subsequent config
        // resolution (model selection step). Uses the thread-safe overlay
        // instead of `std::env::set_var` to avoid UB on multi-threaded runtimes.
        crate::config::inject_single_var("ANTHROPIC_OAUTH_TOKEN", token);

        // Cache for model fetching
        self.llm_api_key = Some(SecretString::from(token.to_string()));

        print_success("Anthropic OAuth configured");
        Ok(())
    }

    /// AWS Bedrock provider setup: region, auth, and cross-region config.
    pub(super) async fn setup_bedrock(&mut self) -> Result<(), SetupError> {
        if self.settings.llm_backend.as_deref() != Some("bedrock") {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some("bedrock".to_string());

        // Region
        let default_region = self
            .settings
            .bedrock_region
            .as_deref()
            .unwrap_or("us-east-1");

        let region_input =
            optional_input("AWS region", Some(&format!("default: {}", default_region)))
                .map_err(SetupError::Io)?;

        let region = region_input.unwrap_or_else(|| default_region.to_string());
        self.settings.bedrock_region = Some(region.clone());

        // Auth method
        print_info("Select authentication method:");
        println!();
        let auth_options = &[
            "AWS default credentials (env vars, ~/.aws/credentials, IAM roles)",
            "AWS named profile (SSO / assume-role)",
        ];
        let auth_choice = select_one("Auth:", auth_options).map_err(SetupError::Io)?;

        match auth_choice {
            0 => {
                // Default AWS credentials — clear any stale named profile
                self.settings.bedrock_profile = None;
                print_info(
                    "Using default AWS credential chain (env vars, ~/.aws/credentials, IAM roles).",
                );
            }
            1 => {
                // Named profile
                let profile =
                    input("AWS profile name (from ~/.aws/config)").map_err(SetupError::Io)?;
                if profile.trim().is_empty() {
                    // Empty input clears any previously configured profile
                    self.settings.bedrock_profile = None;
                    print_info("AWS profile cleared; using default AWS credential chain instead.");
                } else {
                    self.settings.bedrock_profile = Some(profile.clone());
                    print_success(&format!("AWS profile '{}' saved", profile));
                }
            }
            _ => return Err(SetupError::Config("Invalid auth selection".to_string())),
        }

        self.setup_bedrock_cross_region()
    }

    /// Bedrock cross-region inference prefix selection (sub-step of setup_bedrock).
    fn setup_bedrock_cross_region(&mut self) -> Result<(), SetupError> {
        print_info("Cross-region inference routes requests across AWS regions for capacity:");
        println!();
        let cross_options = &[
            "us     - route within US regions (recommended for us-east-1)",
            "global - route to any AWS region worldwide",
            "eu     - route within European regions",
            "apac   - route within Asia-Pacific regions",
            "none   - single-region only (no cross-region routing)",
        ];
        let cross_choice = select_one("Cross-region:", cross_options).map_err(SetupError::Io)?;

        let cross_region = match cross_choice {
            0 => Some("us".to_string()),
            1 => Some("global".to_string()),
            2 => Some("eu".to_string()),
            3 => Some("apac".to_string()),
            4 => None,
            _ => None,
        };
        self.settings.bedrock_cross_region = cross_region;

        let region = self
            .settings
            .bedrock_region
            .as_deref()
            .unwrap_or("us-east-1");
        print_success(&format!("AWS Bedrock configured (region: {})", region));
        Ok(())
    }
}
