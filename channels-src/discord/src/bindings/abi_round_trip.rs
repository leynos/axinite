//! Round trips through the generated canonical-ABI glue, run under Miri.
//!
//! The host calls the exported shims with pointers into guest linear memory.
//! These tests play the host: they lower each argument into an
//! allocator-owned buffer exactly as the host's `cabi_realloc` would, call the
//! generated `_export_*_cabi` shim, decode the return area it fills in, and
//! pass that area to the generated `__post_return_*` shim. `make miri` then
//! checks pointer provenance, the ownership transfer of lowered strings and
//! lists, and that every allocation is freed exactly once.
//!
//! Two guests are exercised. [`DiscordChannel`] is the shipped implementation,
//! and its round trips check the logic the host will actually reach. The
//! import-free [`RecordingChannel`] returns what it received instead: the
//! shipped channel refuses every broadcast with a constant, so its result
//! cannot show whether `user_id`, `content` or `thread_id` survived the ABI.
//! The recording guest echoes them, including every attachment's filename,
//! MIME type and bytes, and records what `on_status` lifted.
//!
//! Only paths that make no host import are exercised, because on a
//! non-WebAssembly target every import is `unreachable`. The generator lays
//! the return area out for WebAssembly's 4-byte pointers, so `make miri` runs
//! these tests for a 32-bit target (`MIRI_ABI_TARGET`, `i686-unknown-linux-gnu`).
//! They also run with Stacked Borrows off: `wit-bindgen` lowers a string from
//! the `as_ptr()` of a boxed slice it then forgets, which both of Miri's
//! aliasing models reject, as `docs/rust-boundary-verification.md` records
//! under "Findings in the generated glue". Allocation, bounds, alignment,
//! initialization and leak checks stay on. A `string` is a pointer word
//! followed by a length word in the return area decoded here, and a
//! `result<_, string>` is a discriminant word followed by that pair.

use core::mem::size_of;
use core::{ptr, slice};
use std::sync::Mutex;

use super::exports::near::agent::channel::{
    __post_return_on_broadcast, _export_on_broadcast_cabi, _export_on_status_cabi, AgentResponse,
    Attachment, ChannelConfig, Guest, IncomingHttpRequest, OutgoingHttpResponse, StatusType,
    StatusUpdate,
};
use crate::DiscordChannel;

/// Lower `text` the way the host's `cabi_realloc` would.
///
/// The callee takes ownership of the returned buffer, so the caller must not
/// free it.
fn lower(text: &str) -> (*mut u8, usize) {
    lower_bytes(text.as_bytes())
}

/// Lower `bytes` the way the host's `cabi_realloc` would.
///
/// The callee takes ownership of the returned buffer, so the caller must not
/// free it. An empty list needs no allocation: its canonical pointer is any
/// aligned, non-null address, which `NonNull::dangling` supplies without the
/// integer-to-pointer cast that `cabi_realloc` uses for that case.
fn lower_bytes(bytes: &[u8]) -> (*mut u8, usize) {
    if bytes.is_empty() {
        return (ptr::NonNull::<u8>::dangling().as_ptr(), 0);
    }
    // SAFETY: a null old pointer with old length 0 asks `cabi_realloc` for a
    // fresh allocation of `bytes.len()` bytes at alignment 1, which is the
    // contract of the canonical ABI's allocation function.
    let buffer = unsafe { wit_bindgen::rt::cabi_realloc(ptr::null_mut(), 0, 1, bytes.len()) };
    // SAFETY: `buffer` is valid for `bytes.len()` writes and cannot overlap the
    // borrowed `bytes`, which the allocator did not hand out.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len()) };
    (buffer, bytes.len())
}

/// One attachment a test sends, in the shape `agent-response` declares.
struct AttachmentFixture<'a> {
    /// Original filename.
    filename: &'a str,
    /// MIME type.
    mime_type: &'a str,
    /// Raw file bytes.
    data: &'a [u8],
}

