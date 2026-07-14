//! Lifecycle management for extensions: discovery, installation, authentication,
//! and activation of channels, tools, and MCP servers.
//!
//! Extensions are the user-facing abstraction that unifies three runtime kinds:
//! - **Channels** (Telegram, Slack, Discord) — messaging integrations (WASM)
//! - **Tools** — sandboxed capabilities (WASM)
//! - **MCP servers** — external API integrations via Model Context Protocol
//!
//! The agent can search a built-in registry (or discover online), install,
//! authenticate, and activate extensions at runtime without CLI commands.
//!
//! ```text
//!  User: "add telegram"
//!    -> tool_search("telegram")    -> finds channel in registry
//!    -> tool_install("telegram")   -> copies bundled WASM to channels dir
//!    -> tool_activate("telegram")  -> configures credentials, starts channel
//! ```

pub mod activation;
mod auth;
pub mod builder;
mod descriptor;
pub mod discovery;
pub mod manager;
pub mod registry;
mod results;
#[cfg(test)]
mod tests;

pub use activation::{
    LiveMcpActivation, LiveMcpActivationConfig, LiveWasmChannelActivation,
    LiveWasmChannelActivationConfig, LiveWasmToolActivation, LiveWasmToolActivationConfig,
    McpActivationPort, McpClientsMap, NoOpMcpActivation, NoOpWasmChannelActivation,
    NoOpWasmToolActivation, WasmChannelActivationPort, WasmToolActivationPort,
};
pub use auth::{AuthResult, AuthStatus, ToolAuthState};
pub use builder::{
    BuildExtensionManagerParams, BuildExtensionsParams, build_extension_manager, build_extensions,
};
pub use descriptor::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry, ResultSource};
pub use discovery::{DiscoveryFuture, DiscoveryPort, NoOpDiscovery, OnlineDiscovery};
pub use manager::{ExtensionManager, ExtensionManagerConfig, LiveWasmChannelSharedState};
pub use registry::ExtensionRegistry;
pub use results::{
    ActivateResult, ExtensionError, InstallResult, InstalledExtension, SearchResult,
    UpgradeOutcome, UpgradeResult,
};
