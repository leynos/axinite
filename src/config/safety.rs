use crate::config::EnvContext;
use crate::config::helpers::{EnvKey, parse_bool_env_from, parse_optional_env_from};
use crate::error::ConfigError;

/// Safety configuration.
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub max_output_length: usize,
    pub injection_check_enabled: bool,
}

impl SafetyConfig {
    // Backwards-compatible ambient entrypoint retained for existing callers.
    #[allow(dead_code)]
    pub(crate) fn resolve() -> Result<Self, ConfigError> {
        Self::resolve_from(&EnvContext::capture_ambient())
    }

    pub(crate) fn resolve_from(ctx: &EnvContext) -> Result<Self, ConfigError> {
        Ok(Self {
            max_output_length: parse_optional_env_from(
                ctx,
                EnvKey("SAFETY_MAX_OUTPUT_LENGTH"),
                100_000,
            )?,
            injection_check_enabled: parse_bool_env_from(
                ctx,
                EnvKey("SAFETY_INJECTION_CHECK_ENABLED"),
                true,
            )?,
        })
    }
}
