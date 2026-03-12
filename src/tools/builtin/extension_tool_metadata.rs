//! Metadata definitions for extension-management tools and their approval rules.

use crate::tools::tool::ApprovalRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtensionToolKind {
    Search,
    Install,
    Auth,
    Activate,
    List,
    Remove,
    Upgrade,
    Info,
}

impl ExtensionToolKind {
    pub const ALL: [Self; 8] = [
        Self::Search,
        Self::Install,
        Self::Auth,
        Self::Activate,
        Self::List,
        Self::Remove,
        Self::Upgrade,
        Self::Info,
    ];

    pub const HOSTED_WORKER_PROXY_SAFE: [Self; 3] = [Self::Search, Self::List, Self::Info];

    pub fn name(self) -> &'static str {
        match self {
            Self::Search => "tool_search",
            Self::Install => "tool_install",
            Self::Auth => "tool_auth",
            Self::Activate => "tool_activate",
            Self::List => "tool_list",
            Self::Remove => "tool_remove",
            Self::Upgrade => "tool_upgrade",
            Self::Info => "extension_info",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Search => {
                "Search for available extensions to add new capabilities. Extensions include \
                 channels (Telegram, Slack, Discord — for messaging), tools, and MCP servers. \
                 Use discover:true to search online if the built-in registry has no results."
            }
            Self::Install => {
                "Install an extension (channel, tool, or MCP server). \
                 Use the name from tool_search results, or provide an explicit URL."
            }
            Self::Auth => {
                "Initiate authentication for an extension. For OAuth, returns a URL. \
                 For manual auth, returns instructions. The user provides their token \
                 through a secure channel, never through this tool."
            }
            Self::Activate => {
                "Activate an installed extension — starts channels, loads tools, or connects to MCP servers."
            }
            Self::List => {
                "List extensions with their authentication and activation status. \
                 Set include_available:true to also show registry entries not yet installed."
            }
            Self::Remove => {
                "Permanently remove an installed extension (channel, tool, or MCP server) from disk. \
                 This action cannot be undone — the WASM binary and configuration files will be deleted."
            }
            Self::Upgrade => {
                "Upgrade installed WASM extensions (channels and tools) to match the current \
                 host WIT version. If name is omitted, checks and upgrades all installed WASM \
                 extensions. Authentication and secrets are preserved."
            }
            Self::Info => {
                "Show detailed information about an installed extension, including version \
                 and WIT version compatibility."
            }
        }
    }

    pub fn parameters_schema(self) -> serde_json::Value {
        match self {
            Self::Search => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (name, keyword, or description fragment)"
                    },
                    "discover": {
                        "type": "boolean",
                        "description": "If true, also search online (slower, 5-15s). Try without first.",
                        "default": false
                    }
                },
                "required": ["query"]
            }),
            Self::Install => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Extension name (from search results or custom)"
                    },
                    "url": {
                        "type": "string",
                        "description": "Explicit URL (for extensions not in the registry)"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["mcp_server", "wasm_tool", "wasm_channel"],
                        "description": "Extension type (auto-detected if omitted)"
                    }
                },
                "required": ["name"]
            }),
            Self::Auth | Self::Activate | Self::Remove | Self::Info => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": match self {
                            Self::Auth => "Extension name to authenticate",
                            Self::Activate => "Extension name to activate",
                            Self::Remove => "Extension name to remove",
                            Self::Info => "Extension name to get info about",
                            _ => unreachable!(),
                        }
                    }
                },
                "required": ["name"]
            }),
            Self::List => serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["mcp_server", "wasm_tool", "wasm_channel"],
                        "description": "Filter by extension type (omit to list all)"
                    },
                    "include_available": {
                        "type": "boolean",
                        "description": "If true, also include registry entries that are not yet installed",
                        "default": false
                    }
                }
            }),
            Self::Upgrade => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Extension name to upgrade (omit to upgrade all)"
                    }
                }
            }),
        }
    }

    pub fn approval_requirement(self) -> ApprovalRequirement {
        match self {
            Self::Search | Self::List | Self::Info => ApprovalRequirement::Never,
            Self::Activate | Self::Install | Self::Auth | Self::Upgrade => {
                ApprovalRequirement::UnlessAutoApproved
            }
            Self::Remove => ApprovalRequirement::Always,
        }
    }
}
