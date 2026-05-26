//! Unit and integration tests for the WASM channel wrapper.

mod approval;
mod attachments;
mod channel;
mod clone;
mod convert;
mod dispatch;
mod store;

use std::sync::Arc;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::WasmChannel;
use crate::pairing::PairingStore;
use crate::tools::wasm::ResourceLimits;

pub(super) fn create_test_channel() -> WasmChannel {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "test".to_string(),
        description: "Test channel".to_string(),
        component: None,
        limits: ResourceLimits::default(),
    });

    let capabilities = ChannelCapabilities::for_channel("test").with_path("/webhook/test");

    WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    )
}
