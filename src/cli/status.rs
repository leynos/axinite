//! System health and diagnostics CLI command.
//!
//! Checks database connectivity, session validity, embeddings,
//! WASM runtime, tool count, and channel availability.

use std::path::PathBuf;

use super::report::{Report, out, outln};
use crate::config::EnvContext;
use crate::settings::Settings;

/// Load settings from JSON and TOML config files, matching the runtime
/// priority: TOML overlay > settings.json > defaults.
///
/// This mirrors the loading chain in `Config::from_env_with_toml()` but
/// without resolving the full `Config` (which requires async + secrets).
/// Paths are derived from the caller's environment snapshot rather than the
/// ambient process environment.
fn load_settings_from(json_path: &std::path::Path, toml_path: &std::path::Path) -> Settings {
    let mut settings = match Settings::load_from(json_path) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Warning: {error}; continuing with default settings");
            Settings::default()
        }
    };

    match Settings::load_toml(toml_path) {
        Ok(Some(toml_settings)) => {
            settings.merge_from(&toml_settings);
        }
        Ok(None) => {} // File not found — fine for default path
        Err(e) => {
            eprintln!("Warning: failed to parse {}: {}", toml_path.display(), e);
        }
    }

    settings
}

fn default_tools_dir(ctx: &EnvContext) -> PathBuf {
    ctx.axinite_base_dir().join("tools")
}

fn default_channels_dir(ctx: &EnvContext) -> PathBuf {
    ctx.axinite_base_dir().join("channels")
}
fn db_backend_name(ctx: &EnvContext) -> String {
    crate::bootstrap::database_backend_name(ctx)
}

