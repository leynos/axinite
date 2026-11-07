//! Shared test utilities for explicit configuration snapshots.
use crate::config::{Config, EnvContext};
use crate::settings::Settings;

/// Create an empty explicit config context for tests.
///
/// Use this when a test wants to build configuration without mutating the
/// host process environment or depending on machine-local credentials.
pub fn test_env_context() -> EnvContext {
    EnvContext::default()
}

/// Create a test context populated with explicit env values.
///
/// This is the preferred helper for tests that need to express precedence
/// through concrete env-var inputs while keeping resolution deterministic.
pub fn test_env_context_with(vars: &[(&str, &str)]) -> EnvContext {
    vars.iter()
        .fold(EnvContext::default(), |ctx, (key, value)| {
            ctx.with_env(*key, *value)
        })
}

/// Build a config from an explicit test context.
///
/// The config is resolved through [`Config::from_context`] using
/// [`Settings::default`], so tests can exercise the explicit snapshot path
/// without touching ambient state.
pub async fn test_config_from_context(
    ctx: &EnvContext,
) -> Result<Config, crate::error::ConfigError> {
    Config::from_context(ctx, &Settings::default()).await
}
