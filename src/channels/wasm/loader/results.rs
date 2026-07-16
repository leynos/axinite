//! Result types for WASM channel loading.
//!
//! `LoadedChannel` pairs a loaded channel with its parsed capabilities file;
//! `LoadResults` aggregates successes and per-path errors from a directory scan.

use std::path::PathBuf;

use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::schema::ChannelCapabilitiesFile;
use crate::channels::wasm::wrapper::WasmChannel;

/// A loaded WASM channel with its capabilities file.
pub struct LoadedChannel {
    /// The loaded channel.
    pub channel: WasmChannel,

    /// The parsed capabilities file (if present).
    pub capabilities_file: Option<ChannelCapabilitiesFile>,
}

impl LoadedChannel {
    /// Get the channel name.
    pub fn name(&self) -> &str {
        self.channel.channel_name()
    }

    /// Get the webhook secret header name from capabilities.
    pub fn webhook_secret_header(&self) -> Option<&str> {
        self.capabilities_file
            .as_ref()
            .and_then(|f| f.webhook_secret_header())
    }

    /// Get the signature verification key secret name from capabilities.
    pub fn signature_key_secret_name(&self) -> Option<String> {
        self.capabilities_file
            .as_ref()
            .and_then(|f| f.signature_key_secret_name().map(|s| s.to_string()))
    }

    /// Get the HMAC-SHA256 signing secret name from capabilities.
    pub fn hmac_secret_name(&self) -> Option<String> {
        self.capabilities_file
            .as_ref()
            .and_then(|f| f.hmac_secret_name().map(|s| s.to_string()))
    }

    /// Get the webhook secret name from capabilities.
    pub fn webhook_secret_name(&self) -> String {
        self.capabilities_file
            .as_ref()
            .map(|f| f.webhook_secret_name())
            .unwrap_or_else(|| format!("{}_webhook_secret", self.channel.channel_name()))
    }
}

/// Results from loading multiple channels.
#[derive(Default)]
pub struct LoadResults {
    /// Successfully loaded channels with their capabilities.
    pub loaded: Vec<LoadedChannel>,

    /// Errors encountered (path, error).
    pub errors: Vec<(PathBuf, WasmChannelError)>,
}

impl LoadResults {
    /// Check if all channels loaded successfully.
    pub fn all_succeeded(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get the count of successfully loaded channels.
    pub fn success_count(&self) -> usize {
        self.loaded.len()
    }

    /// Get the count of failed channels.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Take ownership of loaded channels (extracts just the WasmChannel).
    pub fn take_channels(self) -> Vec<WasmChannel> {
        self.loaded.into_iter().map(|l| l.channel).collect()
    }
}
