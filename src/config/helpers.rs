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

pub(crate) fn intern_env_key(key: &str) -> EnvKey {
    static INTERNED_KEYS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, &'static str>>,
    > = std::sync::OnceLock::new();

    let mut keys = INTERNED_KEYS
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = keys.get(key) {
        return EnvKey(existing);
    }

    let leaked = Box::leak(key.to_string().into_boxed_str());
    keys.insert(leaked.to_string(), leaked);
    EnvKey(leaked)
}

/// Crate-wide mutex for tests that mutate process environment variables.
///
/// The process environment is global state shared across all threads.
/// Per-module mutexes do NOT prevent races between modules running in
/// parallel.  Every `unsafe { set_var / remove_var }` call in tests
/// MUST hold this single lock.
// Shared env-mutation guard retained for integration tests and helper modules.
#[allow(dead_code)]
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
    optional_env_from(ctx, key)?
        .map(|s| {
            s.parse().map_err(|e| ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("{e}"),
            })
        })
        .transpose()
        .map(|opt| opt.unwrap_or(default))
}

// Backwards-compatible ambient helper retained for existing callers.
#[allow(dead_code)]
pub(crate) fn parse_optional_env<T>(key: EnvKey, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    optional_env(key)?
        .map(|s| {
            s.parse().map_err(|e| ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("{e}"),
            })
        })
        .transpose()
        .map(|opt| opt.unwrap_or(default))
}

/// Parse a boolean from an env var with a default.
///
/// Accepts "true"/"1" as true, "false"/"0" as false.
// Backwards-compatible ambient helper retained for existing callers.
#[allow(dead_code)]
pub(crate) fn parse_bool_env(key: EnvKey, default: bool) -> Result<bool, ConfigError> {
    match optional_env(key)? {
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

pub(crate) fn parse_bool_env_from(
    ctx: &EnvContext,
    key: EnvKey,
    default: bool,
) -> Result<bool, ConfigError> {
    match optional_env_from(ctx, key)? {
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

/// Parse an env var into `Option<T>` — returns `None` when unset,
/// `Some(parsed)` when set to a valid value.
// Backwards-compatible ambient helper retained for existing callers.
#[allow(dead_code)]
pub(crate) fn parse_option_env<T>(key: EnvKey) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    optional_env(key)?
        .map(|s| {
            s.parse().map_err(|e| ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("{e}"),
            })
        })
        .transpose()
}

pub(crate) fn parse_option_env_from<T>(
    ctx: &EnvContext,
    key: EnvKey,
) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    optional_env_from(ctx, key)?
        .map(|s| {
            s.parse().map_err(|e| ConfigError::InvalidValue {
                key: key.to_string(),
                message: format!("{e}"),
            })
        })
        .transpose()
}

/// Parse a string from an env var with a default.
// Backwards-compatible ambient helper retained for existing callers.
#[allow(dead_code)]
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
