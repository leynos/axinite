//! Allocation and layout helpers for the canonical-ABI round trips.
//!
//! These helpers play the host. They allocate argument buffers the way the
//! host's `cabi_realloc` would, lay a `list<attachment>` out in the canonical
//! record shape, build the `agent-response` that the fixed broadcast arguments
//! describe, and decode a string out of a return area a shim fills in. They are
//! declared beside the round trips they serve, as a 32-bit test module, because
//! they exist only to exercise the generated glue.

use core::mem::size_of;
use core::{ptr, slice};

use super::exports::near::agent::channel::{AgentResponse, Attachment};

/// Lower `text` the way the host's `cabi_realloc` would.
///
/// The callee takes ownership of the returned buffer, so the caller must not
/// free it.
pub(super) fn lower(text: &str) -> (*mut u8, usize) {
    lower_bytes(text.as_bytes())
}

/// Lower `bytes` the way the host's `cabi_realloc` would.
///
/// The callee takes ownership of the returned buffer, so the caller must not
/// free it. An empty list needs no allocation: its canonical pointer is any
/// aligned, non-null address, which `NonNull::dangling` supplies without the
/// integer-to-pointer cast that `cabi_realloc` uses for that case.
pub(super) fn lower_bytes(bytes: &[u8]) -> (*mut u8, usize) {
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
pub(super) struct AttachmentFixture<'a> {
    /// Original filename.
    pub(super) filename: &'a str,
    /// MIME type.
    pub(super) mime_type: &'a str,
    /// Raw file bytes.
    pub(super) data: &'a [u8],
}

/// Lower a `list<attachment>` into the canonical record layout.
///
/// Each record is the three pairs `filename`, `mime-type` and `data`, so it is
/// six words wide on the 32-bit target these tests build for. The callee takes
/// ownership of the block and of every buffer it references.
pub(super) fn lower_attachments(attachments: &[AttachmentFixture<'_>]) -> (*mut u8, usize) {
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
pub(super) unsafe fn read_string(area: *const u8, word: usize) -> String {
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

/// The `agent-response` the fixed arguments of `broadcast_as` describe.
pub(super) fn agent_response(
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
