//! Shared Telegram channel test helpers for integration tests.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use ironclaw::channels::wasm::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};

/// Resolve the Telegram channel WASM artifact used by integration tests.
pub fn telegram_wasm_path() -> Result<PathBuf, String> {
    let channel_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("channels-src/telegram");

    // `build.rs` writes a flat component artifact for the host to load. Prefer
    // that output, then fall back to the raw build artifact across shared or
    // per-crate target directories.
    let bundled_component = channel_dir.join("telegram.wasm");
    if bundled_component.exists() {
        return Ok(bundled_component);
    }

    ironclaw::registry::artifacts::find_wasm_artifact(&channel_dir, "telegram_channel", "release")
        .ok_or_else(|| {
            let expected = ironclaw::registry::artifacts::resolve_target_dir(&channel_dir)
                .join("wasm32-wasip2/release/telegram_channel.wasm");
            format!(
                "Telegram WASM module not found. Checked {} and {}",
                bundled_component.display(),
                expected.display()
            )
        })
}

/// Create a test runtime for WASM channel operations.
pub fn create_test_runtime() -> Arc<WasmChannelRuntime> {
    let config = WasmChannelRuntimeConfig::for_testing();
    Arc::new(WasmChannelRuntime::new(config).expect("Failed to create runtime"))
}

/// Load the real Telegram WASM module.
pub async fn load_telegram_module(
    runtime: &Arc<WasmChannelRuntime>,
) -> Result<Arc<PreparedChannelModule>, Box<dyn Error>> {
    let path = telegram_wasm_path().map_err(std::io::Error::other)?;
    let wasm_bytes = std::fs::read(&path)
        .map_err(|e| format!("Failed to read WASM module at {}: {}", path.display(), e))?;

    let module = runtime
        .prepare(
            "telegram",
            &wasm_bytes,
            None,
            Some("Telegram Bot API channel".to_string()),
        )
        .await?;

    Ok(module)
}
