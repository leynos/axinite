//! Core diagnostic checks: settings file, NEAR AI session, LLM
//! configuration, database backend, workspace search, and workspace
//! directory.

use std::path::PathBuf;

use crate::config::EnvContext;
use crate::settings::Settings;

use super::CheckResult;

// ── Settings file ───────────────────────────────────────────

/// Report whether the settings file is absent, valid, or invalid.
///
/// Validation deserializes [`Settings`] rather than a generic JSON value, so a
/// file that startup would reject (for example, a field with the wrong type)
/// is reported as a failure here too.
pub(super) fn check_settings_file(ctx: &EnvContext) -> CheckResult {
    let path = ctx.axinite_base_dir().join("settings.json");
    match path.try_exists() {
        Ok(false) => return CheckResult::Pass("no settings file (defaults will be used)".into()),
        Ok(true) => {}
        Err(error) => {
            return CheckResult::Fail(format!("cannot inspect {}: {error}", path.display()));
        }
    }

    match Settings::load_from(&path) {
        Ok(_) => CheckResult::Pass(format!("valid ({})", path.display())),
        Err(error) => CheckResult::Fail(format!(
            "settings.json is invalid: {error}. Fix or delete {}",
            path.display()
        )),
    }
}

// ── NEAR AI session ─────────────────────────────────────────

pub(super) async fn check_nearai_session(ctx: &EnvContext) -> CheckResult {
    let session_path = ctx.axinite_base_dir().join("session.json");
    if !session_path.exists() {
        return missing_session_result(ctx, &session_path);
    }
    verify_session_file(&session_path)
}

/// Result when no session file exists: pass in API-key mode, otherwise
/// direct the user to onboarding.
fn missing_session_result(ctx: &EnvContext, session_path: &std::path::Path) -> CheckResult {
    if ctx.contains_env_var("NEARAI_API_KEY") {
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

/// Resolve the LLM configuration from the supplied snapshot and report the
/// active backend and model.
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
pub(super) async fn check_database_with_context(ctx: &EnvContext) -> CheckResult {
    let backend = crate::bootstrap::database_backend_name(ctx);

    match backend.as_str() {
        "libsql" | "turso" | "sqlite" => check_libsql_database(ctx),
        _ => check_postgres_database(ctx).await,
    }
}

/// Report whether the configured libSQL database file already exists.
fn check_libsql_database(ctx: &EnvContext) -> CheckResult {
    let path = ctx
        .get_owned("LIBSQL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::default_libsql_path_in(&ctx.axinite_base_dir()));

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
async fn check_postgres_database(ctx: &EnvContext) -> CheckResult {
    if ctx.get("DATABASE_URL").is_none() {
        return CheckResult::Fail("DATABASE_URL not set".into());
    }

    match try_pg_connect(ctx).await {
        Ok(()) => CheckResult::Pass("PostgreSQL connected".into()),
        Err(e) => CheckResult::Fail(format!("PostgreSQL connection failed: {e}")),
    }
}

/// Acquire a PostgreSQL client from `DATABASE_URL` with a 5-second
/// connection timeout. Shared by the connection and pgvector checks.
#[cfg(feature = "postgres")]
async fn pg_client_from_env(ctx: &EnvContext) -> Result<deadpool_postgres::Client, String> {
    let url = ctx
        .get("DATABASE_URL")
        .ok_or_else(|| "DATABASE_URL not set".to_string())?;

    let config = deadpool_postgres::Config {
        url: Some(url.to_string()),
        ..Default::default()
    };
    let pool = crate::db::tls::create_pool(&config, crate::config::SslMode::from_context(ctx))
        .map_err(|e| format!("pool error: {e}"))?;

    tokio::time::timeout(std::time::Duration::from_secs(5), pool.get())
        .await
        .map_err(|_| "connection timeout (5s)".to_string())?
        .map_err(|e| format!("{e}"))
}

#[cfg(feature = "postgres")]
async fn try_pg_connect(ctx: &EnvContext) -> Result<(), String> {
    let client = pg_client_from_env(ctx).await?;

    client
        .execute("SELECT 1", &[])
        .await
        .map_err(|e| format!("{e}"))?;

    Ok(())
}

#[cfg(not(feature = "postgres"))]
async fn try_pg_connect(_ctx: &EnvContext) -> Result<(), String> {
    Err("postgres feature not compiled in".into())
}

// ── Workspace search ────────────────────────────────────────

/// Report the workspace search mode for the configured database backend.
pub(super) async fn check_workspace_search_with_context(ctx: &EnvContext) -> CheckResult {
    let backend = crate::bootstrap::database_backend_name(ctx);

    match backend.as_str() {
        "libsql" | "turso" | "sqlite" => {
            // libSQL uses brute-force cosine similarity after V9 migration
            CheckResult::Pass("hybrid search (brute-force cosine)".into())
        }
        _ => {
            // PostgreSQL with pgvector
            #[cfg(feature = "postgres")]
            {
                if ctx.get("DATABASE_URL").is_some() {
                    match try_pgvector_check(ctx).await {
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
async fn try_pgvector_check(ctx: &EnvContext) -> Result<(), String> {
    let client = pg_client_from_env(ctx).await?;

    // Check if pgvector extension is available
    let row = client
        .query_one("SELECT 1 FROM pg_extension WHERE extname = 'vector'", &[])
        .await
        .map_err(|e| format!("pgvector extension not found: {e}"))?;

    drop(row);
    Ok(())
}

// ── Workspace directory ─────────────────────────────────────

/// Report whether the Axinite base directory exists and is a directory.
pub(super) fn check_workspace_dir(ctx: &EnvContext) -> CheckResult {
    let dir = ctx.axinite_base_dir();

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
