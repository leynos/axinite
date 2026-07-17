//! Subsystem diagnostic checks: embeddings, routines, gateway, MCP
//! servers, skills, and secrets configuration.

use crate::bootstrap::axinite_base_dir;
use crate::config::EnvContext;
use crate::settings::Settings;

use super::CheckResult;

// ── Embeddings ──────────────────────────────────────────────

pub(super) fn check_embeddings(settings: &Settings) -> CheckResult {
    check_embeddings_with_context(&EnvContext::capture_ambient(), settings)
}

pub(super) fn check_embeddings_with_context(ctx: &EnvContext, settings: &Settings) -> CheckResult {
    let config = match crate::config::EmbeddingsConfig::resolve_from(ctx, settings) {
        Ok(config) => config,
        Err(e) => return CheckResult::Fail(format!("config error: {e}")),
    };
    if !config.enabled {
        return CheckResult::Skip("disabled (set EMBEDDING_ENABLED=true)".into());
    }
    if embeddings_credentials_present(&config) {
        CheckResult::Pass(format!(
            "provider={}, model={}",
            config.provider, config.model
        ))
    } else {
        let hint = match config.provider.as_str() {
            "nearai" => "run `axinite onboard` to create a session",
            _ => "set OPENAI_API_KEY",
        };
        CheckResult::Fail(format!(
            "provider={} but credentials missing ({})",
            config.provider, hint
        ))
    }
}

/// Whether credentials for the configured embeddings provider are present.
fn embeddings_credentials_present(config: &crate::config::EmbeddingsConfig) -> bool {
    match config.provider.as_str() {
        "openai" => config.openai_api_key().is_some(),
        "nearai" => nearai_session_present(),
        "ollama" => true, // local, no creds needed
        _ => config.openai_api_key().is_some(),
    }
}

/// Whether a non-empty NEAR AI session file exists.
///
/// NearAiEmbeddings uses SessionManager::get_token() which only returns
/// session tokens, NOT NEARAI_API_KEY
/// (src/workspace/embeddings.rs:309, src/llm/session.rs:132).
fn nearai_session_present() -> bool {
    let session_path = crate::config::llm::default_session_path();
    session_path.exists()
        && ambient_fs::read_to_string(&session_path)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}

// ── Routines config ─────────────────────────────────────────

pub(super) fn check_routines_config() -> CheckResult {
    check_routines_config_with_context(&EnvContext::capture_ambient())
}

pub(super) fn check_routines_config_with_context(ctx: &EnvContext) -> CheckResult {
    match crate::config::RoutineConfig::resolve_from(ctx) {
        Ok(config) => {
            if config.enabled {
                CheckResult::Pass(format!(
                    "enabled (interval={}s, max_concurrent={})",
                    config.cron_check_interval_secs, config.max_concurrent_routines
                ))
            } else {
                CheckResult::Skip("disabled".into())
            }
        }
        Err(e) => CheckResult::Fail(format!("config error: {e}")),
    }
}

// ── Gateway config ──────────────────────────────────────────

pub(super) fn check_gateway_config(settings: &Settings) -> CheckResult {
    // Use the same resolve() path as runtime so invalid env values
    // (e.g. GATEWAY_PORT=abc) are caught here too.
    match crate::config::ChannelsConfig::resolve(settings) {
        Ok(channels) => match channels.gateway {
            Some(gw) => {
                if gw.auth_token.is_some() {
                    CheckResult::Pass(format!(
                        "enabled at {}:{} (auth token set)",
                        gw.host, gw.port
                    ))
                } else {
                    CheckResult::Pass(format!(
                        "enabled at {}:{} (no auth token — random token will be generated)",
                        gw.host, gw.port
                    ))
                }
            }
            None => CheckResult::Skip("disabled (GATEWAY_ENABLED=false)".into()),
        },
        Err(e) => CheckResult::Fail(format!("config error: {e}")),
    }
}

// ── MCP servers ─────────────────────────────────────────────

pub(super) async fn check_mcp_config() -> CheckResult {
    let file = match crate::tools::mcp::config::load_mcp_servers().await {
        Ok(file) => file,
        Err(e) => return mcp_load_failure(&e.to_string()),
    };

    let servers: Vec<_> = file.enabled_servers().collect();
    if servers.is_empty() {
        return CheckResult::Skip("no MCP servers configured".into());
    }
    validate_mcp_servers(&servers)
}

/// Map an MCP config load error, distinguishing a missing config file
/// (skip) from a corrupted one (fail).
fn mcp_load_failure(msg: &str) -> CheckResult {
    if msg.contains("not found") || msg.contains("No such file") {
        CheckResult::Skip("no MCP config file".into())
    } else {
        CheckResult::Fail(format!("config error: {msg}"))
    }
}

/// Validate each configured server, summarizing any invalid entries.
fn validate_mcp_servers(servers: &[&crate::tools::mcp::config::McpServerConfig]) -> CheckResult {
    let invalid: Vec<String> = servers
        .iter()
        .filter_map(|server| {
            server
                .validate()
                .err()
                .map(|e| format!("{}: {}", server.name, e))
        })
        .collect();

    if invalid.is_empty() {
        CheckResult::Pass(format!("{} server(s) configured, all valid", servers.len()))
    } else {
        CheckResult::Fail(format!(
            "{} server(s), {} invalid: {}",
            servers.len(),
            invalid.len(),
            invalid.join("; ")
        ))
    }
}

// ── Skills ──────────────────────────────────────────────────

pub(super) async fn check_skills() -> CheckResult {
    let user_dir = axinite_base_dir().join("skills");
    let installed_dir = axinite_base_dir().join("installed_skills");

    let mut registry = crate::skills::SkillRegistry::new(user_dir.clone());
    registry = registry.with_installed_dir(installed_dir);

    // discover_all() returns loaded skill names (not warnings).
    let _loaded_names = registry.discover_all().await;

    let count = registry.count();
    if count == 0 {
        return CheckResult::Skip("no skills discovered".into());
    }

    CheckResult::Pass(format!("{count} skill(s) loaded"))
}

// ── Secrets ─────────────────────────────────────────────────

pub(super) fn check_secrets(settings: &Settings) -> CheckResult {
    match settings.secrets_master_key_source {
        crate::settings::KeySource::Keychain => {
            CheckResult::Pass("master key source: OS keychain".into())
        }
        crate::settings::KeySource::Env => {
            if std::env::var("SECRETS_MASTER_KEY").is_ok() {
                CheckResult::Pass("master key source: env var (set)".into())
            } else {
                CheckResult::Fail(
                    "master key source: env var but SECRETS_MASTER_KEY not set".into(),
                )
            }
        }
        crate::settings::KeySource::None => {
            CheckResult::Skip("secrets not configured (run `axinite onboard`)".into())
        }
    }
}
