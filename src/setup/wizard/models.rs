//! Steps 4 and 5: model selection and embeddings configuration.

use super::model_catalog::{
    OpenAICompatModelsRequest, fetch_anthropic_models, fetch_ollama_models,
    fetch_openai_compatible_models, fetch_openai_models,
};
use super::*;

/// Keep only models whose id contains the given filter (case-insensitive),
/// when a filter is configured.
fn apply_models_filter(
    models: Vec<(String, String)>,
    filter: Option<&str>,
) -> Vec<(String, String)> {
    let Some(filter) = filter else {
        return models;
    };
    let filter_lower = filter.to_lowercase();
    models
        .into_iter()
        .filter(|(id, _)| id.to_lowercase().contains(&filter_lower))
        .collect()
}

impl SetupWizard {
    /// Step 4: Model selection.
    ///
    /// Branches on the selected LLM backend and fetches models from the
    /// appropriate provider API, with static defaults as fallback.
    pub(super) async fn step_model_selection(&mut self) -> Result<(), SetupError> {
        if self.offer_keep_current_model()? {
            return Ok(());
        }

        let backend = self
            .settings
            .llm_backend
            .as_deref()
            .unwrap_or("nearai")
            .to_string();
        let registry =
            crate::llm::ProviderRegistry::load().map_err(|e| SetupError::Config(e.to_string()))?;

        if backend == "nearai" {
            return self.select_nearai_model().await;
        }
        if let Some(def) = registry.find(&backend) {
            return self.select_registry_model(&backend, def).await;
        }
        if backend == "bedrock" {
            return self.select_required_model(
                "Bedrock model ID (e.g., anthropic.claude-opus-4-6-v1)",
                "Model ID is required",
            );
        }
        // Unknown provider, manual entry
        self.select_required_model(
            "Model name (e.g., meta-llama/Llama-3-8b-chat-hf)",
            "Model name is required",
        )
    }

    /// Offer to keep the currently configured model.
    ///
    /// Returns `true` when the user kept the current model.
    fn offer_keep_current_model(&mut self) -> Result<bool, SetupError> {
        let Some(current) = self.settings.selected_model.clone() else {
            return Ok(false);
        };

        print_info(&format!("Current model: {}", current));
        println!();

        let options = ["Keep current model", "Change model"];
        let choice = select_one("What would you like to do?", &options).map_err(SetupError::Io)?;

        if choice == 0 {
            print_success(&format!("Keeping {}", current));
            return Ok(true);
        }
        Ok(false)
    }

    /// Model selection for NEAR AI, using the provider's `list_models()`
    /// with static defaults as fallback.
    async fn select_nearai_model(&mut self) -> Result<(), SetupError> {
        let fetched = self.fetch_nearai_models().await;
        let default_models: Vec<(String, String)> = vec![
            (
                "zai-org/GLM-latest".into(),
                "GLM Latest (default, fast)".into(),
            ),
            (
                "anthropic::claude-sonnet-4-20250514".into(),
                "Claude Sonnet 4 (best quality)".into(),
            ),
            (
                "openai::gpt-5.3-codex".into(),
                "GPT-5.3 Codex (flagship)".into(),
            ),
            ("openai::gpt-5.2".into(), "GPT-5.2".into()),
            ("openai::gpt-4o".into(), "GPT-4o".into()),
        ];

        let models = if fetched.is_empty() {
            default_models
        } else {
            fetched.iter().map(|m| (m.clone(), m.clone())).collect()
        };
        self.select_from_model_list(&models)
    }

    /// Model selection for a registry-defined provider: fetch from the
    /// provider API when possible, otherwise fall back to manual entry.
    async fn select_registry_model(
        &mut self,
        backend: &str,
        def: &crate::llm::ProviderDefinition,
    ) -> Result<(), SetupError> {
        let can_list = def
            .setup
            .as_ref()
            .map(|s| s.can_list_models())
            .unwrap_or(false);

        if !can_list {
            return self.select_model_with_default(&def.default_model);
        }

        let models = self.fetch_models_for_backend(backend, def).await;
        // Apply models_filter from setup hint (e.g., Groq "chat" filters non-chat models)
        let models =
            apply_models_filter(models, def.setup.as_ref().and_then(|s| s.models_filter()));

        if models.is_empty() {
            // Fall back to manual entry
            return self.select_model_with_default(&def.default_model);
        }
        self.select_from_model_list(&models)
    }

    /// Fetch models from the appropriate provider API for the backend.
    async fn fetch_models_for_backend(
        &self,
        backend: &str,
        def: &crate::llm::ProviderDefinition,
    ) -> Vec<(String, String)> {
        let cached_key = self
            .llm_api_key
            .as_ref()
            .map(|k| k.expose_secret().to_string());

        match backend {
            "anthropic" => fetch_anthropic_models(cached_key.as_deref()).await,
            "openai" => fetch_openai_models(cached_key.as_deref()).await,
            "ollama" => {
                let base_url = self
                    .settings
                    .ollama_base_url
                    .as_deref()
                    .or(def.default_base_url.as_deref())
                    .unwrap_or("http://localhost:11434");
                let models = fetch_ollama_models(base_url).await;
                if models.is_empty() {
                    print_info("No models found. Pull one first: ollama pull llama3");
                }
                models
            }
            _ => {
                // Generic OpenAI-compatible model listing
                let base_url = def.default_base_url.as_deref().unwrap_or("");
                fetch_openai_compatible_models(OpenAICompatModelsRequest {
                    base_url,
                    cached_key: cached_key.as_deref(),
                })
                .await
            }
        }
    }

