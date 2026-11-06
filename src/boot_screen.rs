//! Boot screen displayed after all initialization completes.
//!
//! Shows a polished ANSI-styled status panel summarizing the agent's runtime
//! state: model, database, tool count, enabled features, active channels,
//! and the gateway URL.

use crate::cli::Cli;
use crate::config::Config;
use crate::sandbox::detect::DockerStatus;
use crate::tunnel::Tunnel;

/// Runtime-computed values used to populate the startup boot screen.
pub struct BootData<'a> {
    /// Primary LLM model identifier shown on the model line.
    pub llm_model: String,
    /// Optional cheaper model identifier. `None` omits the `cheap` label.
    pub cheap_model: Option<String>,
    /// Number of tools registered with the agent at startup.
    pub tool_count: usize,
    /// Gateway web-UI URL, present only when the gateway channel is active.
    pub gateway_url: Option<String>,
    /// Docker availability, used to pick the sandbox feature label.
    pub docker_status: crate::sandbox::detect::DockerStatus,
    /// Names of the enabled channels, shown on the channels line.
    pub channel_names: Vec<String>,
    /// Live tunnel handle. A public URL it reports takes precedence over the
    /// static `tunnel.public_url` from configuration.
    pub active_tunnel: &'a Option<Box<dyn Tunnel>>,
}

/// All displayable fields for the boot screen.
pub struct BootInfo {
    /// Crate version from `CARGO_PKG_VERSION`, shown after the agent name.
    pub version: String,
    /// Configured agent name, used as the banner title.
    pub agent_name: String,
    /// LLM backend identifier, shown as `via {backend}` on the model line.
    pub llm_backend: String,
    /// Primary LLM model identifier.
    pub llm_model: String,
    /// Cheaper model identifier, shown with a `cheap` label when set.
    pub cheap_model: Option<String>,
    /// Database backend name, or `none` when the run skips the database.
    pub db_backend: String,
    /// Whether the database is connected. `false` renders the status as `none`.
    pub db_connected: bool,
    /// Number of registered tools, shown on the tools line.
    pub tool_count: usize,
    /// Gateway web-UI URL, sanitised for display. `None` when no gateway is active.
    pub gateway_url: Option<String>,
    /// Whether embeddings are enabled. Adds an `embeddings` feature label.
    pub embeddings_enabled: bool,
    /// Embedding provider name. Present only when embeddings are enabled.
    pub embeddings_provider: Option<String>,
    /// Whether the heartbeat task is enabled. Adds a `heartbeat` feature label.
    pub heartbeat_enabled: bool,
    /// Heartbeat interval in seconds. The label shows whole minutes, rounded down.
    pub heartbeat_interval_secs: u64,
    /// Whether sandboxing is enabled in configuration. This struct records the flag;
    /// the rendered sandbox label is driven by `docker_status`.
    pub sandbox_enabled: bool,
    /// Docker availability. Selects the sandbox feature label, or no label when disabled.
    pub docker_status: crate::sandbox::detect::DockerStatus,
    /// Whether Claude Code integration is enabled. Adds a `claude-code` feature label.
    pub claude_code_enabled: bool,
    /// Whether scheduled routines are enabled. Adds a `routines` feature label.
    pub routines_enabled: bool,
    /// Whether skills are enabled. Adds a `skills` feature label.
    pub skills_enabled: bool,
    /// Names of the active channels, shown on the channels line when non-empty.
    pub channels: Vec<String>,
    /// Public URL from a managed tunnel (e.g., "https://abc.ngrok.io").
    pub tunnel_url: Option<String>,
    /// Provider name for the managed tunnel (e.g., "ngrok").
    pub tunnel_provider: Option<String>,
}

impl BootInfo {
    /// Build a boot-screen view model from config and runtime startup data.
    pub fn from_config_and_data(config: &Config, cli: &Cli, data: &BootData<'_>) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            agent_name: config.agent.name.clone(),
            llm_backend: config.llm.backend.to_string(),
            llm_model: data.llm_model.clone(),
            cheap_model: data.cheap_model.clone(),
            db_backend: if cli.no_db {
                "none".to_string()
            } else {
                config.database.backend.to_string()
            },
            db_connected: !cli.no_db,
            tool_count: data.tool_count,
            gateway_url: data.gateway_url.clone(),
            embeddings_enabled: config.embeddings.enabled,
            embeddings_provider: config
                .embeddings
                .enabled
                .then(|| config.embeddings.provider.clone()),
            heartbeat_enabled: config.heartbeat.enabled,
            heartbeat_interval_secs: config.heartbeat.interval_secs,
            sandbox_enabled: config.sandbox.enabled,
            docker_status: data.docker_status,
            claude_code_enabled: config.claude_code.enabled,
            routines_enabled: config.routines.enabled,
            skills_enabled: config.skills.enabled,
            channels: data.channel_names.clone(),
            tunnel_url: data
                .active_tunnel
                .as_ref()
                .and_then(|t| t.public_url())
                .or_else(|| config.tunnel.public_url.clone()),
            tunnel_provider: data.active_tunnel.as_ref().map(|t| t.name().to_string()),
        }
    }
}

