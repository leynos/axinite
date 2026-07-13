//! Curated built-in extension entries that ship with ironclaw.

use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

/// Well-known extensions that ship with ironclaw.
///
/// If `relay_url` is provided, a channel-relay Slack entry is included in the list.
/// Pass `None` when the relay is not configured.
pub fn builtin_entries() -> Vec<RegistryEntry> {
    builtin_entries_with_relay(std::env::var("CHANNEL_RELAY_URL").ok())
}

/// Well-known extensions, with an optional relay URL for the channel-relay entry.
pub fn builtin_entries_with_relay(relay_url: Option<String>) -> Vec<RegistryEntry> {
    let mut entries = vec![
        // -- MCP Servers --
        RegistryEntry {
            name: "notion".to_string(),
            display_name: "Notion".to_string(),
            kind: ExtensionKind::McpServer,
            description: "Connect to Notion for reading and writing pages, databases, and comments"
                .to_string(),
            keywords: vec![
                "notes".into(),
                "wiki".into(),
                "docs".into(),
                "pages".into(),
                "database".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.notion.com/mcp".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "linear".to_string(),
            display_name: "Linear".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to Linear for issue tracking, project management, and team workflows"
                    .to_string(),
            keywords: vec![
                "issues".into(),
                "tickets".into(),
                "project".into(),
                "tracking".into(),
                "bugs".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.linear.app/sse".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "github".to_string(),
            display_name: "GitHub".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to GitHub for repository management, issues, PRs, and code search"
                    .to_string(),
            keywords: vec![
                "git".into(),
                "repos".into(),
                "code".into(),
                "pull-request".into(),
                "issues".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://api.githubcopilot.com/mcp/".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "slack-mcp".to_string(),
            display_name: "Slack MCP".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to Slack via MCP for messaging, channel management, and team communication"
                    .to_string(),
            keywords: vec![
                "messaging".into(),
                "chat".into(),
                "channels".into(),
                "team".into(),
                "communication".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.slack.com".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "sentry".to_string(),
            display_name: "Sentry".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to Sentry for error tracking, performance monitoring, and debugging"
                    .to_string(),
            keywords: vec![
                "errors".into(),
                "monitoring".into(),
                "debugging".into(),
                "crashes".into(),
                "performance".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.sentry.dev/mcp".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "stripe".to_string(),
            display_name: "Stripe".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to Stripe for payment processing, subscriptions, and financial data"
                    .to_string(),
            keywords: vec![
                "payments".into(),
                "billing".into(),
                "subscriptions".into(),
                "invoices".into(),
                "finance".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.stripe.com".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "cloudflare".to_string(),
            display_name: "Cloudflare".to_string(),
            kind: ExtensionKind::McpServer,
            description:
                "Connect to Cloudflare for DNS, Workers, KV, and infrastructure management"
                    .to_string(),
            keywords: vec![
                "cdn".into(),
                "dns".into(),
                "workers".into(),
                "hosting".into(),
                "infrastructure".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.cloudflare.com/mcp".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "asana".to_string(),
            display_name: "Asana".to_string(),
            kind: ExtensionKind::McpServer,
            description: "Connect to Asana for task management, projects, and team coordination"
                .to_string(),
            keywords: vec![
                "tasks".into(),
                "projects".into(),
                "management".into(),
                "team".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.asana.com/v2/mcp".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        RegistryEntry {
            name: "intercom".to_string(),
            display_name: "Intercom".to_string(),
            kind: ExtensionKind::McpServer,
            description: "Connect to Intercom for customer messaging, support, and engagement"
                .to_string(),
            keywords: vec![
                "support".into(),
                "customers".into(),
                "messaging".into(),
                "chat".into(),
                "helpdesk".into(),
            ],
            source: ExtensionSource::McpUrl {
                url: "https://mcp.intercom.com/mcp".to_string(),
            },
            fallback_source: None,
            auth_hint: AuthHint::Dcr,
            version: None,
        },
        // WASM channels (telegram, slack, discord, whatsapp) come from the embedded
        // registry catalog (registry/channels/*.json) with WasmDownload URLs pointing
        // to GitHub release artifacts. See new_with_catalog() for merging.
    ];

    // Conditionally add channel-relay entries when relay URL is configured
    if let Some(relay_url) = relay_url {
        entries.push(RegistryEntry {
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
        });
    }

    entries
}
