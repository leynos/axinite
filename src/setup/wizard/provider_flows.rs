//! Generic provider setup flows shared across vendors: API-key-based,
//! Ollama-style, and OpenAI-compatible providers.

use super::*;

pub(super) struct ApiKeyProviderSpec<'a> {
    pub(super) backend: &'a str,
    pub(super) env_var: &'a str,
    pub(super) secret_name: &'a str,
    pub(super) prompt_label: &'a str,
    pub(super) hint_url: &'a str,
    pub(super) override_display_name: Option<&'a str>,
}

pub(super) struct OpenAICompatSpec<'a> {
    pub(super) backend_id: &'a str,
    pub(super) secret_name: &'a str,
    pub(super) display_name: &'a str,
}

impl SetupWizard {
    /// Shared setup flow for API-key-based providers.
    pub(super) async fn setup_api_key_provider(
        &mut self,
        spec: ApiKeyProviderSpec<'_>,
    ) -> Result<(), SetupError> {
        let ApiKeyProviderSpec {
            backend,
            env_var,
            secret_name,
            prompt_label,
            hint_url,
            override_display_name,
        } = spec;

        let display_name = override_display_name.unwrap_or(match backend {
            "anthropic" => "Anthropic",
            "openai" => "OpenAI",
            other => other,
        });

        self.switch_llm_backend(backend);

        // Check env var first
        if self
            .try_env_api_key(env_var, secret_name, display_name)
            .await?
        {
            return Ok(());
        }

        println!();
        print_info(&format!("Get your API key from: {hint_url}"));
        println!();

        let key = secret_input(prompt_label).map_err(SetupError::Io)?;
        if key.expose_secret().is_empty() {
            return Err(SetupError::Config("API key cannot be empty".to_string()));
        }

        self.store_api_key(env_var, secret_name, &key).await?;

        print_success(&format!("{display_name} configured"));
        Ok(())
    }

    /// Record the chosen LLM backend, clearing the selected model only when
    /// switching providers (the old model may be invalid).
    fn switch_llm_backend(&mut self, backend: &str) {
        if self.settings.llm_backend.as_deref() != Some(backend) {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some(backend.to_string());
    }

    /// Offer to reuse an API key found in the environment, persisting it to
    /// the secrets store when accepted.
    ///
    /// Returns `true` when the env-provided key was accepted.
    async fn try_env_api_key(
        &mut self,
        env_var: &str,
        secret_name: &str,
        display_name: &str,
    ) -> Result<bool, SetupError> {
        let Ok(existing) = std::env::var(env_var) else {
            return Ok(false);
        };

        print_info(&format!("{env_var} found: {}", mask_api_key(&existing)));
        if !confirm("Use this key?", true).map_err(SetupError::Io)? {
            return Ok(false);
        }

        // Persist env-provided key to secrets store for future runs
        if let Ok(ctx) = self.init_secrets_context().await {
            let key = SecretString::from(existing.clone());
            if let Err(e) = ctx.save_secret(secret_name, &key).await {
                tracing::warn!("Failed to persist env key to secrets: {}", e);
            }
        }
        self.llm_api_key = Some(SecretString::from(existing));
        print_success(&format!("{display_name} configured (from env)"));
        Ok(true)
    }

    /// Persist a freshly entered API key to the secrets store (when
    /// available), the env overlay, and the in-memory cache.
    async fn store_api_key(
        &mut self,
        env_var: &str,
        secret_name: &str,
        key: &SecretString,
    ) -> Result<(), SetupError> {
        // Store in secrets if available
        if let Ok(ctx) = self.init_secrets_context().await {
            ctx.save_secret(secret_name, key)
                .await
                .map_err(|e| SetupError::Config(format!("Failed to save API key: {e}")))?;
            print_success("API key encrypted and saved");
        } else {
            print_info(&format!(
                "Secrets not available. Set {env_var} in your environment."
            ));
        }

        let key_str = key.expose_secret();

        // Make key visible to `optional_env()` for subsequent config resolution.
        // Uses the thread-safe overlay instead of `std::env::set_var` to avoid
        // UB on multi-threaded runtimes.
        crate::config::inject_single_var(env_var, key_str);

        // Cache key in memory for model fetching later in the wizard
        self.llm_api_key = Some(SecretString::from(key_str.to_string()));
        Ok(())
    }

    /// Generic Ollama-style setup: just needs a base URL, no API key.
    pub(super) fn setup_ollama_generic(
        &mut self,
        def: &crate::llm::ProviderDefinition,
    ) -> Result<(), SetupError> {
        self.switch_llm_backend(&def.id);

        let default_url = self
            .settings
            .ollama_base_url
            .as_deref()
            .or(def.default_base_url.as_deref())
            .unwrap_or("http://localhost:11434");

        let display_name = def
            .setup
            .as_ref()
            .map(|s| s.display_name())
            .unwrap_or(&def.id);

        let url_input = optional_input(
            &format!("{display_name} base URL"),
            Some(&format!("default: {}", default_url)),
        )
        .map_err(SetupError::Io)?;

        let url = url_input.unwrap_or_else(|| default_url.to_string());
        self.settings.ollama_base_url = Some(url.clone());

        print_success(&format!("{display_name} configured ({})", url));
        Ok(())
    }

    /// Generic OpenAI-compatible setup: base URL + optional API key.
    pub(super) async fn setup_openai_compatible_generic(
        &mut self,
        spec: OpenAICompatSpec<'_>,
    ) -> Result<(), SetupError> {
        let OpenAICompatSpec {
            backend_id,
            secret_name,
            display_name,
        } = spec;

        self.switch_llm_backend(backend_id);

        let url = self.prompt_compatible_base_url()?;
        if url.is_empty() {
            return Err(SetupError::Config(format!(
                "Base URL is required for {display_name}"
            )));
        }

        self.settings.openai_compatible_base_url = Some(url.clone());

        self.maybe_store_endpoint_api_key(secret_name).await?;

        print_success(&format!("{display_name} configured ({})", url));
        Ok(())
    }

    /// Prompt for the endpoint base URL, offering any previously configured
    /// value as the default.
    fn prompt_compatible_base_url(&self) -> Result<String, SetupError> {
        let existing_url = self
            .settings
            .openai_compatible_base_url
            .clone()
            .or_else(|| std::env::var("LLM_BASE_URL").ok());

        if let Some(u) = existing_url {
            let url_input = optional_input("Base URL", Some(&format!("current: {}", u)))
                .map_err(SetupError::Io)?;
            Ok(url_input.unwrap_or(u))
        } else {
            input("Base URL (e.g., http://localhost:8000/v1)").map_err(SetupError::Io)
        }
    }

    /// Ask whether the endpoint needs an API key, and store one when the
    /// user supplies it.
    async fn maybe_store_endpoint_api_key(&mut self, secret_name: &str) -> Result<(), SetupError> {
        if !confirm("Does this endpoint require an API key?", false).map_err(SetupError::Io)? {
            return Ok(());
        }

        let key = secret_input("API key").map_err(SetupError::Io)?;
        if key.expose_secret().is_empty() {
            return Ok(());
        }

        if let Ok(ctx) = self.init_secrets_context().await {
            ctx.save_secret(secret_name, &key)
                .await
                .map_err(|e| SetupError::Config(format!("Failed to save API key: {e}")))?;
            print_success("API key encrypted and saved");
        } else {
            print_info("Secrets not available. Set the API key in your environment.");
        }
        Ok(())
    }
}
