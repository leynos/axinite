//! Helpers that keep phase assembly readable: credential resolution and
//! media tool registration.

use std::sync::Arc;

use crate::tools::ToolRegistry;
use crate::tools::{ImageToolsRegistration, VisionToolsRegistration};

use super::builder::AppBuilder;

impl AppBuilder {
    /// Resolve the API base URL and (optional) key used for image and vision
    /// tools, preferring the explicit provider config over NEAR AI defaults.
    pub(super) fn media_api_credentials(&self) -> (String, Option<String>) {
        use secrecy::ExposeSecret;
        if let Some(ref provider) = self.config.llm.provider {
            (
                provider.base_url.clone(),
                provider
                    .api_key
                    .as_ref()
                    .map(|s| s.expose_secret().to_string()),
            )
        } else {
            (
                self.config.llm.nearai.base_url.clone(),
                self.config
                    .llm
                    .nearai
                    .api_key
                    .as_ref()
                    .map(|s| s.expose_secret().to_string()),
            )
        }
    }

    /// Register image generation and vision tools when API credentials are
    /// available.
    pub(super) fn register_media_tools(&self, tools: &Arc<ToolRegistry>) {
        let (api_base, api_key_opt) = self.media_api_credentials();
        let Some(api_key) = api_key_opt else {
            return;
        };

        // Check for image generation models
        let model_name = self
            .config
            .llm
            .provider
            .as_ref()
            .map(|p| p.model.clone())
            .unwrap_or_else(|| self.config.llm.nearai.model.clone());
        let models = vec![model_name.clone()];
        let gen_model = crate::llm::image_models::suggest_image_model(&models)
            .unwrap_or("flux-1.1-pro")
            .to_string();
        tools.register_image_tools(ImageToolsRegistration::new(
            api_base.clone(),
            api_key.clone(),
            gen_model,
        ));

        // Check for vision models
        let vision_model = crate::llm::vision_models::suggest_vision_model(&models)
            .unwrap_or(&model_name)
            .to_string();
        tools.register_vision_tools(VisionToolsRegistration {
            api_base_url: api_base,
            api_key,
            vision_model,
            base_dir: None,
        });
    }
}
