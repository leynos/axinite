use crate::error::ConfigError;

use super::EnvContext;
use super::INJECTED_VARS;

/// A typed wrapper for an environment-variable name.
///
/// Using `EnvKey` instead of a bare `&str` makes the domain intent explicit and
/// prevents accidental argument transposition at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnvKey(pub &'static str);

impl EnvKey {
    #[inline]
    pub(crate) fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for EnvKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Shared parse logic for `parse_option_env` and `parse_option_env_from`.
/// Returns `None` when the raw value is absent; `Some(parsed)` when present and valid.
fn parse_option_core<T>(key: EnvKey, raw: Option<String>) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    raw.map(|s| {
        s.parse().map_err(|e| ConfigError::InvalidValue {
            key: key.to_string(),
            message: format!("{e}"),
        })
    })
    .transpose()
}

/// Shared parse logic for `parse_bool_env` and `parse_bool_env_from`.
fn parse_bool_core(key: EnvKey, raw: Option<String>, default: bool) -> Result<bool, ConfigError> {
    match raw {
        Some(s) => match s.to_lowercase().as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("must be 'true' or 'false', got '{s}'"),
            }),
        },
        None => Ok(default),
    }
}

/// Crate-wide mutex for tests that mutate process environment variables.
///
/// The process environment is global state shared across all threads.
/// Per-module mutexes do NOT prevent races between modules running in
/// parallel.  Every `unsafe { set_var / remove_var }` call in tests
/// MUST hold this single lock.
// Shared env-mutation guard retained for integration tests and helper modules.
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn optional_env_from(
    ctx: &EnvContext,
    key: EnvKey,
) -> Result<Option<String>, ConfigError> {
    Ok(ctx.get_owned(key.as_str()))
}

pub(crate) fn optional_env(key: EnvKey) -> Result<Option<String>, ConfigError> {
    // Check real env vars first (always win over injected secrets)
    match std::env::var(key.as_str()) {
        Ok(val) if val.is_empty() => {}
        Ok(val) => return Ok(Some(val)),
        Err(std::env::VarError::NotPresent) => {}
        Err(e) => {
            return Err(ConfigError::ParseError(format!(
                "failed to read {key}: {e}"
            )));
        }
    }

    // Fall back to thread-safe overlay (secrets injected from DB)
    if let Some(val) = INJECTED_VARS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(key.as_str())
        .cloned()
    {
        return Ok(Some(val));
    }

    Ok(None)
}

pub(crate) fn parse_optional_env_from<T>(
    ctx: &EnvContext,
    key: EnvKey,
    default: T,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_option_core(key, optional_env_from(ctx, key)?).map(|opt| opt.unwrap_or(default))
}

// Backwards-compatible ambient helper retained for existing callers.
pub(crate) fn parse_optional_env<T>(key: EnvKey, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_option_core(key, optional_env(key)?).map(|opt| opt.unwrap_or(default))
}

/// Parse a boolean from an env var with a default.
///
/// Accepts "true"/"1" as true, "false"/"0" as false.
// Backwards-compatible ambient helper retained for existing callers.
pub(crate) fn parse_bool_env(key: EnvKey, default: bool) -> Result<bool, ConfigError> {
    parse_bool_core(key, optional_env(key)?, default)
}

pub(crate) fn parse_bool_env_from(
    ctx: &EnvContext,
    key: EnvKey,
    default: bool,
) -> Result<bool, ConfigError> {
    parse_bool_core(key, optional_env_from(ctx, key)?, default)
}

/// Parse an env var into `Option<T>` — returns `None` when unset,
/// `Some(parsed)` when set to a valid value.
// Backwards-compatible ambient helper retained for existing callers.
pub(crate) fn parse_option_env<T>(key: EnvKey) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_option_core(key, optional_env(key)?)
}

pub(crate) fn parse_option_env_from<T>(
    ctx: &EnvContext,
    key: EnvKey,
) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_option_core(key, optional_env_from(ctx, key)?)
}

/// Parse a string from an env var with a default.
// Backwards-compatible ambient helper retained for existing callers.
pub(crate) fn parse_string_env(
    key: EnvKey,
    default: impl Into<String>,
) -> Result<String, ConfigError> {
    Ok(optional_env(key)?.unwrap_or_else(|| default.into()))
}

pub(crate) fn parse_string_env_from(
    ctx: &EnvContext,
    key: EnvKey,
    default: impl Into<String>,
) -> Result<String, ConfigError> {
    Ok(optional_env_from(ctx, key)?.unwrap_or_else(|| default.into()))
}

const _: () = {
    let _: fn(EnvKey, String) -> Result<String, ConfigError> = parse_optional_env::<String>;
    let _: fn(EnvKey, bool) -> Result<bool, ConfigError> = parse_bool_env;
    let _: fn(EnvKey) -> Result<Option<String>, ConfigError> = parse_option_env::<String>;
    let _: fn(EnvKey, String) -> Result<String, ConfigError> = parse_string_env;
};