    /// Prompt for a model name, falling back to the provider default when
    /// the user enters nothing.
    fn select_model_with_default(&mut self, default: &str) -> Result<(), SetupError> {
        let model_id =
            input(&format!("Model name (default: {default})")).map_err(SetupError::Io)?;
        let model_id = if model_id.is_empty() {
            default.to_string()
        } else {
            model_id
        };
        self.settings.selected_model = Some(model_id.clone());
        print_success(&format!("Selected {}", model_id));
        Ok(())
    }

    /// Prompt for a model identifier that must not be empty.
    fn select_required_model(
        &mut self,
        prompt: &str,
        required_msg: &str,
    ) -> Result<(), SetupError> {
        let model_id = input(prompt).map_err(SetupError::Io)?;
        if model_id.is_empty() {
            return Err(SetupError::Config(required_msg.to_string()));
        }
        self.settings.selected_model = Some(model_id.clone());
        print_success(&format!("Selected {}", model_id));
        Ok(())
    }

    /// Present a model list to the user, with a "Custom model ID" escape hatch.
    ///
    /// Each entry is `(model_id, display_label)`.
    fn select_from_model_list(&mut self, models: &[(String, String)]) -> Result<(), SetupError> {
        println!("Available models:");
        println!();

        let mut options: Vec<&str> = models.iter().map(|(_, desc)| desc.as_str()).collect();
        options.push("Custom model ID");

        let choice = select_one("Select a model:", &options).map_err(SetupError::Io)?;

        let selected = if choice == options.len() - 1 {
            loop {
                let raw = input("Enter model ID").map_err(SetupError::Io)?;
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    println!("Model ID cannot be empty.");
                    continue;
                }
                break trimmed;
            }
        } else {
            models[choice].0.clone()
        };

        self.settings.selected_model = Some(selected.clone());
        print_success(&format!("Selected {}", selected));
        Ok(())
    }

    async fn fetch_nearai_models(&self) -> Vec<String> {
        crate::setup::nearai::fetch_nearai_models(self.session_manager.as_ref()).await
    }

    /// Step 5: Embeddings configuration.
    pub(super) fn step_embeddings(&mut self) -> Result<(), SetupError> {
        print_info("Embeddings enable semantic search in your workspace memory.");
        println!();

        if !confirm("Enable semantic search?", true).map_err(SetupError::Io)? {
            self.settings.embeddings.enabled = false;
            print_info("Embeddings disabled. Workspace will use keyword search only.");
            return Ok(());
        }

        let backend = self
            .settings
            .llm_backend
            .as_deref()
            .unwrap_or("nearai")
            .to_string();
        let has_openai_key = self.has_openai_embeddings_key(&backend);
        let has_nearai = backend == "nearai" || self.session_manager.is_some();

        // If the LLM backend is OpenAI and we already have a key, default to OpenAI embeddings
        if backend == "openai" && has_openai_key {
            self.enable_embeddings("openai");
            print_success("Embeddings enabled via OpenAI (using existing API key)");
            return Ok(());
        }

        // If no NEAR AI session and no OpenAI key, only OpenAI is viable
        if !has_nearai && !has_openai_key {
            print_info("No NEAR AI session or OpenAI key found for embeddings.");
            print_info("Set OPENAI_API_KEY in your environment to enable embeddings.");
            self.settings.embeddings.enabled = false;
            return Ok(());
        }

        self.choose_embeddings_provider(has_nearai, has_openai_key)
    }

    /// Report whether an OpenAI API key is available for embeddings, either
    /// from the environment or cached from the OpenAI provider setup.
    fn has_openai_embeddings_key(&self, backend: &str) -> bool {
        if std::env::var("OPENAI_API_KEY").is_ok() {
            return true;
        }
        backend == "openai" && self.llm_api_key.is_some()
    }

    /// Ask which embeddings provider to use and record the choice.
    fn choose_embeddings_provider(
        &mut self,
        has_nearai: bool,
        has_openai_key: bool,
    ) -> Result<(), SetupError> {
        let mut options = Vec::new();
        if has_nearai {
            options.push("NEAR AI (uses same auth, no extra cost)");
        }
        options.push("OpenAI (requires API key)");

        let choice = select_one("Select embeddings provider:", &options).map_err(SetupError::Io)?;

        if has_nearai && choice == 0 {
            self.enable_embeddings("nearai");
            print_success("Embeddings enabled via NEAR AI");
            return Ok(());
        }

        if !has_openai_key {
            print_info("OPENAI_API_KEY not set in environment.");
            print_info("Add it to your .env file or environment to enable embeddings.");
        }
        self.enable_embeddings("openai");
        print_success("Embeddings configured for OpenAI");
        Ok(())
    }

    /// Enable embeddings for `provider` with the default embedding model.
    fn enable_embeddings(&mut self, provider: &str) {
        self.settings.embeddings.enabled = true;
        self.settings.embeddings.provider = provider.to_string();
        self.settings.embeddings.model = "text-embedding-3-small".to_string();
    }
}
