//! Database configuration: backend selection and connection settings
//! resolved from the environment.

use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

use crate::bootstrap::axinite_base_dir;
use crate::config::EnvContext;
use crate::config::helpers::{EnvKey, optional_env_from, parse_optional_env_from};
use crate::error::ConfigError;

/// Which database backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatabaseBackend {
    /// PostgreSQL via deadpool-postgres (default).
    #[default]
    Postgres,
    /// libSQL/Turso embedded database.
    LibSql,
}

impl std::fmt::Display for DatabaseBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Postgres => write!(f, "postgres"),
            Self::LibSql => write!(f, "libsql"),
        }
    }
}

impl std::str::FromStr for DatabaseBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Ok(Self::Postgres),
            "libsql" | "turso" | "sqlite" => Ok(Self::LibSql),
            _ => Err(format!(
                "invalid database backend '{}', expected 'postgres' or 'libsql'",
                s
            )),
        }
    }
}

/// PostgreSQL SSL/TLS mode, matching libpq semantics for the common cases.
///
/// Default is `Prefer`: attempt TLS, fall back to plaintext.  This is the
/// safest non-breaking default — local Postgres without TLS keeps working
/// while managed providers (Neon, Supabase, RDS) automatically get TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SslMode {
    /// Never use TLS (equivalent to libpq `sslmode=disable`).
    Disable,
    /// Try TLS first; fall back to plaintext on failure (default).
    #[default]
    Prefer,
    /// Require TLS; fail if the server does not support it.
    Require,
}

impl std::fmt::Display for SslMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disable => write!(f, "disable"),
            Self::Prefer => write!(f, "prefer"),
            Self::Require => write!(f, "require"),
        }
    }
}

impl std::str::FromStr for SslMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disable" => Ok(Self::Disable),
            "prefer" => Ok(Self::Prefer),
            "require" => Ok(Self::Require),
            _ => Err(format!(
                "invalid DATABASE_SSLMODE '{}', expected 'disable', 'prefer', or 'require'",
                s
            )),
        }
    }
}

/// Database configuration.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Which backend to use (default: Postgres).
    pub backend: DatabaseBackend,

    // -- PostgreSQL fields --
    pub url: SecretString,
    pub pool_size: usize,
    /// TLS mode for PostgreSQL connections (default: Prefer).
    pub ssl_mode: SslMode,

    // -- libSQL fields --
    /// Path to local libSQL database file (default: ~/.axinite/axinite.db).
    pub libsql_path: Option<PathBuf>,
    /// Turso cloud URL for remote sync (optional).
    pub libsql_url: Option<String>,
    /// Turso auth token (required when libsql_url is set).
    pub libsql_auth_token: Option<SecretString>,
}

impl DatabaseConfig {
    // Backwards-compatible ambient entrypoint retained for existing callers.
    pub(crate) fn resolve() -> Result<Self, ConfigError> {
        Self::resolve_from(&EnvContext::capture_ambient())
    }

    pub(crate) fn resolve_from(ctx: &EnvContext) -> Result<Self, ConfigError> {
        let backend = Self::resolve_backend(ctx)?;
        let url = Self::resolve_url(ctx, backend)?;
        let pool_size = parse_optional_env_from(ctx, EnvKey("DATABASE_POOL_SIZE"), 10)?;
        let ssl_mode = Self::resolve_ssl_mode(ctx)?;
        let libsql_path = Self::resolve_libsql_path(ctx, backend)?;
        let libsql_url = optional_env_from(ctx, EnvKey("LIBSQL_URL"))?;
        let libsql_auth_token =
            optional_env_from(ctx, EnvKey("LIBSQL_AUTH_TOKEN"))?.map(SecretString::from);

        if libsql_url.is_some() && libsql_auth_token.is_none() {
            return Err(ConfigError::MissingRequired {
                key: "LIBSQL_AUTH_TOKEN".to_string(),
                hint: "LIBSQL_AUTH_TOKEN is required when LIBSQL_URL is set".to_string(),
            });
        }

        Ok(Self {
            backend,
            url: SecretString::from(url),
            pool_size,
            ssl_mode,
            libsql_path,
            libsql_url,
            libsql_auth_token,
        })
    }

    /// Resolve the backend from DATABASE_BACKEND, defaulting when unset.
    fn resolve_backend(ctx: &EnvContext) -> Result<DatabaseBackend, ConfigError> {
        match optional_env_from(ctx, EnvKey("DATABASE_BACKEND"))? {
            Some(b) => b.parse().map_err(|e| ConfigError::InvalidValue {
                key: "DATABASE_BACKEND".to_string(),
                message: e,
            }),
            None => Ok(DatabaseBackend::default()),
        }
    }

