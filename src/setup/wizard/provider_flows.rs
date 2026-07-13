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

        // Clear model only when switching providers (old model may be invalid)
        if self.settings.llm_backend.as_deref() != Some(backend) {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some(backend.to_string());

        // Check env var first
        if let Ok(existing) = std::env::var(env_var) {
            print_info(&format!("{env_var} found: {}", mask_api_key(&existing)));
            if confirm("Use this key?", true).map_err(SetupError::Io)? {
                // Persist env-provided key to secrets store for future runs
                if let Ok(ctx) = self.init_secrets_context().await {
                    let key = SecretString::from(existing.clone());
                    if let Err(e) = ctx.save_secret(secret_name, &key).await {
                        tracing::warn!("Failed to persist env key to secrets: {}", e);
                    }
                }
                self.llm_api_key = Some(SecretString::from(existing));
                print_success(&format!("{display_name} configured (from env)"));
                return Ok(());
            }
        }

        println!();
        print_info(&format!("Get your API key from: {hint_url}"));
        println!();

        let key = secret_input(prompt_label).map_err(SetupError::Io)?;
        let key_str = key.expose_secret();

        if key_str.is_empty() {
            return Err(SetupError::Config("API key cannot be empty".to_string()));
        }

        // Store in secrets if available
        if let Ok(ctx) = self.init_secrets_context().await {
            ctx.save_secret(secret_name, &key)
                .await
                .map_err(|e| SetupError::Config(format!("Failed to save API key: {e}")))?;
            print_success("API key encrypted and saved");
        } else {
            print_info(&format!(
                "Secrets not available. Set {env_var} in your environment."
            ));
        }

        // Make key visible to `optional_env()` for subsequent config resolution.
        // Uses the thread-safe overlay instead of `std::env::set_var` to avoid
        // UB on multi-threaded runtimes.
        crate::config::inject_single_var(env_var, key_str);

        // Cache key in memory for model fetching later in the wizard
        self.llm_api_key = Some(SecretString::from(key_str.to_string()));

        print_success(&format!("{display_name} configured"));
        Ok(())
    }

    /// Generic Ollama-style setup: just needs a base URL, no API key.
    pub(super) fn setup_ollama_generic(
        &mut self,
        def: &crate::llm::ProviderDefinition,
    ) -> Result<(), SetupError> {
        // Clear model only when switching providers (old model may be invalid)
        if self.settings.llm_backend.as_deref() != Some(&def.id) {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some(def.id.clone());

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

        // Clear model only when switching providers (old model may be invalid)
        if self.settings.llm_backend.as_deref() != Some(backend_id) {
            self.settings.selected_model = None;
        }
        self.settings.llm_backend = Some(backend_id.to_string());

        let existing_url = self
            .settings
            .openai_compatible_base_url
            .clone()
            .or_else(|| std::env::var("LLM_BASE_URL").ok());

        let url = if let Some(ref u) = existing_url {
            let url_input = optional_input("Base URL", Some(&format!("current: {}", u)))
                .map_err(SetupError::Io)?;
            url_input.unwrap_or_else(|| u.clone())
        } else {
            input("Base URL (e.g., http://localhost:8000/v1)").map_err(SetupError::Io)?
        };

        if url.is_empty() {
            return Err(SetupError::Config(format!(
                "Base URL is required for {display_name}"
            )));
        }

        self.settings.openai_compatible_base_url = Some(url.clone());

        // Optional API key
        if confirm("Does this endpoint require an API key?", false).map_err(SetupError::Io)? {
            let key = secret_input("API key").map_err(SetupError::Io)?;
            let key_str = key.expose_secret();

            if !key_str.is_empty() {
                if let Ok(ctx) = self.init_secrets_context().await {
                    ctx.save_secret(secret_name, &key)
                        .await
                        .map_err(|e| SetupError::Config(format!("Failed to save API key: {e}")))?;
                    print_success("API key encrypted and saved");
                } else {
                    print_info("Secrets not available. Set the API key in your environment.");
                }
            }
        }

        print_success(&format!("{display_name} configured ({})", url));
        Ok(())
    }
}
