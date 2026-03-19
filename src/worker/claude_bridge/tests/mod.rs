//! Tests for the Claude Code bridge helpers.

pub(super) use super::fs_setup::{build_permission_settings, copy_dir_recursive};
pub(super) use super::ndjson::{
    ClaudeStreamEvent, ContentBlock, MessageWrapper, stream_event_to_payloads, truncate,
};
pub(super) use crate::worker::api::{JobEventPayload, JobEventType};

mod claude_fs_setup;
mod claude_ndjson_parsing;
mod claude_payload_mapping;
mod utils;