/// Lower a `list<attachment>` into the canonical record layout.
///
/// Each record is the three pairs `filename`, `mime-type` and `data`, so it is
/// six words wide on the 32-bit target these tests build for. The callee takes
/// ownership of the block and of every buffer it references.
fn lower_attachments(attachments: &[AttachmentFixture<'_>]) -> (*mut u8, usize) {
    const WORDS_PER_RECORD: usize = 6;
    if attachments.is_empty() {
        return (ptr::NonNull::<usize>::dangling().as_ptr().cast::<u8>(), 0);
    }
    let size = attachments.len() * WORDS_PER_RECORD * size_of::<usize>();
    // SAFETY: a null old pointer with old length 0 asks `cabi_realloc` for a
    // fresh block of `size` bytes at word alignment, which is the alignment
    // the canonical ABI uses for a list whose records hold pointers.
    let base =
        unsafe { wit_bindgen::rt::cabi_realloc(ptr::null_mut(), 0, size_of::<usize>(), size) };
    for (index, attachment) in attachments.iter().enumerate() {
        let (filename_ptr, filename_len) = lower(attachment.filename);
        let (mime_ptr, mime_len) = lower(attachment.mime_type);
        let (data_ptr, data_len) = lower_bytes(attachment.data);
        let record = base.cast::<usize>().wrapping_add(index * WORDS_PER_RECORD);
        // The pointer words are written as pointers rather than as integers, so
        // the provenance of each buffer survives into the record the shim
        // reads. A pointer-to-integer cast would leave these words without
        // provenance, which Miri rejects when the glue lifts the string.
        // SAFETY: `record` points at the start of record `index` inside the
        // block allocated above, and each of the six writes stays within it.
        unsafe {
            record.cast::<*mut u8>().write(filename_ptr);
            record.add(1).write(filename_len);
            record.add(2).cast::<*mut u8>().write(mime_ptr);
            record.add(3).write(mime_len);
            record.add(4).cast::<*mut u8>().write(data_ptr);
            record.add(5).write(data_len);
        }
    }
    (base, attachments.len())
}

/// Read the string stored at word `word` of a return area.
///
/// # Safety
///
/// `area` must point to a live return area holding a pointer and a length at
/// that word, as filled in by a generated export shim.
unsafe fn read_string(area: *const u8, word: usize) -> String {
    // SAFETY: the caller guarantees the area holds a pointer word then a length
    // word at `word`, and that the pointed-to bytes stay live until
    // `__post_return_*` runs, which happens after this copy.
    unsafe {
        let base = area.add(word * size_of::<usize>());
        let data = *base.cast::<*const u8>();
        let len = *base.add(size_of::<usize>()).cast::<usize>();
        String::from_utf8(slice::from_raw_parts(data, len).to_vec()).expect("UTF-8 string")
    }
}

/// The `agent-response` the fixed arguments of [`broadcast_as`] describe.
fn agent_response(
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) -> AgentResponse {
    AgentResponse {
        message_id: "message-id".to_string(),
        content: content.to_string(),
        thread_id: thread_id.map(str::to_owned),
        metadata_json: "{}".to_string(),
        attachments: attachments
            .iter()
            .map(|attachment| Attachment {
                filename: attachment.filename.to_string(),
                mime_type: attachment.mime_type.to_string(),
                data: attachment.data.to_vec(),
            })
            .collect(),
    }
}

/// Call `on_broadcast` through the ABI as guest `G` and return its error text.
fn broadcast_as<G: Guest>(
    user_id: &str,
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) -> Result<(), String> {
    let (user_ptr, user_len) = lower(user_id);
    let (message_ptr, message_len) = lower("message-id");
    let (content_ptr, content_len) = lower(content);
    let (present, thread_ptr, thread_len) = match thread_id {
        Some(text) => {
            let (pointer, len) = lower(text);
            (1, pointer, len)
        }
        None => (0, ptr::null_mut(), 0),
    };
    let (metadata_ptr, metadata_len) = lower("{}");
    let (attachments_ptr, attachments_len) = lower_attachments(attachments);
    // SAFETY: every pointer was allocated by `lower` or `lower_attachments`
    // and is handed over to the shim, which lifts it into owned values. The
    // shim returns a live return area that is decoded before, and released by,
    // `__post_return`.
    unsafe {
        let area = _export_on_broadcast_cabi::<G>(
            user_ptr,
            user_len,
            message_ptr,
            message_len,
            content_ptr,
            content_len,
            present,
            thread_ptr,
            thread_len,
            metadata_ptr,
            metadata_len,
            attachments_ptr,
            attachments_len,
        );
        let result = match *area.cast::<u8>() {
            0 => Ok(()),
            1 => Err(read_string(area, 1)),
            other => panic!("invalid result discriminant {other}"),
        };
        __post_return_on_broadcast::<G>(area);
        result
    }
}

/// Call `on_broadcast` through the ABI as the shipped channel.
fn broadcast(
    user_id: &str,
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) -> Result<(), String> {
    broadcast_as::<DiscordChannel>(user_id, content, thread_id, attachments)
}

/// An import-free stand-in for the channel guest that echoes what it received.
///
/// The shipped channel refuses every broadcast with a constant error, so its
/// result cannot show whether the arguments were lifted correctly. This guest
/// has no host import either, which keeps it runnable on a non-WebAssembly
/// target, and it returns the lifted values so every field is observable.
struct RecordingChannel;

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
fn status_recordings_with(marker: &str) -> Vec<(u8, String, String)> {
    STATUS_RECORDINGS
        .lock()
        .expect("status recordings")
        .iter()
        .filter(|(_, _, metadata)| metadata == marker)
        .cloned()
        .collect()
}

/// Deliver one `on_status` call through the ABI with the given fields.
fn record_status(index: u8, message: &str, metadata: &str) {
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

/// Require the shim to hand [`RecordingChannel`] exactly the arguments it was
/// given.
///
/// `broadcast_as` releases the return area through
/// `__post_return_on_broadcast` for every case, so each generated input also
/// exercises the post-return cleanup of the lowered `user_id`, `content` and
/// `thread_id`.
fn assert_lifted_unchanged(
    user_id: &str,
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) {
    let expected = <RecordingChannel as Guest>::on_broadcast(
        user_id.to_string(),
        agent_response(content, thread_id, attachments),
    );
    assert_eq!(
        broadcast_as::<RecordingChannel>(user_id, content, thread_id, attachments),
        expected,
        "results differ"
    );
}

/// A deterministic generator of argument strings.
///
/// A fixed seed makes the sweep repeatable, and drawing scalars from every
/// UTF-8 length class keeps multi-byte and multi-scalar content in each run.
struct Cases(u64);

impl Cases {
    /// Start a sequence at `seed`.
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Draw the next value of a xorshift64 sequence.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Draw a value below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bound fits u64");
        usize::try_from(self.next() % bound).expect("remainder fits usize")
    }

    /// Draw a string of `scalars` scalars from every UTF-8 length class.
    fn string(&mut self, scalars: usize) -> String {
        const ALPHABET: [char; 4] = ['a', 'é', '€', '🦀'];
        (0..scalars)
            .map(|_| ALPHABET[self.below(ALPHABET.len())])
            .collect()
    }

    /// Draw `len` bytes covering every byte value, so attachment data includes
    /// NUL and sequences that are not valid UTF-8.
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len)
            .map(|_| u8::try_from(self.next() & 0xff).expect("masked to a byte"))
            .collect()
    }
}

