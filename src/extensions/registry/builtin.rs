//! Curated built-in extension entries that ship with ironclaw.

use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

/// Compact spec for a curated MCP server entry (DCR auth, no fallback).
struct McpEntrySpec {
    name: &'static str,
    display_name: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    url: &'static str,
}

impl McpEntrySpec {
    /// Expand the spec into a full registry entry.
    fn to_entry(&self) -> RegistryEntry {
        RegistryEntry {
            name: self.name.to_string(),
            display_name: self.display_name.to_string(),
            kind: ExtensionKind::McpServer,
            description: self.description.to_string(),
            keywords: self.keywords.iter().map(|k| (*k).to_string()).collect(),
            source: ExtensionSource::McpUrl {
                url: self.url.to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        }
    }
}

/// Curated MCP servers that ship with ironclaw.
///
/// WASM channels (telegram, slack, discord, whatsapp) come from the embedded
/// registry catalog (registry/channels/*.json) with WasmDownload URLs pointing
/// to GitHub release artifacts. See new_with_catalog() for merging.
const MCP_SERVERS: &[McpEntrySpec] = &[
    McpEntrySpec {
        name: "notion",
        display_name: "Notion",
        description: "Connect to Notion for reading and writing pages, databases, and comments",
        keywords: &["notes", "wiki", "docs", "pages", "database"],
        url: "https://mcp.notion.com/mcp",
    },
    McpEntrySpec {
        name: "linear",
        display_name: "Linear",
        description: "Connect to Linear for issue tracking, project management, and team workflows",
        keywords: &["issues", "tickets", "project", "tracking", "bugs"],
        url: "https://mcp.linear.app/sse",
    },
    McpEntrySpec {
        name: "github",
        display_name: "GitHub",
        description: "Connect to GitHub for repository management, issues, PRs, and code search",
        keywords: &["git", "repos", "code", "pull-request", "issues"],
        url: "https://api.githubcopilot.com/mcp/",
    },
    McpEntrySpec {
        name: "slack-mcp",
        display_name: "Slack MCP",
        description: "Connect to Slack via MCP for messaging, channel management, and team communication",
        keywords: &["messaging", "chat", "channels", "team", "communication"],
        url: "https://mcp.slack.com",
    },
    McpEntrySpec {
        name: "sentry",
        display_name: "Sentry",
        description: "Connect to Sentry for error tracking, performance monitoring, and debugging",
        keywords: &[
            "errors",
            "monitoring",
            "debugging",
            "crashes",
            "performance",
        ],
        url: "https://mcp.sentry.dev/mcp",
    },
    McpEntrySpec {
        name: "stripe",
        display_name: "Stripe",
        description: "Connect to Stripe for payment processing, subscriptions, and financial data",
        keywords: &[
            "payments",
            "billing",
            "subscriptions",
            "invoices",
            "finance",
        ],
        url: "https://mcp.stripe.com",
    },
    McpEntrySpec {
        name: "cloudflare",
        display_name: "Cloudflare",
        description: "Connect to Cloudflare for DNS, Workers, KV, and infrastructure management",
        keywords: &["cdn", "dns", "workers", "hosting", "infrastructure"],
        url: "https://mcp.cloudflare.com/mcp",
    },
    McpEntrySpec {
        name: "asana",
        display_name: "Asana",
        description: "Connect to Asana for task management, projects, and team coordination",
        keywords: &["tasks", "projects", "management", "team"],
        url: "https://mcp.asana.com/v2/mcp",
    },
    McpEntrySpec {
        name: "intercom",
        display_name: "Intercom",
        description: "Connect to Intercom for customer messaging, support, and engagement",
        keywords: &["support", "customers", "messaging", "chat", "helpdesk"],
        url: "https://mcp.intercom.com/mcp",
    },
];

/// The channel-relay Slack entry, available when a relay URL is configured.
fn slack_relay_entry(relay_url: String) -> RegistryEntry {
    RegistryEntry {
        name: crate::channels::relay::DEFAULT_RELAY_NAME.to_string(),
        display_name: "Slack".to_string(),
        kind: ExtensionKind::ChannelRelay,
        description: "Connect Slack workspace via channel relay".to_string(),
        keywords: vec![
            "slack".into(),
            "chat".into(),
            "messaging".into(),
            "relay".into(),
        ],
        source: ExtensionSource::ChannelRelay { relay_url },
        fallback_source: None,
        auth_hint: AuthHint::ChannelRelayOAuth,
        version: None,
    }
}

/// Well-known extensions that ship with ironclaw.
///
/// If `relay_url` is provided, a channel-relay Slack entry is included in the list.
/// Pass `None` when the relay is not configured.
pub fn builtin_entries() -> Vec<RegistryEntry> {
    builtin_entries_with_relay(std::env::var("CHANNEL_RELAY_URL").ok())
}

/// Well-known extensions, with an optional relay URL for the channel-relay entry.
pub fn builtin_entries_with_relay(relay_url: Option<String>) -> Vec<RegistryEntry> {
    let mut entries: Vec<RegistryEntry> = MCP_SERVERS.iter().map(McpEntrySpec::to_entry).collect();

    // Conditionally add channel-relay entries when relay URL is configured
    if let Some(relay_url) = relay_url {
        entries.push(slack_relay_entry(relay_url));
    }

    entries
}
