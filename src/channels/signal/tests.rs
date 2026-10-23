//! Unit tests for Signal channel pairing storage and message handling.
//!
//! Shared fixtures live here; the tests themselves are grouped into themed
//! submodules.

mod allowlist;
mod config;
mod envelope_content;
mod envelope_policy;
mod rpc_params;
mod sse;
mod targets;
mod timestamps;
mod validation;

use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use tempfile::TempDir;

use crate::channels::{NativeChannel, OutgoingResponse};

use super::*;

static SIGNAL_PAIRING_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct SignalPairingStoreGuard {
    _guard: MutexGuard<'static, ()>,
    _temp_dir: TempDir,
}

impl SignalPairingStoreGuard {
    fn install() -> std::io::Result<Self> {
        let guard = SIGNAL_PAIRING_TEST_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let temp_dir = tempfile::tempdir()?;
        *SIGNAL_PAIRING_STORE_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(temp_dir.path().to_path_buf());
        Ok(Self {
            _guard: guard,
            _temp_dir: temp_dir,
        })
    }
}

impl Drop for SignalPairingStoreGuard {
    fn drop(&mut self) {
        *SIGNAL_PAIRING_STORE_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

fn make_config() -> SignalConfig {
    SignalConfig {
        http_url: "http://127.0.0.1:8686".to_string(),
        account: "+1234567890".to_string(),
        allow_from: vec!["+1111111111".to_string()],
        allow_from_groups: vec![],
        dm_policy: "allowlist".to_string(),
        group_policy: "disabled".to_string(),
        group_allow_from: vec![],
        ignore_attachments: false,
        ignore_stories: false,
    }
}

/// Create a config that allows a specific group (and all senders).
fn make_config_with_allowed_group(group_id: &str) -> SignalConfig {
    SignalConfig {
        http_url: "http://127.0.0.1:8686".to_string(),
        account: "+1234567890".to_string(),
        allow_from: vec!["*".to_string()],
        allow_from_groups: vec![group_id.to_string()],
        dm_policy: "allowlist".to_string(),
        group_policy: "allowlist".to_string(),
        group_allow_from: vec![],
        ignore_attachments: true,
        ignore_stories: true,
    }
}

/// Build a channel from a literal test config.
///
/// `SignalChannel::new` only fails if the HTTP client backend cannot
/// initialise, which does not depend on the config; panic with context.
fn make_channel_from_config(config: SignalConfig) -> SignalChannel {
    match SignalChannel::new(config) {
        Ok(channel) => channel,
        Err(e) => panic!("SignalChannel::new should succeed for test config: {e}"),
    }
}

fn make_channel() -> SignalChannel {
    make_channel_from_config(make_config())
}

fn make_channel_with_allowed_group(group_id: &str) -> SignalChannel {
    make_channel_from_config(make_config_with_allowed_group(group_id))
}

fn make_envelope(source_number: Option<&str>, message: Option<&str>) -> Envelope {
    Envelope {
        source: source_number.map(String::from),
        source_number: source_number.map(String::from),
        source_name: None,
        source_uuid: None,
        data_message: message.map(|m| DataMessage {
            message: Some(m.to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    }
}