/// Lengths on both sides of the allocator's small and medium size classes.
const GENERATED_LENGTHS: [usize; 16] = [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65];

#[test]
fn a_broadcast_reports_that_it_is_not_implemented() {
    let error = broadcast("user", "hello", None, &[]).expect_err("Discord cannot broadcast");
    assert_eq!(error, "broadcast not yet implemented for Discord channel");
}

#[test]
fn a_broadcast_with_a_thread_lifts_and_drops_every_field() {
    let error = broadcast_as::<RecordingChannel>("user-é", "héllo wörld", Some("thread-1"), &[])
        .expect_err("the recording guest reports the lifted arguments as an error");
    assert_eq!(
        error,
        "user-é\u{1f}message-id\u{1f}héllo wörld\u{1f}<some>thread-1\u{1f}{}\u{1f}0"
    );
}

#[test]
fn a_broadcast_without_a_thread_lifts_and_drops_every_field() {
    let error = broadcast_as::<RecordingChannel>("user", "hello", None, &[])
        .expect_err("the recording guest reports the lifted arguments as an error");
    assert_eq!(
        error,
        "user\u{1f}message-id\u{1f}hello\u{1f}<none>\u{1f}{}\u{1f}0"
    );
}

#[test]
fn a_broadcast_with_attachments_lifts_and_drops_every_field() {
    let attachments = [
        AttachmentFixture {
            filename: "shot-é.png",
            mime_type: "image/png",
            data: b"\x89PNG",
        },
        AttachmentFixture {
            filename: "notes.txt",
            mime_type: "text/plain",
            data: b"hello",
        },
    ];
    let error = broadcast_as::<RecordingChannel>("user", "hello", None, &attachments)
        .expect_err("the recording guest reports the lifted arguments as an error");
    assert_eq!(
        error,
        concat!(
            "user\u{1f}message-id\u{1f}hello\u{1f}<none>\u{1f}{}\u{1f}2",
            "\u{1f}shot-é.png\u{1f}image/png\u{1f}89504e47",
            "\u{1f}notes.txt\u{1f}text/plain\u{1f}68656c6c6f",
        )
    );
}

