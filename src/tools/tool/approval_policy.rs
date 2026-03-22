/// How much approval a specific tool invocation requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalRequirement {
    /// No approval needed.
    Never,
    /// Needs approval, but session auto-approve can bypass.
    UnlessAutoApproved,
    /// Always needs explicit approval (even if auto-approved).
    Always,
}

impl ApprovalRequirement {
    /// Whether this invocation requires approval in contexts where
    /// auto-approve is irrelevant (e.g. autonomous worker/scheduler).
    pub fn is_required(&self) -> bool {
        !matches!(self, Self::Never)
    }
}

/// Approval context for autonomous tool execution (routines, background jobs).
///
/// Interactive sessions don't use this type — they rely on session-level
/// auto-approve lists managed by the UI. This enum models only the autonomous
/// case where no interactive user is present.
#[derive(Debug, Clone)]
pub enum ApprovalContext {
    /// Autonomous job with no interactive user. `UnlessAutoApproved` tools are
    /// pre-approved. `Always` tools are blocked unless listed in `allowed_tools`.
    Autonomous {
        /// Tool names that are pre-authorized even for `Always` approval.
        allowed_tools: std::collections::HashSet<String>,
    },
}

impl ApprovalContext {
    /// Create an autonomous context with no extra tool permissions.
    pub fn autonomous() -> Self {
        Self::Autonomous {
            allowed_tools: std::collections::HashSet::new(),
        }
    }

    /// Create an autonomous context with specific tools pre-authorized.
    pub fn autonomous_with_tools(tools: impl IntoIterator<Item = String>) -> Self {
        Self::Autonomous {
            allowed_tools: tools.into_iter().collect(),
        }
    }

    /// Check whether a tool invocation is blocked in this context.
    pub fn is_blocked(&self, tool_name: &str, requirement: ApprovalRequirement) -> bool {
        match self {
            Self::Autonomous { allowed_tools } => match requirement {
                ApprovalRequirement::Never => false,
                ApprovalRequirement::UnlessAutoApproved => false,
                ApprovalRequirement::Always => !allowed_tools.contains(tool_name),
            },
        }
    }

    /// Check whether a tool is blocked given an optional context.
    ///
    /// When `None`, falls back to legacy behavior: all non-`Never` tools are blocked.
    pub fn is_blocked_or_default(
        context: &Option<Self>,
        tool_name: &str,
        requirement: ApprovalRequirement,
    ) -> bool {
        match context {
            Some(ctx) => ctx.is_blocked(tool_name, requirement),
            None => requirement.is_required(),
        }
    }
}

/// Per-tool rate limit configuration for built-in tool invocations.
///
/// Controls how many times a tool can be invoked per user, per time window.
/// Read-only tools (echo, time, json, file_read, etc.) should NOT be rate limited.
/// Write/external tools (shell, http, file_write, memory_write, create_job) should be.
#[derive(Debug, Clone)]
pub struct ToolRateLimitConfig {
    /// Maximum invocations per minute.
    pub requests_per_minute: u32,
    /// Maximum invocations per hour.
    pub requests_per_hour: u32,
}

impl ToolRateLimitConfig {
    /// Create a config with explicit limits.
    pub fn new(requests_per_minute: u32, requests_per_hour: u32) -> Self {
        Self {
            requests_per_minute,
            requests_per_hour,
        }
    }
}

impl Default for ToolRateLimitConfig {
    /// Default: 60 requests/minute, 1000 requests/hour (generous for WASM HTTP).
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            requests_per_hour: 1000,
        }
    }
}

/// Where a tool should execute: orchestrator process or inside a container.
///
/// Orchestrator tools run in the main agent process (memory access, job mgmt, etc).
/// Container tools run inside Docker containers (shell, file ops, code mods).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolDomain {
    /// Safe to run in the orchestrator (pure functions, memory, job management).
    Orchestrator,
    /// Must run inside a sandboxed container (filesystem, shell, code).
    Container,
}

/// Hosted-worker catalog eligibility for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedToolEligibility {
    /// The tool may be advertised to hosted workers.
    Eligible,
    /// The tool requires approval semantics hosted workers cannot satisfy.
    ApprovalGated,
}

/// Source family a tool belongs to for hosted catalogue projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostedToolCatalogSource {
    /// Tool supplied by a live MCP server wrapper.
    Mcp,
    /// Tool supplied by an orchestrator-owned WASM wrapper.
    Wasm,
}
