//! Calls and assertions that drive the generated shims for a whole case.
//!
//! Each function here lowers one set of broadcast arguments, calls the
//! generated `_export_on_broadcast_cabi` shim as a chosen guest, decodes the
//! result, and releases the return area through `__post_return_on_broadcast`.
//! [`Cases`] supplies the repeatable generated arguments the sweep needs.

use core::ptr;

use super::abi_round_trip_guest::RecordingChannel;
use super::abi_round_trip_layout::{
    agent_response, lower, lower_attachments, read_string, AttachmentFixture,
};
use super::exports::near::agent::channel::{
    __post_return_on_broadcast, _export_on_broadcast_cabi, Guest,
};
use crate::DiscordChannel;

/// Call `on_broadcast` through the ABI as guest `G` and return its error text.
pub(super) fn broadcast_as<G: Guest>(
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
pub(super) fn broadcast(
    user_id: &str,
    content: &str,
    thread_id: Option<&str>,
    attachments: &[AttachmentFixture<'_>],
) -> Result<(), String> {
    broadcast_as::<DiscordChannel>(user_id, content, thread_id, attachments)
}

/// Require the shim to hand [`RecordingChannel`] exactly the arguments it was
/// given.
///
/// `broadcast_as` releases the return area through
/// `__post_return_on_broadcast` for every case, so each generated input also
/// exercises the post-return cleanup of the lowered `user_id`, `content` and
/// `thread_id`.
pub(super) fn assert_lifted_unchanged(
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

/// Require the shim's result to equal the guest function's own result.
///
/// `on_broadcast` refuses every request before it reaches a host import, which
/// is `unreachable` on a non-WebAssembly target. The comparison therefore
/// exercises the lifting of arbitrary arguments against the same refusal the
/// guest returns when reached without the ABI. The refusal is a constant, so
/// these cases prove ownership transfer rather than value; the recording guest
/// in `generated_broadcast_arguments_cross_the_abi_unchanged` observes the
/// values.
pub(super) fn assert_matches_the_guest(
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

/// A deterministic generator of argument strings.
///
/// A fixed seed makes the sweep repeatable, and drawing scalars from every
/// UTF-8 length class keeps multi-byte and multi-scalar content in each run.
pub(super) struct Cases(u64);

impl Cases {
    /// Start a sequence at `seed`.
    pub(super) const fn new(seed: u64) -> Self {
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
    pub(super) fn string(&mut self, scalars: usize) -> String {
        const ALPHABET: [char; 4] = ['a', 'é', '€', '🦀'];
        (0..scalars)
            .map(|_| ALPHABET[self.below(ALPHABET.len())])
            .collect()
    }

    /// Draw `len` bytes covering every byte value, so attachment data includes
    /// NUL and sequences that are not valid UTF-8.
    pub(super) fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len)
            .map(|_| u8::try_from(self.next() & 0xff).expect("masked to a byte"))
            .collect()
    }
}

/// Lengths on both sides of the allocator's small and medium size classes.
pub(super) const GENERATED_LENGTHS: [usize; 16] =
    [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65];
