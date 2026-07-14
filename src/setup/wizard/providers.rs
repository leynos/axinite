//! Step 3: inference provider selection menu and setup dispatch.

use super::provider_flows::{ApiKeyProviderSpec, OpenAICompatSpec};
use super::*;

/// Human-readable display name for a provider id.
fn provider_display_name(current: &str, registry: &crate::llm::ProviderRegistry) -> String {
    if current == "nearai" {
        "NEAR AI".to_string()
    } else if let Some(def) = registry.find(current) {
        def.setup
            .as_ref()
            .map(|s| s.display_name().to_string())
            .unwrap_or_else(|| def.id.clone())
    } else {
        current.to_string()
    }
}

/// Report whether the provider id names a supported provider.
fn is_known_provider(current: &str, registry: &crate::llm::ProviderRegistry) -> bool {
    matches!(current, "nearai" | "bedrock") || registry.is_known(current)
}

/// Build the provider menu: NearAI first, then all registry providers with
/// setup hints, then Bedrock (native AWS SDK, not registry-based).
///
/// Returns parallel vectors of menu labels and provider ids.
fn build_provider_menu(registry: &crate::llm::ProviderRegistry) -> (Vec<String>, Vec<String>) {
    let selectable = registry.selectable();
    let mut options: Vec<String> = Vec::with_capacity(2 + selectable.len());
    let mut provider_ids: Vec<String> = Vec::with_capacity(2 + selectable.len());

    options.push("NEAR AI          - multi-model access via NEAR account".to_string());
    provider_ids.push("nearai".to_string());

    for def in &selectable {
        let label = format!(
            "{:<17}- {}",
            def.setup
                .as_ref()
                .map(|s| s.display_name())
                .unwrap_or(&def.id),
            def.description
        );
        options.push(label);
        provider_ids.push(def.id.clone());
    }

    options.push("AWS Bedrock      - Claude & other models via AWS (IAM, SSO)".to_string());
    provider_ids.push("bedrock".to_string());

    (options, provider_ids)
}

impl SetupWizard {
    /// Step 3: Inference provider selection.
    ///
    /// Uses the provider registry to dynamically build the selection menu.
    /// NearAI is always first (special auth), then all registry providers
    /// that have setup hints.
    pub(super) async fn step_inference_provider(&mut self) -> Result<(), SetupError> {
        let registry =
            crate::llm::ProviderRegistry::load().map_err(|e| SetupError::Config(e.to_string()))?;

        // Show current provider if already configured
        if let Some(current) = self.settings.llm_backend.clone()
            && self
                .offer_keep_current_provider(&current, &registry)
                .await?
        {
            return Ok(());
        }

        print_info("Select your inference provider:");
        println!();

        let (options, provider_ids) = build_provider_menu(&registry);
        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
        let choice = select_one("Provider:", &option_refs).map_err(SetupError::Io)?;
        let selected_id = &provider_ids[choice];

        if selected_id == "bedrock" {
            self.setup_bedrock().await?;
        } else {
            self.run_provider_setup(selected_id, &registry).await?;
        }

        Ok(())
    }

    /// Offer to keep (and, where needed, re-configure) the currently
    /// configured provider.
    ///
    /// Returns `true` when the current provider was kept.
    async fn offer_keep_current_provider(
        &mut self,
        current: &str,
        registry: &crate::llm::ProviderRegistry,
    ) -> Result<bool, SetupError> {
        print_info(&format!(
            "Current provider: {}",
            provider_display_name(current, registry)
        ));
        println!();

        if !is_known_provider(current, registry) {
            print_info(&format!(
                "Unknown provider '{}', please select a supported provider.",
                current
            ));
            return Ok(false);
        }

        if !confirm("Keep current provider?", true).map_err(SetupError::Io)? {
            return Ok(false);
        }

        if current == "bedrock" {
            // Keeping the existing Bedrock config — no need to re-run
            // the full setup flow (region, auth, cross-region).
            print_info("Keeping existing AWS Bedrock configuration.");
            return Ok(true);
        }

        self.run_provider_setup(current, registry).await?;
        Ok(true)
    }

    /// Run the setup flow for a specific provider.
    ///
    /// NearAI has its own special flow. Registry providers dispatch
    /// based on their `SetupHint` kind.
    pub(super) async fn run_provider_setup(
        &mut self,
        provider_id: &str,
        registry: &crate::llm::ProviderRegistry,
    ) -> Result<(), SetupError> {
        if provider_id == "nearai" {
            return self.setup_nearai().await;
        }

        let def = registry
            .find(provider_id)
            .ok_or_else(|| SetupError::Config(format!("Unknown provider: {}", provider_id)))?;

        // Providers without a setup hint (e.g., user-defined providers configured
        // purely via env vars) skip credential setup and go to model selection.
        let Some(setup) = def.setup.as_ref() else {
            print_info(&format!(
                "Provider '{}' has no setup wizard. Configure via environment variables.",
                provider_id
            ));
            self.settings.llm_backend = Some(provider_id.to_string());
            return Ok(());
        };

        // Anthropic has a custom flow: API key or OAuth token from `claude login`.
        if provider_id == "anthropic" {
            return self.setup_anthropic().await;
        }

        match setup {
            crate::llm::registry::SetupHint::ApiKey {
                secret_name,
                key_url,
                display_name,
                ..
            } => {
                let env_var = def.api_key_env.as_deref().unwrap_or("LLM_API_KEY");
                let url = key_url.as_deref().unwrap_or("the provider's website");

                // Only store base URL for providers that resolve through
                // LLM_BASE_URL (openai_compatible, openrouter). Other providers
                // like groq/nvidia have their own base_url_env and don't need
                // this backward-compat setting.
                if def.base_url_env.as_deref() == Some("LLM_BASE_URL")
                    && let Some(ref base_url) = def.default_base_url
                {
                    self.settings.openai_compatible_base_url = Some(base_url.clone());
                }

                let prompt_label = format!("{display_name} API key");
                self.setup_api_key_provider(ApiKeyProviderSpec {
                    backend: &def.id,
                    env_var,
                    secret_name,
                    prompt_label: &prompt_label,
                    hint_url: url,
                    override_display_name: Some(display_name),
                })
                .await?;
            }
            crate::llm::registry::SetupHint::Ollama { .. } => {
                self.setup_ollama_generic(def)?;
            }
            crate::llm::registry::SetupHint::OpenAiCompatible {
                secret_name,
                display_name,
                ..
            } => {
                self.setup_openai_compatible_generic(OpenAICompatSpec {
                    backend_id: &def.id,
                    secret_name,
                    display_name,
                })
                .await?;
            }
        }

        Ok(())
    }
}
