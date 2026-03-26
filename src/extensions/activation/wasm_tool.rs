//! WASM tool activation port.
//!
//! Isolates WASM tool loading, capability parsing, and tool-registry
//! registration behind a trait boundary.

use super::ActivationFuture;
use crate::extensions::{ActivateResult, ExtensionError};

/// Object-safe port for activating WASM tool extensions (dyn-facing).
///
/// Concrete implementations should implement [`NativeWasmToolActivationPort`]
/// instead; the blanket adapter boxes futures automatically.
pub trait WasmToolActivationPort: Send + Sync {
    /// Load the named WASM tool from disk, register it, and return
    /// the activation result.
    fn activate_wasm_tool<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}

/// Native async sibling for WASM tool activation.
pub trait NativeWasmToolActivationPort: Send + Sync {
    /// Load the named WASM tool and register it.
    fn activate_wasm_tool<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Future<Output = Result<ActivateResult, ExtensionError>> + Send + 'a;
}

use std::future::Future;

impl<T> WasmToolActivationPort for T
where
    T: NativeWasmToolActivationPort + Send + Sync,
{
    fn activate_wasm_tool<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(NativeWasmToolActivationPort::activate_wasm_tool(self, name))
    }
}