    /// Resolve the database URL for the chosen backend.
    ///
    /// The PostgreSQL URL is required only when using the postgres backend.
    /// For the libsql backend, default to an empty placeholder.
    /// DATABASE_URL is loaded from ~/.axinite/.env via dotenvy early in startup.
    fn resolve_url(ctx: &EnvContext, backend: DatabaseBackend) -> Result<String, ConfigError> {
        optional_env_from(ctx, EnvKey("DATABASE_URL"))?
            .or_else(|| (backend == DatabaseBackend::LibSql).then(|| "unused://libsql".to_string()))
            .ok_or_else(|| ConfigError::MissingRequired {
                key: "DATABASE_URL".to_string(),
                hint: "Run 'axinite onboard' or set DATABASE_URL environment variable".to_string(),
            })
    }

    /// Resolve the TLS mode from DATABASE_SSLMODE, defaulting when unset.
    fn resolve_ssl_mode(ctx: &EnvContext) -> Result<SslMode, ConfigError> {
        match optional_env_from(ctx, EnvKey("DATABASE_SSLMODE"))? {
            Some(s) => s.parse().map_err(|e| ConfigError::InvalidValue {
                key: "DATABASE_SSLMODE".to_string(),
                message: e,
            }),
            None => Ok(SslMode::default()),
        }
    }

    /// Resolve the local libSQL path, defaulting for the libsql backend.
    fn resolve_libsql_path(
        ctx: &EnvContext,
        backend: DatabaseBackend,
    ) -> Result<Option<PathBuf>, ConfigError> {
        Ok(optional_env_from(ctx, EnvKey("LIBSQL_PATH"))?
            .map(PathBuf::from)
            .or_else(|| {
                (backend == DatabaseBackend::LibSql)
                    .then(|| default_libsql_path_in(&ctx.axinite_base_dir()))
            }))
    }

    /// Get the database URL (exposes the secret).
    pub fn url(&self) -> &str {
        self.url.expose_secret()
    }
}

const _: () = {
    let _ = DatabaseConfig::resolve;
};

impl SslMode {
    /// Read from `DATABASE_SSLMODE` env var, defaulting to `Prefer`.
    ///
    /// Silently falls back to `Prefer` on missing or unparsable values.
    /// Used by lightweight CLI tools (status, doctor) that don't run the
    /// full config pipeline.
    pub fn from_env() -> Self {
        std::env::var("DATABASE_SSLMODE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default()
    }
}

/// Default libSQL database path (~/.axinite/axinite.db).
///
/// Falls back to the pre-rename `ironclaw.db` when only the legacy file
/// exists, so an install migrated with `mv ~/.ironclaw ~/.axinite` keeps its
/// existing data reachable without a manual file rename.
pub fn default_libsql_path() -> PathBuf {
    default_libsql_path_in(&axinite_base_dir())
}

/// Resolve the default libSQL database file within `base`.
///
/// Prefers `axinite.db`; selects the legacy `ironclaw.db` only when the
/// preferred file is absent and the legacy one exists.
pub(crate) fn default_libsql_path_in(base: &Path) -> PathBuf {
    let preferred = base.join("axinite.db");
    if preferred.exists() {
        return preferred;
    }
    let legacy = base.join("ironclaw.db");
    if legacy.exists() { legacy } else { preferred }
}

#[cfg(test)]
mod tests {
    //! Unit tests for database configuration parsing and defaults.

    use super::*;

    #[test]
    fn ssl_mode_default_is_prefer() {
        assert_eq!(SslMode::default(), SslMode::Prefer);
    }

    #[test]
    fn ssl_mode_parse_roundtrip() {
        for mode in [SslMode::Disable, SslMode::Prefer, SslMode::Require] {
            let s = mode.to_string();
            let parsed: SslMode = s.parse().expect("should parse");
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn ssl_mode_parse_case_insensitive() {
        assert_eq!("DISABLE".parse::<SslMode>().unwrap(), SslMode::Disable);
        assert_eq!("Prefer".parse::<SslMode>().unwrap(), SslMode::Prefer);
        assert_eq!("REQUIRE".parse::<SslMode>().unwrap(), SslMode::Require);
    }

    #[test]
    fn ssl_mode_parse_invalid() {
        assert!("invalid".parse::<SslMode>().is_err());
    }

    #[test]
    fn default_libsql_path_prefers_axinite_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        ambient_fs::write(&dir.path().join("axinite.db"), "").expect("write");
        ambient_fs::write(&dir.path().join("ironclaw.db"), "").expect("write");
        assert_eq!(
            default_libsql_path_in(dir.path()),
            dir.path().join("axinite.db")
        );
    }

    #[test]
    fn default_libsql_path_falls_back_to_legacy_ironclaw_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        ambient_fs::write(&dir.path().join("ironclaw.db"), "").expect("write");
        assert_eq!(
            default_libsql_path_in(dir.path()),
            dir.path().join("ironclaw.db")
        );
    }

    #[test]
    fn default_libsql_path_defaults_to_axinite_db_when_neither_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            default_libsql_path_in(dir.path()),
            dir.path().join("axinite.db")
        );
    }
}
