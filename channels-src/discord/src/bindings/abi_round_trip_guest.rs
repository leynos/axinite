//! The import-free recording guest the ABI round trips run against.
//!
//! The shipped channel refuses every broadcast with a constant error, so its
//! result cannot show whether the arguments were lifted correctly. This guest
//! has no host import, which keeps it runnable on a non-WebAssembly target, and
//! it returns the lifted values so every field is observable. It is declared
//! beside the round trips it serves, as a 32-bit test module.

use std::sync::Mutex;

use super::abi_round_trip_layout::lower;
use super::exports::near::agent::channel::{
    _export_on_status_cabi, AgentResponse, ChannelConfig, Guest, IncomingHttpRequest,
    OutgoingHttpResponse, StatusType, StatusUpdate,
};

/// An import-free stand-in for the channel guest that echoes what it received.
pub(super) struct RecordingChannel;

impl Guest for RecordingChannel {
    fn on_start(config_json: String) -> Result<ChannelConfig, String> {
        Err(config_json)
    }

    fn on_http_request(req: IncomingHttpRequest) -> OutgoingHttpResponse {
        OutgoingHttpResponse {
            status: 200,
            headers_json: req.headers_json,
            body: req.body,
        }
    }

    fn on_poll() {}

    fn on_respond(response: AgentResponse) -> Result<(), String> {
        Err(echo_agent_response(&response))
    }

    fn on_status(update: StatusUpdate) {
        STATUS_RECORDINGS.lock().expect("status recordings").push((
            status_index(update.status),
            update.message,
            update.metadata_json,
        ));
    }

    fn on_broadcast(user_id: String, response: AgentResponse) -> Result<(), String> {
        Err(format!("{user_id}\u{1f}{}", echo_agent_response(&response)))
    }

    fn on_shutdown() {}
}

/// What [`RecordingChannel`] saw in each `on_status` call, in call order.
static STATUS_RECORDINGS: Mutex<Vec<(u8, String, String)>> = Mutex::new(Vec::new());

/// The recordings whose metadata is exactly `marker`, cloned out of the lock.
///
/// Tests run in parallel threads, so a status test must identify its own
/// recording instead of counting entries: the metadata string is that test's
/// marker, and each status test uses one no other test uses.
pub(super) fn status_recordings_with(marker: &str) -> Vec<(u8, String, String)> {
    STATUS_RECORDINGS
        .lock()
        .expect("status recordings")
        .iter()
        .filter(|(_, _, metadata)| metadata == marker)
        .cloned()
        .collect()
}

/// Deliver one `on_status` call through the ABI with the given fields.
pub(super) fn record_status(index: u8, message: &str, metadata: &str) {
    let (message_ptr, message_len) = lower(message);
    let (metadata_ptr, metadata_len) = lower(metadata);
    // SAFETY: both strings were allocated by `lower` and are handed over to
    // the shim, which lifts them into owned `String`s and drops them. The
    // discriminant is a `status-type` case chosen by the caller, widened to
    // the `i32` the canonical ABI gives enumerated types, and `on_status`
    // returns nothing, so there is no return area to release.
    unsafe {
        _export_on_status_cabi::<RecordingChannel>(
            i32::from(index),
            message_ptr,
            message_len,
            metadata_ptr,
            metadata_len,
        );
    }
}

/// The WIT discriminant of each `status-type` case, in declaration order.
fn status_index(status: StatusType) -> u8 {
    match status {
        StatusType::Thinking => 0,
        StatusType::Done => 1,
        StatusType::Interrupted => 2,
        StatusType::ToolStarted => 3,
        StatusType::ToolCompleted => 4,
        StatusType::ToolResult => 5,
        StatusType::ApprovalNeeded => 6,
        StatusType::Status => 7,
        StatusType::JobStarted => 8,
        StatusType::AuthRequired => 9,
        StatusType::AuthCompleted => 10,
    }
}

/// Encode an agent response so a test can assert every lifted field.
///
/// Fields are separated by the ASCII unit separator, which cannot appear in
/// the fixtures. An absent thread is marked `<none>` and an empty one `<some>`
/// with no text, so the two states stay distinct. Each attachment contributes
/// its filename, MIME type and data, the data hex-encoded so arbitrary bytes
/// survive the comparison.
fn echo_agent_response(response: &AgentResponse) -> String {
    let thread = match response.thread_id.as_deref() {
        None => "<none>".to_string(),
        Some(text) => format!("<some>{text}"),
    };
    let mut encoded = format!(
        "{}\u{1f}{}\u{1f}{thread}\u{1f}{}\u{1f}{}",
        response.message_id,
        response.content,
        response.metadata_json,
        response.attachments.len()
    );
    for attachment in &response.attachments {
        encoded.push_str(&format!(
            "\u{1f}{}\u{1f}{}\u{1f}{}",
            attachment.filename,
            attachment.mime_type,
            hex(&attachment.data)
        ));
    }
    encoded
}

/// Encode bytes as lowercase hexadecimal, so arbitrary data can be asserted.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
