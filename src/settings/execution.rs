//! Settings sections for agent behaviour, sandboxes, safety, builder,
//! and transcription.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::default_true;

/// Agent behaviour configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    /// Agent name.
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Maximum parallel jobs.
    #[serde(default = "default_max_parallel_jobs")]
    pub max_parallel_jobs: u32,

    /// Job timeout in seconds.
    #[serde(default = "default_job_timeout")]
    pub job_timeout_secs: u64,

    /// Stuck job threshold in seconds.
    #[serde(default = "default_stuck_threshold")]
    pub stuck_threshold_secs: u64,

    /// Whether to use planning before tool execution.
    #[serde(default = "default_true")]
    pub use_planning: bool,

    /// Self-repair check interval in seconds.
    #[serde(default = "default_repair_interval")]
    pub repair_check_interval_secs: u64,

    /// Maximum repair attempts.
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: u32,

    /// Session idle timeout in seconds (default: 7 days). Sessions inactive
    /// longer than this are pruned from memory.
    #[serde(default = "default_session_idle_timeout")]
    pub session_idle_timeout_secs: u64,

    /// Maximum tool-call iterations per agentic loop invocation (default: 50).
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,

    /// When true, skip tool approval checks entirely. For benchmarks/CI.
    #[serde(default)]
    pub auto_approve_tools: bool,

    /// Default timezone for new sessions (IANA name, e.g. "America/New_York").
    #[serde(default = "default_timezone")]
    pub default_timezone: String,

    /// Maximum tokens per job (0 = unlimited).
    #[serde(default)]
    pub max_tokens_per_job: u64,
}

fn default_agent_name() -> String {
    "axinite".to_string()
}

fn default_max_parallel_jobs() -> u32 {
    5
}

fn default_job_timeout() -> u64 {
    3600 // 1 hour
}

fn default_stuck_threshold() -> u64 {
    300 // 5 minutes
}

fn default_repair_interval() -> u64 {
    60 // 1 minute
}

fn default_session_idle_timeout() -> u64 {
    7 * 24 * 3600 // 7 days
}

fn default_max_repair_attempts() -> u32 {
    3
}

fn default_max_tool_iterations() -> usize {
    50
}

fn default_timezone() -> String {
    "UTC".to_string()
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            name: default_agent_name(),
            max_parallel_jobs: default_max_parallel_jobs(),
            job_timeout_secs: default_job_timeout(),
            stuck_threshold_secs: default_stuck_threshold(),
            use_planning: true,
            repair_check_interval_secs: default_repair_interval(),
            max_repair_attempts: default_max_repair_attempts(),
            session_idle_timeout_secs: default_session_idle_timeout(),
            max_tool_iterations: default_max_tool_iterations(),
            auto_approve_tools: false,
            default_timezone: default_timezone(),
            max_tokens_per_job: 0,
        }
    }
}

/// WASM sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSettings {
    /// Whether WASM tool execution is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Directory containing installed WASM tools.
    #[serde(default)]
    pub tools_dir: Option<PathBuf>,

    /// Default memory limit in bytes.
    #[serde(default = "default_wasm_memory_limit")]
    pub default_memory_limit: u64,

    /// Default execution timeout in seconds.
    #[serde(default = "default_wasm_timeout")]
    pub default_timeout_secs: u64,

    /// Default fuel limit for CPU metering.
    #[serde(default = "default_wasm_fuel_limit")]
    pub default_fuel_limit: u64,

    /// Whether to cache compiled modules.
    #[serde(default = "default_true")]
    pub cache_compiled: bool,

    /// Directory for compiled module cache.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

fn default_wasm_memory_limit() -> u64 {
    10 * 1024 * 1024 // 10 MB
}

fn default_wasm_timeout() -> u64 {
    60
}

fn default_wasm_fuel_limit() -> u64 {
    10_000_000
}

impl Default for WasmSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            tools_dir: None,
            default_memory_limit: default_wasm_memory_limit(),
            default_timeout_secs: default_wasm_timeout(),
            default_fuel_limit: default_wasm_fuel_limit(),
            cache_compiled: true,
            cache_dir: None,
        }
    }
}

/// Docker sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Whether the Docker sandbox is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Sandbox policy: "readonly", "workspace_write", or "full_access".
    #[serde(default = "default_sandbox_policy")]
    pub policy: String,

    /// Command timeout in seconds.
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_secs: u64,

    /// Memory limit in megabytes.
    #[serde(default = "default_sandbox_memory")]
    pub memory_limit_mb: u64,

    /// CPU shares (relative weight).
    #[serde(default = "default_sandbox_cpu_shares")]
    pub cpu_shares: u32,

    /// Docker image for the sandbox.
    #[serde(default = "default_sandbox_image")]
    pub image: String,

    /// Whether to auto-pull the image if not found.
    #[serde(default = "default_true")]
    pub auto_pull_image: bool,

    /// Additional domains to allow through the network proxy.
    #[serde(default)]
    pub extra_allowed_domains: Vec<String>,

    /// Whether Claude Code sandbox mode is enabled.
    #[serde(default)]
    pub claude_code_enabled: bool,
}

fn default_sandbox_policy() -> String {
    "readonly".to_string()
}

fn default_sandbox_timeout() -> u64 {
    120
}

fn default_sandbox_memory() -> u64 {
    2048
}

fn default_sandbox_cpu_shares() -> u32 {
    1024
}

fn default_sandbox_image() -> String {
    "axinite-worker:latest".to_string()
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            policy: default_sandbox_policy(),
            timeout_secs: default_sandbox_timeout(),
            memory_limit_mb: default_sandbox_memory(),
            cpu_shares: default_sandbox_cpu_shares(),
            image: default_sandbox_image(),
            auto_pull_image: true,
            extra_allowed_domains: Vec::new(),
            claude_code_enabled: false,
        }
    }
}

/// Safety configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySettings {
    /// Maximum output length in bytes.
    #[serde(default = "default_max_output_length")]
    pub max_output_length: usize,

    /// Whether injection check is enabled.
    #[serde(default = "default_true")]
    pub injection_check_enabled: bool,
}

fn default_max_output_length() -> usize {
    100_000
}

impl Default for SafetySettings {
    fn default() -> Self {
        Self {
            max_output_length: default_max_output_length(),
            injection_check_enabled: true,
        }
    }
}

/// Builder configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderSettings {
    /// Whether the software builder tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Directory for build artefacts.
    #[serde(default)]
    pub build_dir: Option<PathBuf>,

    /// Maximum iterations for the build loop.
    #[serde(default = "default_builder_max_iterations")]
    pub max_iterations: u32,

    /// Build timeout in seconds.
    #[serde(default = "default_builder_timeout")]
    pub timeout_secs: u64,

    /// Whether to automatically register built WASM tools.
    #[serde(default = "default_true")]
    pub auto_register: bool,
}

fn default_builder_max_iterations() -> u32 {
    20
}

fn default_builder_timeout() -> u64 {
    600
}

impl Default for BuilderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            build_dir: None,
            max_iterations: default_builder_max_iterations(),
            timeout_secs: default_builder_timeout(),
            auto_register: true,
        }
    }
}

/// Transcription pipeline settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSettings {
    /// Whether audio transcription is enabled.
    #[serde(default)]
    pub enabled: bool,
}
