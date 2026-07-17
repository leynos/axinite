//! Incremental persistence of wizard settings and bootstrap environment
//! variables.

use super::*;

impl SetupWizard {
    /// Persist current settings to the database.
    ///
    /// Returns `Ok(true)` if settings were saved, `Ok(false)` if no database
    /// connection is available yet (e.g., before Step 1 completes).
    pub(super) async fn persist_settings(&self) -> Result<bool, SetupError> {
        if let Some(persistence) = self.default_settings_persistence() {
            persistence
                .save_default_settings(&self.settings)
                .await
                .map_err(|e| {
                    SetupError::Database(format!("Failed to save settings to database: {}", e))
                })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Write bootstrap environment variables to `~/.axinite/.env`.
    ///
    /// These are the chicken-and-egg settings needed before the database is
    /// connected (DATABASE_BACKEND, DATABASE_URL, LLM_BACKEND, etc.).
    ///
    /// **Credentials are NOT written here.** API keys and OAuth tokens live
    /// only in the encrypted secrets DB. `LlmConfig::resolve()` defers
    /// gracefully when credentials are missing during early startup, and the
    /// re-resolution in `AppBuilder::build_all()` fills them in after
    /// `inject_llm_keys_from_secrets()` loads from encrypted storage.
    pub(super) fn write_bootstrap_env(&self) -> Result<(), SetupError> {
        let registry =
            crate::llm::ProviderRegistry::load().map_err(|e| SetupError::Config(e.to_string()))?;
        let mut env_vars: Vec<(String, String)> = Vec::new();

        self.push_database_env(&mut env_vars);
        self.push_llm_env(&registry, &mut env_vars);

        // Secrets master key (env var mode): write to .env so it's available
        // on next startup before the DB is connected.
        if let Some(ref key_hex) = self.settings.secrets_master_key_hex {
            env_vars.push(("SECRETS_MASTER_KEY".to_string(), key_hex.clone()));
        }

        // Always write ONBOARD_COMPLETED so that check_onboard_needed()
        // (which runs before the DB is connected) knows to skip re-onboarding.
        if self.settings.onboard_completed {
            env_vars.push(("ONBOARD_COMPLETED".to_string(), "true".to_string()));
        }

        // Claude Code sandbox mode
        if self.settings.sandbox.claude_code_enabled {
            env_vars.push(("CLAUDE_CODE_ENABLED".to_string(), "true".to_string()));
        }

        self.push_signal_env(&mut env_vars);

        if !env_vars.is_empty() {
            let pairs: Vec<(&str, &str)> = env_vars
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            crate::bootstrap::upsert_bootstrap_vars(&pairs).map_err(|e| {
                SetupError::Io(std::io::Error::other(format!(
                    "Failed to save bootstrap env to .env: {}",
                    e
                )))
            })?;
        }

        Ok(())
    }

    /// Append database bootstrap variables for `~/.axinite/.env`.
    fn push_database_env(&self, env_vars: &mut Vec<(String, String)>) {
        if let Some(ref backend) = self.settings.database_backend {
            env_vars.push(("DATABASE_BACKEND".to_string(), backend.clone()));
        }
        if let Some(ref url) = self.settings.database_url {
            env_vars.push(("DATABASE_URL".to_string(), url.clone()));
        }
        if let Some(ref path) = self.settings.libsql_path {
            env_vars.push(("LIBSQL_PATH".to_string(), path.clone()));
        }
        if let Some(ref url) = self.settings.libsql_url {
            env_vars.push(("LIBSQL_URL".to_string(), url.clone()));
        }
    }

    /// Append LLM bootstrap variables for `~/.axinite/.env`.
    ///
    /// Same chicken-and-egg problem as DATABASE_BACKEND:
    /// `Config::from_env()` needs the backend before the DB is connected.
    fn push_llm_env(
        &self,
        registry: &crate::llm::ProviderRegistry,
        env_vars: &mut Vec<(String, String)>,
    ) {
        if let Some(ref backend) = self.settings.llm_backend {
            env_vars.push(("LLM_BACKEND".to_string(), backend.clone()));
        }
        if let Some(ref url) = self.settings.openai_compatible_base_url {
            env_vars.push(("LLM_BASE_URL".to_string(), url.clone()));
        }
        if let Some(ref url) = self.settings.ollama_base_url {
            env_vars.push(("OLLAMA_BASE_URL".to_string(), url.clone()));
        }
        if let Some(ref region) = self.settings.bedrock_region {
            env_vars.push(("BEDROCK_REGION".to_string(), region.clone()));
        }
        if self.settings.llm_backend.as_deref() == Some("bedrock") {
            self.push_bedrock_env(env_vars);
        }

        // Model name: same chicken-and-egg — Config::from_env() resolves the
        // model before the DB is connected, so we must persist it to .env.
        // Write the backend-specific env var so the correct resolution path
        // picks it up (looked up from the provider registry).
        // Bedrock model is already written above as BEDROCK_MODEL, skip here.
        if self.settings.llm_backend.as_deref() != Some("bedrock")
            && let Some(ref model) = self.settings.selected_model
        {
            let backend_str = self.settings.llm_backend.as_deref().unwrap_or("nearai");
            let model_env = registry.model_env_var(backend_str);
            env_vars.push((model_env.to_string(), model.clone()));
        }

        // Also write provider-specific base URL env var if the provider
        // defines one (e.g., GROQ doesn't need LLM_BASE_URL since its
        // default is compiled in, but it doesn't hurt to be explicit).
        if let Some(ref backend) = self.settings.llm_backend
            && let Some(pair) = provider_base_url_var(registry, backend)
        {
            env_vars.push(pair);
        }
    }

    /// Append Bedrock-specific bootstrap variables.
    fn push_bedrock_env(&self, env_vars: &mut Vec<(String, String)>) {
        if let Some(ref model) = self.settings.selected_model {
            env_vars.push(("BEDROCK_MODEL".to_string(), model.clone()));
        }
        if let Some(ref cross) = self.settings.bedrock_cross_region {
            env_vars.push(("BEDROCK_CROSS_REGION".to_string(), cross.clone()));
        }
        if let Some(ref profile) = self.settings.bedrock_profile {
            env_vars.push(("AWS_PROFILE".to_string(), profile.clone()));
        }
    }

    /// Append Signal channel bootstrap variables for `~/.axinite/.env`
    /// (chicken-and-egg: config resolves before DB).
    fn push_signal_env(&self, env_vars: &mut Vec<(String, String)>) {
        if let Some(ref url) = self.settings.channels.signal_http_url {
            env_vars.push(("SIGNAL_HTTP_URL".to_string(), url.clone()));
        }
        if let Some(ref account) = self.settings.channels.signal_account {
            env_vars.push(("SIGNAL_ACCOUNT".to_string(), account.clone()));
        }
        if let Some(ref allow_from) = self.settings.channels.signal_allow_from {
            env_vars.push(("SIGNAL_ALLOW_FROM".to_string(), allow_from.clone()));
        }
        if let Some(ref allow_from_groups) = self.settings.channels.signal_allow_from_groups
            && !allow_from_groups.is_empty()
        {
            env_vars.push((
                "SIGNAL_ALLOW_FROM_GROUPS".to_string(),
                allow_from_groups.clone(),
            ));
        }
        if let Some(ref dm_policy) = self.settings.channels.signal_dm_policy {
            env_vars.push(("SIGNAL_DM_POLICY".to_string(), dm_policy.clone()));
        }
        if let Some(ref group_policy) = self.settings.channels.signal_group_policy {
            env_vars.push(("SIGNAL_GROUP_POLICY".to_string(), group_policy.clone()));
        }
        if let Some(ref group_allow_from) = self.settings.channels.signal_group_allow_from
            && !group_allow_from.is_empty()
        {
            env_vars.push((
                "SIGNAL_GROUP_ALLOW_FROM".to_string(),
                group_allow_from.clone(),
            ));
        }
    }

    /// Persist the NEAR AI session token to the database.
    ///
    /// The session manager writes to disk during `ensure_authenticated()` but
    /// doesn't have a DB store attached during onboarding. This reads the
    /// session file from disk and stores it under the `nearai.session_token`
    /// key so the runtime's `attach_store()` finds it without fallback.
    ///
    /// Best-effort: silently ignores errors (no DB connection yet, no
    /// session file, etc.).
    pub(super) async fn persist_session_to_db(&self) {
        let session_path = crate::config::llm::default_session_path();
        let data = match ambient_fs::read_to_string(&session_path) {
            Ok(d) if !d.trim().is_empty() => d,
            _ => return,
        };
        let value: serde_json::Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(persistence) = self.default_settings_persistence() {
            if let Err(e) = persistence.save_session_token(&value).await {
                tracing::debug!("Could not persist session token to database: {}", e);
            } else {
                tracing::debug!("Session token persisted to database");
            }
        }
    }

    /// Persist settings to DB and bootstrap .env after each step.
    ///
    /// Silently ignores errors (e.g., DB not connected yet before step 1
    /// completes). This is best-effort incremental persistence.
    pub(super) async fn persist_after_step(&self) {
        // Write bootstrap .env (always possible)
        if let Err(e) = self.write_bootstrap_env() {
            tracing::debug!("Could not write bootstrap env after step: {}", e);
        }

        // Persist to DB
        match self.persist_settings().await {
            Ok(true) => tracing::debug!("Settings persisted to database after step"),
            Ok(false) => tracing::debug!("No DB connection yet, skipping settings persist"),
            Err(e) => tracing::debug!("Could not persist settings after step: {}", e),
        }
    }

    /// Load previously saved settings from the database after Step 1
    /// establishes a connection.
    ///
    /// This enables recovery from partial onboarding runs: if the user
    /// completed steps 1-4 previously but step 5 failed, re-running
    /// the wizard will pre-populate settings from the database.
    ///
    /// **Callers must re-apply any wizard choices made before this call**
    /// via `self.settings.merge_from(&step_settings)`, since `merge_from`
    /// prefers the `other` argument's non-default values. Without this,
    /// stale DB values would overwrite fresh user choices.
    pub(super) async fn try_load_existing_settings(&mut self) {
        if let Some(persistence) = self.default_settings_persistence() {
            match persistence.get_all_settings_map().await {
                Ok(db_map) if !db_map.is_empty() => {
                    let existing = Settings::from_db_map(&db_map);
                    self.settings.merge_from(&existing);
                    tracing::info!("Loaded {} existing settings from database", db_map.len());
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Could not load existing settings: {}", e);
                }
            }
        }
    }
}