/// Render the libSQL database path and whether Turso sync is configured.
fn print_libsql_status<W: std::io::Write>(ctx: &EnvContext, report: &mut Report<W>) {
    let path = ctx
        .get("LIBSQL_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::config::default_libsql_path_in(&ctx.axinite_base_dir()));
    if path.exists() {
        let turso = if ctx.contains_env_var("LIBSQL_URL") {
            " + Turso sync"
        } else {
            ""
        };
        outln!(report, "libSQL ({}{})", path.display(), turso);
    } else {
        outln!(report, "libSQL (file missing: {})", path.display());
    }
}

/// Render the database backend and, for PostgreSQL, whether a connection
/// succeeds.
async fn print_database_status<W: std::io::Write>(
    ctx: &EnvContext,
    db_backend: &str,
    report: &mut Report<W>,
) {
    out!(report, "  Database:    ");
    match db_backend {
        "libsql" | "turso" | "sqlite" => print_libsql_status(ctx, report),
        _ => {
            if ctx.contains_env_var("DATABASE_URL") {
                match check_database(ctx).await {
                    Ok(()) => outln!(report, "connected (PostgreSQL)"),
                    Err(e) => outln!(report, "error ({})", e),
                }
            } else {
                outln!(report, "not configured");
            }
        }
    }
}

fn print_search_status<W: std::io::Write>(db_backend: &str, report: &mut Report<W>) {
    out!(report, "  Search:      ");
    match db_backend {
        "libsql" | "turso" | "sqlite" => {
            outln!(report, "hybrid (brute-force cosine)");
        }
        _ => {
            outln!(report, "hybrid (pgvector)");
        }
    }
}

/// Render whether the NEAR AI session file exists in the configured base
/// directory.
fn print_session_status<W: std::io::Write>(ctx: &EnvContext, report: &mut Report<W>) {
    out!(report, "  Session:     ");
    let session_path = ctx.axinite_base_dir().join("session.json");
    if session_path.exists() {
        outln!(report, "found ({})", session_path.display());
    } else {
        outln!(report, "not found (run `axinite onboard`)");
    }
}

/// Render whether a secrets master key is configured in the snapshot.
fn print_secrets_status<W: std::io::Write>(ctx: &EnvContext, report: &mut Report<W>) {
    // Secrets (auto-detect from env only; skip keychain probe to avoid
    // triggering macOS system password dialogs on a simple status check)
    out!(report, "  Secrets:     ");
    if ctx.contains_env_var("SECRETS_MASTER_KEY") {
        outln!(report, "configured (env)");
    } else {
        // We don't probe the keychain here because get_generic_password()
        // triggers macOS unlock+authorisation dialogs, which is bad UX for
        // a read-only status command. If onboarding completed with keychain
        // storage, the key is there; we just can't cheaply verify it.
        outln!(report, "env not set (keychain may be configured)");
    }
}
/// Render whether embeddings are enabled, treating an OpenAI API key or an
/// explicit `EMBEDDING_ENABLED` flag as enabling them, and show the provider
/// and model.
fn print_embeddings_status<W: std::io::Write>(
    ctx: &EnvContext,
    settings: &Settings,
    report: &mut Report<W>,
) {
    out!(report, "  Embeddings:  ");
    let emb_enabled = settings.embeddings.enabled
        || ctx.contains_env_var("OPENAI_API_KEY")
        || ctx.get("EMBEDDING_ENABLED") == Some("true");
    if emb_enabled {
        outln!(
            report,
            "enabled (provider: {}, model: {})",
            settings.embeddings.provider,
            settings.embeddings.model
        );
    } else {
        outln!(report, "disabled");
    }
}

/// Render how many `.wasm` tools are installed in `tools_dir`, or report that
/// the directory is missing.
fn print_wasm_tools_status<W: std::io::Write>(tools_dir: &std::path::Path, report: &mut Report<W>) {
    out!(report, "  WASM Tools:  ");
    if tools_dir.exists() {
        let count = count_wasm_files(tools_dir);
        outln!(report, "{} installed ({})", count, tools_dir.display());
    } else {
        outln!(report, "directory not found ({})", tools_dir.display());
    }
}

/// Render the enabled channels and the WASM channel directory.
fn print_channels_status<W: std::io::Write>(
    ctx: &EnvContext,
    settings: &Settings,
    report: &mut Report<W>,
) {
    out!(report, "  Channels:    ");
    let channels_dir = settings
        .channels
        .wasm_channels_dir
        .clone()
        .unwrap_or_else(|| default_channels_dir(ctx));
    let mut channel_info = vec!["cli".to_string()];
    if settings.channels.http_enabled {
        channel_info.push(format!(
            "http:{}",
            settings.channels.http_port.unwrap_or(3000)
        ));
    }
    if channels_dir.exists() {
        let wasm_count = count_wasm_files(&channels_dir);
        if wasm_count > 0 {
            channel_info.push(format!("{} wasm", wasm_count));
        }
    }
    outln!(report, "{}", channel_info.join(", "));
}
/// Render whether the heartbeat is enabled, honouring the `HEARTBEAT_ENABLED`
/// environment override, and show its interval when enabled.
fn print_heartbeat_status<W: std::io::Write>(
    ctx: &EnvContext,
    settings: &Settings,
    report: &mut Report<W>,
) {
    out!(report, "  Heartbeat:   ");
    let hb_enabled = settings.heartbeat.enabled || ctx.get("HEARTBEAT_ENABLED") == Some("true");
    if hb_enabled {
        outln!(
            report,
            "enabled (interval: {}s)",
            settings.heartbeat.interval_secs
        );
    } else {
        outln!(report, "disabled");
    }
}

/// Render how many configured MCP servers are enabled, or report that none are
/// configured.
async fn print_mcp_status<W: std::io::Write>(report: &mut Report<W>) {
    out!(report, "  MCP Servers: ");
    match crate::tools::mcp::config::load_mcp_servers().await {
        Ok(servers) => {
            let enabled = servers.servers.iter().filter(|s| s.enabled).count();
            let total = servers.servers.len();
            outln!(report, "{} enabled / {} configured", enabled, total);
        }
        Err(_) => outln!(report, "none configured"),
    }
}

/// Load status settings from the supplied snapshot's base directory.
///
/// The snapshot selects the settings files, so a caller that supplies a
/// divergent `AXINITE_BASE_DIR` observes that directory's settings rather than
/// the ambient ones.
pub(crate) fn settings_for_context(env_context: &EnvContext) -> Settings {
    let base_dir = env_context.axinite_base_dir();
    load_settings_from(
        &base_dir.join("settings.json"),
        &base_dir.join("config.toml"),
    )
}

/// Render the status report for the supplied environment snapshot.
///
/// Settings, paths, and database selection all come from `env_context`, so a
/// caller that supplies a divergent `AXINITE_BASE_DIR` sees that deployment's
/// state. The returned text is exactly what [`run_status_command`] prints.
#[cfg(test)]
pub(crate) async fn status_report(env_context: &EnvContext) -> String {
    let mut report = Report::new(Vec::new());
    render_status_report(env_context, &mut report).await;
    report.into_string()
}

/// Stream the status report to `report` as each section is produced.
async fn render_status_report<W: std::io::Write>(env_context: &EnvContext, report: &mut Report<W>) {
    let base_dir = env_context.axinite_base_dir();
    let settings = settings_for_context(env_context);

    outln!(report, "Axinite Status");
    outln!(report, "===============\n");

    // Version
    outln!(
        report,
        "  Version:     {} v{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let db_backend = db_backend_name(env_context);
    print_database_status(env_context, &db_backend, report).await;
    print_search_status(&db_backend, report);
    print_session_status(env_context, report);
    print_secrets_status(env_context, report);
    print_embeddings_status(env_context, &settings, report);
    print_wasm_tools_status(
        &settings
            .wasm
            .tools_dir
            .clone()
            .unwrap_or_else(|| default_tools_dir(env_context)),
        report,
    );
    print_channels_status(env_context, &settings, report);
    print_heartbeat_status(env_context, &settings, report);
    print_mcp_status(report).await;

    // Config path
    outln!(
        report,
        "\n  Config:      {}",
        base_dir.join(".env").display()
    );
}

/// Run the status command, printing system health info.
pub async fn run_status_command() -> anyhow::Result<()> {
    let env_context = crate::bootstrap::capture_config_env_context();
    let mut report = Report::new(std::io::stdout().lock());
    render_status_report(&env_context, &mut report).await;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn check_database(ctx: &EnvContext) -> anyhow::Result<()> {
    let url = ctx
        .get("DATABASE_URL")
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL not set"))?;

    let config: deadpool_postgres::Config = deadpool_postgres::Config {
        url: Some(url.to_string()),
        ..Default::default()
    };
    let pool = crate::db::tls::create_pool(&config, crate::config::SslMode::from_context(ctx))
        .map_err(|e| anyhow::anyhow!("pool error: {}", e))?;

    let client = tokio::time::timeout(std::time::Duration::from_secs(5), pool.get())
        .await
        .map_err(|_| anyhow::anyhow!("timeout"))?
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    client
        .execute("SELECT 1", &[])
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}

#[cfg(not(feature = "postgres"))]
async fn check_database(_ctx: &EnvContext) -> anyhow::Result<()> {
    // For non-postgres backends, just report configured
    Ok(())
}

fn count_wasm_files(dir: &std::path::Path) -> usize {
    ambient_fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "wasm"))
                .count()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