#[test]
fn a_status_update_lifts_and_drops_its_strings() {
    let marker = r#"{"chat_id": 1}"#;
    record_status(0, "thinking about it", marker);
    assert_eq!(
        status_recordings_with(marker),
        vec![(0u8, "thinking about it".to_string(), marker.to_string())]
    );
}

#[test]
fn an_empty_status_message_crosses_the_abi() {
    // The empty metadata string is this test's marker; no other status test
    // records it.
    record_status(10, "", "");
    assert_eq!(
        status_recordings_with(""),
        vec![(10u8, String::new(), String::new())]
    );
}

#[test]
fn status_recordings_are_selected_by_their_marker() {
    let marker = r#"{"isolation": "target"}"#;
    record_status(3, "target message", marker);
    let interleaved = std::thread::spawn(|| record_status(4, "other", r#"{"isolation": "other"}"#));
    interleaved
        .join()
        .expect("the interleaved recording thread");
    assert_eq!(
        status_recordings_with(marker),
        vec![(3u8, "target message".to_string(), marker.to_string())],
        "another thread's recording must not be selected"
    );
}

/// Require the shim's result to equal the guest function's own result.
///
/// `on_broadcast` refuses every request before it reaches a host import, which
/// is `unreachable` on a non-WebAssembly target. The comparison therefore
/// exercises the lifting of arbitrary arguments against the same refusal the
/// guest returns when reached without the ABI. The refusal is a constant, so
/// these cases prove ownership transfer rather than value; the recording guest
/// in `generated_broadcast_arguments_cross_the_abi_unchanged` observes the
/// values.
fn assert_matches_the_guest(
    user_id: &str,
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) {
    let expected = <DiscordChannel as Guest>::on_broadcast(
        user_id.to_string(),
        agent_response(content, thread_id, attachments),
    );
    assert_eq!(
        broadcast(user_id, content, thread_id, attachments),
        expected,
        "results differ"
    );
}

#[test]
fn strings_of_every_length_class_cross_the_abi() {
    let units = ["a", "é", "€", "🦀"];
    // Lengths either side of the allocator's small and medium size classes.
    for unit in units {
        for repeat in [0usize, 1, 2, 7, 8, 9, 15, 16, 17, 255, 256, 1000] {
            let text = unit.repeat(repeat);
            assert_matches_the_guest(&text, &text, Some(&text), &[]);
            assert_matches_the_guest(&text, &text, None, &[]);
        }
    }
}

#[test]
fn every_thread_state_crosses_the_abi() {
    let long = "é".repeat(1000);
    let states = [None, Some(""), Some("t"), Some("🦀"), Some(long.as_str())];
    for thread_id in states {
        assert_matches_the_guest("user", "content", thread_id, &[]);
        assert_matches_the_guest("", "", thread_id, &[]);
    }
}

#[test]
fn generated_broadcast_arguments_cross_the_abi_unchanged() {
    let mut cases = Cases::new(0x5eed_0000_d15c_0001);
    for round in 0..40usize {
        let user_id = cases.string(GENERATED_LENGTHS[round % GENERATED_LENGTHS.len()]);
        let content = cases.string(GENERATED_LENGTHS[(round + 5) % GENERATED_LENGTHS.len()]);
        // Cycle the optional state so `None`, `Some("")` and a non-empty
        // `Some` appear in every run, as distinct cases.
        let thread_id = match round % 3 {
            0 => None,
            1 => Some(cases.string(GENERATED_LENGTHS[(round + 11) % GENERATED_LENGTHS.len()])),
            _ => Some(String::new()),
        };
        // Cycle the attachment count so the empty list and non-empty lists of
        // one and two records all appear, each with a generated filename, MIME
        // type and arbitrary bytes.
        let count = round % 3;
        let filenames: Vec<String> = (0..count)
            .map(|index| cases.string(GENERATED_LENGTHS[(round + index) % GENERATED_LENGTHS.len()]))
            .collect();
        let mime_types: Vec<String> = (0..count)
            .map(|_| cases.string(GENERATED_LENGTHS[(round + 7) % GENERATED_LENGTHS.len()]))
            .collect();
        let data_blobs: Vec<Vec<u8>> = (0..count)
            .map(|index| {
                cases.bytes(GENERATED_LENGTHS[(round + index + 3) % GENERATED_LENGTHS.len()])
            })
            .collect();
        let attachments: Vec<AttachmentFixture<'_>> = filenames
            .iter()
            .zip(&mime_types)
            .zip(&data_blobs)
            .map(|((filename, mime_type), data)| AttachmentFixture {
                filename,
                mime_type,
                data,
            })
            .collect();
        assert_lifted_unchanged(&user_id, &content, thread_id.as_deref(), &attachments);
    }
}
