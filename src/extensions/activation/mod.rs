//! Activation ports for decoupling extension lifecycle policy from runtime
//! mechanisms.
//!
//! Each port isolates one activation path behind a minimal object-safe trait
//! interface returning boxed futures. The [`ExtensionManager`](super::ExtensionManager)
//! dispatches to these ports without depending on concrete runtimes, making each
//! activation path independently testable.
//!
//! ```text
//! ┌──────────────────────┐     ┌───────────────────────┐
//! │  ExtensionManager    │────>│  McpActivationPort    │
//! │  (policy/catalogue)  │────>│  WasmToolActivation…  │
//! │                      │────>│  WasmChannelActivat…  │
//! │                      │────>│  ChannelRelayActivat… │
//! └──────────────────────┘     └───────────────────────┘
//! ```

use std::future::Future;
use std::pin::Pin;

use super::{ActivateResult, ExtensionError};

mod live_mcp;
mod live_wasm_channel;
mod live_wasm_tool;
mod mcp;
mod noop;
mod wasm_channel;
mod wasm_tool;

pub use live_mcp::{LiveMcpActivation, LiveMcpActivationConfig, McpClientCell, McpClientsMap};
pub use live_wasm_channel::{LiveWasmChannelActivation, LiveWasmChannelActivationConfig};
pub use live_wasm_tool::{LiveWasmToolActivation, LiveWasmToolActivationConfig};
pub use mcp::McpActivationPort;
pub use noop::{NoOpMcpActivation, NoOpWasmChannelActivation, NoOpWasmToolActivation};
pub use wasm_channel::WasmChannelActivationPort;
pub use wasm_tool::WasmToolActivationPort;

pub(crate) use live_wasm_channel::ChannelRuntimeState;

/// Boxed future alias for dyn-safe activation methods.
pub type ActivationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ActivateResult, ExtensionError>> + Send + 'a>>;
