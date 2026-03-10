pub mod assertions;
pub mod cleanup;
pub mod instrumented_llm;
pub mod metrics;
pub mod test_channel;
pub mod test_rig;
pub mod trace_llm;

use std::sync::Arc;
use std::time::Duration;

use ironclaw::tools::wasm::{ResourceLimits, WasmRuntimeConfig, WasmToolRuntime};

#[allow(dead_code)]
pub(crate) fn metadata_test_runtime() -> Arc<WasmToolRuntime> {
    let config = WasmRuntimeConfig {
        default_limits: ResourceLimits::default()
            .with_memory(8 * 1024 * 1024)
            .with_fuel(100_000)
            .with_timeout(Duration::from_secs(5)),
        ..WasmRuntimeConfig::for_testing()
    };
    Arc::new(WasmToolRuntime::new(config).expect("create wasm runtime"))
}
