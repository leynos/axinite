//! Core diagnostic checks: settings file, NEAR AI session, LLM
//! configuration, database backend, workspace search, and workspace
//! directory.

use std::path::PathBuf;

use crate::bootstrap::axinite_base_dir;
use crate::config::EnvContext;
use crate::settings::Settings;

use super::CheckResult;

// ── Settings file ───────────────────────────────────────────

pub(super) fn check_settings_file() -> CheckResult {
    let path = Settings::default_path();
    if !path.exists() {
        return CheckResult::Pass("no settings file (defaults will be used)".into());
    }

    match ambient_fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(_) => CheckResult::Pass(format!("valid ({})", path.display())),
            Err(e) => CheckResult::Fail(format!(
                "settings.json is malformed: {}. Fix or delete {}",
                e,
                path.display()
            )),
        },
        Err(e) => CheckResult::Fail(format!("cannot read {}: {}", path.display(), e)),
    }
}

// ── NEAR AI session ─────────────────────────────────────────

pub(super) async fn check_nearai_session() -> CheckResult {
    let session_path = crate::config::llm::default_session_path();
    if !session_path.exists() {
        return missing_session_result(&session_path);
    }
    verify_session_file(&session_path)
}

/// Result when no session file exists: pass in API-key mode, otherwise
/// direct the user to onboarding.
fn missing_session_result(session_path: &std::path::Path) -> CheckResult {
    if std::env::var("NEARAI_API_KEY").is_ok() {
        return CheckResult::Pass("API key configured".into());
    }
    CheckResult::Fail(format!(
        "session file not found at {}. Run `axinite onboard`",
        session_path.display()
    ))
}

/// Verify the session file is readable and non-empty.
fn verify_session_file(session_path: &std::path::Path) -> CheckResult {
    match ambient_fs::read_to_string(session_path) {
        Ok(content) if content.trim().is_empty() => {
            CheckResult::Fail("session file is empty".into())
        }
        Ok(_) => CheckResult::Pass(format!("session found ({})", session_path.display())),
        Err(e) => CheckResult::Fail(format!("cannot read session file: {e}")),
    }
}

// ── LLM configuration ──────────────────────────────────────

pub(super) fn check_llm_config(settings: &Settings) -> CheckResult {
    check_llm_config_with_context(&EnvContext::capture_ambient(), settings)
}

pub(super) fn check_llm_config_with_context(ctx: &EnvContext, settings: &Settings) -> CheckResult {
    match crate::llm::LlmConfig::resolve_from(ctx, settings) {
        Ok(config) => {
            // Show the model for the active backend, not always nearai.model.
            let model = if let Some(ref bedrock) = config.bedrock {
                &bedrock.model
            } else if let Some(ref provider) = config.provider {
                &provider.model
            } else {
                &config.nearai.model
            };
            CheckResult::Pass(format!("backend={}, model={}", config.backend, model))
        }
        Err(e) => CheckResult::Fail(format!("LLM config error: {e}")),
    }
}

// ── Database ────────────────────────────────────────────────

pub(super) async fn check_database() -> CheckResult {
    let backend = std::env::var("DATABASE_BACKEND")
        .ok()
        .unwrap_or_else(|| "postgres".into());

    match backend.as_str() {
        "libsql" | "turso" | "sqlite" => check_libsql_database(),
        _ => check_postgres_database().await,
    }
}

/// Report whether the configured libSQL database file already exists.
fn check_libsql_database() -> CheckResult {
    let path = std::env::var("LIBSQL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::config::default_libsql_path());

    if path.exists() {
        CheckResult::Pass(format!("libSQL database exists ({})", path.display()))
    } else {
        CheckResult::Pass(format!(
            "libSQL database not found at {} (will be created on first run)",
            path.display()
        ))
    }
}

/// Attempt a PostgreSQL connection, or report the missing `DATABASE_URL`.
async fn check_postgres_database() -> CheckResult {
    if std::env::var("DATABASE_URL").is_err() {
        return CheckResult::Fail("DATABASE_URL not set".into());
    }

    match try_pg_connect().await {
        Ok(()) => CheckResult::Pass("PostgreSQL connected".into()),
        Err(e) => CheckResult::Fail(format!("PostgreSQL connection failed: {e}")),
    }
}

/// Acquire a PostgreSQL client from `DATABASE_URL` with a 5-second
/// connection timeout. Shared by the connection and pgvector checks.
#[cfg(feature = "postgres")]
async fn pg_client_from_env() -> Result<deadpool_postgres::Client, String> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set".to_string())?;

    let config = deadpool_postgres::Config {
        url: Some(url),
        ..Default::default()
    };
    let pool = crate::db::tls::create_pool(&config, crate::config::SslMode::from_env())
        .map_err(|e| format!("pool error: {e}"))?;

    tokio::time::timeout(std::time::Duration::from_secs(5), pool.get())
        .await
        .map_err(|_| "connection timeout (5s)".to_string())?
        .map_err(|e| format!("{e}"))
}

#[cfg(feature = "postgres")]
async fn try_pg_connect() -> Result<(), String> {
    let client = pg_client_from_env().await?;

    client
        .execute("SELECT 1", &[])
        .await
        .map_err(|e| format!("{e}"))?;

    Ok(())
}

#[cfg(not(feature = "postgres"))]
async fn try_pg_connect() -> Result<(), String> {
    Err("postgres feature not compiled in".into())
}

// ── Workspace search ────────────────────────────────────────

pub(super) async fn check_workspace_search() -> CheckResult {
    let backend = std::env::var("DATABASE_BACKEND")
        .ok()
        .unwrap_or_else(|| "postgres".into());

    match backend.as_str() {
        "libsql" | "turso" | "sqlite" => {
            // libSQL uses brute-force cosine similarity after V9 migration
            CheckResult::Pass("hybrid search (brute-force cosine)".into())
        }
        _ => {
            // PostgreSQL with pgvector
            #[cfg(feature = "postgres")]
            {
                if std::env::var("DATABASE_URL").is_ok() {
                    match try_pgvector_check().await {
                        Ok(()) => CheckResult::Pass("hybrid search (pgvector)".into()),
                        Err(e) => {
                            CheckResult::Fail(format!("pgvector extension check failed: {}", e))
                        }
                    }
                } else {
                    CheckResult::Skip("DATABASE_URL not set".into())
                }
            }
            #[cfg(not(feature = "postgres"))]
            {
                CheckResult::Skip("postgres feature not compiled in".into())
            }
        }
    }
}

#[cfg(feature = "postgres")]
async fn try_pgvector_check() -> Result<(), String> {
    let client = pg_client_from_env().await?;

    // Check if pgvector extension is available
    let row = client
        .query_one("SELECT 1 FROM pg_extension WHERE extname = 'vector'", &[])
        .await
        .map_err(|e| format!("pgvector extension not found: {e}"))?;

    drop(row);
    Ok(())
}

// ── Workspace directory ─────────────────────────────────────

pub(super) fn check_workspace_dir() -> CheckResult {
    let dir = axinite_base_dir();

    if dir.exists() {
        if dir.is_dir() {
            CheckResult::Pass(format!("{}", dir.display()))
        } else {
            CheckResult::Fail(format!("{} exists but is not a directory", dir.display()))
        }
    } else {
        CheckResult::Pass(format!("{} will be created on first run", dir.display()))
    }
}