struct Palette<'a> {
    cyan: &'a str,
    dim: &'a str,
    yellow: &'a str,
    yellow_underline: &'a str,
    reset: &'a str,
}

fn render_model_line(info: &BootInfo, p: &Palette<'_>) -> String {
    let model_display = if let Some(ref cheap) = info.cheap_model {
        format!(
            "{cyan}{llm}{reset}  {dim}cheap{reset} {cyan}{cheap}{reset}",
            cyan = p.cyan,
            dim = p.dim,
            reset = p.reset,
            llm = info.llm_model,
        )
    } else {
        format!("{}{}{}", p.cyan, info.llm_model, p.reset)
    };
    format!(
        "  {dim}model{reset}     {model_display}  {dim}via {backend}{reset}\n",
        dim = p.dim,
        reset = p.reset,
        backend = info.llm_backend,
    )
}

fn format_docker_feature(status: &DockerStatus, yellow: &str, reset: &str) -> Option<String> {
    match status {
        DockerStatus::Available => Some("sandbox".to_string()),
        DockerStatus::NotInstalled => {
            Some(format!("{yellow}sandbox (docker not installed){reset}"))
        }
        DockerStatus::NotRunning => Some(format!("{yellow}sandbox (docker not running){reset}")),
        DockerStatus::Disabled => None,
    }
}

fn collect_features(info: &BootInfo, yellow: &str, reset: &str) -> Vec<String> {
    let mut features = Vec::new();
    if info.embeddings_enabled {
        if let Some(ref provider) = info.embeddings_provider {
            features.push(format!("embeddings ({provider})"));
        } else {
            features.push("embeddings".to_string());
        }
    }
    if info.heartbeat_enabled {
        let mins = info.heartbeat_interval_secs / 60;
        features.push(format!("heartbeat ({mins}m)"));
    }
    if let Some(label) = format_docker_feature(&info.docker_status, yellow, reset) {
        features.push(label);
    }
    if info.claude_code_enabled {
        features.push("claude-code".to_string());
    }
    if info.routines_enabled {
        features.push("routines".to_string());
    }
    if info.skills_enabled {
        features.push("skills".to_string());
    }
    features
}

fn render_footer_urls(info: &BootInfo, p: &Palette<'_>) -> String {
    let mut out = String::new();
    if let Some(ref url) = info.gateway_url {
        out.push('\n');
        out.push_str(&format!(
            "  {dim}gateway{reset}   {yu}{url}{reset}\n",
            dim = p.dim,
            reset = p.reset,
            yu = p.yellow_underline,
        ));
    }
    if let Some(ref url) = info.tunnel_url {
        let provider_tag = info
            .tunnel_provider
            .as_deref()
            .map(|name| format!(" {dim}({name}){reset}", dim = p.dim, reset = p.reset))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {dim}tunnel{reset}    {yu}{url}{reset}{provider_tag}\n",
            dim = p.dim,
            reset = p.reset,
            yu = p.yellow_underline,
        ));
    }
    out
}

/// Render the boot screen to a string.
pub fn render_boot_screen(info: &BootInfo) -> String {
    let bold = "\x1b[1m";
    let cyan = "\x1b[36m";
    let dim = "\x1b[90m";
    let yellow = "\x1b[33m";
    let yellow_underline = "\x1b[33;4m";
    let reset = "\x1b[0m";

    let palette = Palette {
        cyan,
        dim,
        yellow,
        yellow_underline,
        reset,
    };
    let border = format!("  {dim}{}{reset}", "\u{2576}".repeat(58));
    let db_status = if info.db_connected {
        "connected"
    } else {
        "none"
    };
    let features = collect_features(info, palette.yellow, palette.reset);

    let mut output = String::new();
    output.push('\n');
    output.push_str(&border);
    output.push('\n');
    output.push('\n');
    output.push_str(&format!(
        "  {bold}{}{reset} v{}\n",
        info.agent_name, info.version
    ));
    output.push('\n');
    output.push_str(&render_model_line(info, &palette));
    output.push_str(&format!(
        "  {dim}database{reset}  {cyan}{}{reset} {dim}({db_status}){reset}\n",
        info.db_backend
    ));
    output.push_str(&format!(
        "  {dim}tools{reset}     {cyan}{}{reset} {dim}registered{reset}\n",
        info.tool_count
    ));
    if !features.is_empty() {
        output.push_str(&format!(
            "  {dim}features{reset}  {cyan}{}{reset}\n",
            features.join("  ")
        ));
    }
    if !info.channels.is_empty() {
        output.push_str(&format!(
            "  {dim}channels{reset}  {cyan}{}{reset}\n",
            info.channels.join("  ")
        ));
    }
    output.push_str(&render_footer_urls(info, &palette));
    output.push('\n');
    output.push_str(&border);
    output.push('\n');
    output.push('\n');
    output.push_str("  /help for commands, /quit to exit\n");
    output.push('\n');
    output
}

/// Print the boot screen to stdout.
pub fn print_boot_screen(info: &BootInfo) {
    print!("{}", render_boot_screen(info));
}

#[cfg(test)]
mod tests;
