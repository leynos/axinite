//! Steps 4 and 5: model selection and embeddings configuration.

use super::model_catalog::{
    OpenAICompatModelsRequest, fetch_anthropic_models, fetch_ollama_models,
    fetch_openai_compatible_models, fetch_openai_models,
};
use super::*;

impl SetupWizard {
    /// Step 4: Model selection.
    ///
    /// Branches on the selected LLM backend and fetches models from the
    /// appropriate provider API, with static defaults as fallback.
    pub(super) async fn step_model_selection(&mut self) -> Result<(), SetupError> {
        // Show current model if already configured
        if let Some(ref current) = self.settings.selected_model {
            print_info(&format!("Current model: {}", current));
            println!();

            let options = ["Keep current model", "Change model"];
            let choice =
                select_one("What would you like to do?", &options).map_err(SetupError::Io)?;

            if choice == 0 {
                print_success(&format!("Keeping {}", current));
                return Ok(());
            }
        }

        let backend = self.settings.llm_backend.as_deref().unwrap_or("nearai");
        let registry =
            crate::llm::ProviderRegistry::load().map_err(|e| SetupError::Config(e.to_string()))?;

        if backend == "nearai" {
            // NEAR AI: use existing provider list_models()
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
            self.select_from_model_list(&models)?;
        } else if let Some(def) = registry.find(backend) {
            let can_list = def
                .setup
                .as_ref()
                .map(|s| s.can_list_models())
                .unwrap_or(false);

            if can_list {
                // Try to fetch models from the provider's /v1/models endpoint
                let cached_key = self
                    .llm_api_key
                    .as_ref()
                    .map(|k| k.expose_secret().to_string());

                let models = match backend {
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
                };

                // Apply models_filter from setup hint (e.g., Groq "chat" filters non-chat models)
                let models =
                    if let Some(filter) = def.setup.as_ref().and_then(|s| s.models_filter()) {
                        let filter_lower = filter.to_lowercase();
                        models
                            .into_iter()
                            .filter(|(id, _)| id.to_lowercase().contains(&filter_lower))
                            .collect()
                    } else {
                        models
                    };

                if models.is_empty() {
                    // Fall back to manual entry
                    let default = &def.default_model;
                    let model_id = input(&format!("Model name (default: {default})"))
                        .map_err(SetupError::Io)?;
                    let model_id = if model_id.is_empty() {
                        default.clone()
                    } else {
                        model_id
                    };
                    self.settings.selected_model = Some(model_id.clone());
                    print_success(&format!("Selected {}", model_id));
                } else {
                    self.select_from_model_list(&models)?;
                }
            } else {
                // Manual model entry
                let default = &def.default_model;
                let model_id =
                    input(&format!("Model name (default: {default})")).map_err(SetupError::Io)?;
                let model_id = if model_id.is_empty() {
                    default.clone()
                } else {
                    model_id
                };
                self.settings.selected_model = Some(model_id.clone());
                print_success(&format!("Selected {}", model_id));
            }
        } else if backend == "bedrock" {
            let model_id = input("Bedrock model ID (e.g., anthropic.claude-opus-4-6-v1)")
                .map_err(SetupError::Io)?;
            if model_id.is_empty() {
                return Err(SetupError::Config("Model ID is required".to_string()));
            }
            self.settings.selected_model = Some(model_id.clone());
            print_success(&format!("Selected {}", model_id));
        } else {
            // Unknown provider, manual entry
            let model_id = input("Model name (e.g., meta-llama/Llama-3-8b-chat-hf)")
                .map_err(SetupError::Io)?;
            if model_id.is_empty() {
                return Err(SetupError::Config("Model name is required".to_string()));
            }
            self.settings.selected_model = Some(model_id.clone());
            print_success(&format!("Selected {}", model_id));
        }

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

        let backend = self.settings.llm_backend.as_deref().unwrap_or("nearai");
        let has_openai_key = std::env::var("OPENAI_API_KEY").is_ok()
            || (backend == "openai" && self.llm_api_key.is_some());
        let has_nearai = backend == "nearai" || self.session_manager.is_some();

        // If the LLM backend is OpenAI and we already have a key, default to OpenAI embeddings
        if backend == "openai" && has_openai_key {
            self.settings.embeddings.enabled = true;
            self.settings.embeddings.provider = "openai".to_string();
            self.settings.embeddings.model = "text-embedding-3-small".to_string();
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

        let mut options = Vec::new();
        if has_nearai {
            options.push("NEAR AI (uses same auth, no extra cost)");
        }
        options.push("OpenAI (requires API key)");

        let choice = select_one("Select embeddings provider:", &options).map_err(SetupError::Io)?;

        // Map choice back to provider name
        let provider = if has_nearai && choice == 0 {
            "nearai"
        } else {
            "openai"
        };

        match provider {
            "nearai" => {
                self.settings.embeddings.enabled = true;
                self.settings.embeddings.provider = "nearai".to_string();
                self.settings.embeddings.model = "text-embedding-3-small".to_string();
                print_success("Embeddings enabled via NEAR AI");
            }
            _ => {
                if !has_openai_key {
                    print_info("OPENAI_API_KEY not set in environment.");
                    print_info("Add it to your .env file or environment to enable embeddings.");
                }
                self.settings.embeddings.enabled = true;
                self.settings.embeddings.provider = "openai".to_string();
                self.settings.embeddings.model = "text-embedding-3-small".to_string();
                print_success("Embeddings configured for OpenAI");
            }
        }

        Ok(())
    }
}
