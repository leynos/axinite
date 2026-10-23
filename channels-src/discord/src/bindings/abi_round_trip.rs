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
//! Only paths that make no host import are exercised, because on a
//! non-WebAssembly target every import is `unreachable`. The generator lays
//! the return area out for WebAssembly's 4-byte pointers, so `make miri` runs
//! these tests for a 32-bit target (`MIRI_ABI_TARGET`, `i686-unknown-linux-gnu`)
//! and with Stacked Borrows off; the Makefile explains both. A `string` is a
//! pointer word followed by a length word in the return area decoded here,
//! and a `result<_, string>` is a discriminant word followed by that pair.

use core::mem::size_of;
use core::{ptr, slice};

use super::exports::near::agent::channel::{
    __post_return_on_broadcast, _export_on_broadcast_cabi, _export_on_status_cabi,
};
use crate::DiscordChannel;

/// Lower `text` the way the host's `cabi_realloc` would.
///
/// The callee takes ownership of the returned buffer, so the caller must not
/// free it. An empty string needs no allocation: its canonical pointer is any
/// aligned, non-null address, which `NonNull::dangling` supplies without the
/// integer-to-pointer cast that `cabi_realloc` uses for that case.
fn lower(text: &str) -> (*mut u8, usize) {
    if text.is_empty() {
        return (ptr::NonNull::<u8>::dangling().as_ptr(), 0);
    }
    // SAFETY: a null old pointer with old length 0 asks `cabi_realloc` for a
    // fresh allocation of `text.len()` bytes at alignment 1, which is the
    // contract of the canonical ABI's allocation function.
    let buffer = unsafe { wit_bindgen::rt::cabi_realloc(ptr::null_mut(), 0, 1, text.len()) };
    // SAFETY: `buffer` is valid for `text.len()` writes and cannot overlap the
    // borrowed `text`, which the allocator did not hand out.
    unsafe { ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len()) };
    (buffer, text.len())
}

/// Lower an empty `list` of word-aligned records, which has no allocation.
fn lower_empty_list() -> (*mut u8, usize) {
    (ptr::NonNull::<usize>::dangling().as_ptr().cast::<u8>(), 0)
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

/// Call `on_broadcast` through the ABI and return its error text, if any.
fn broadcast(user_id: &str, content: &str, thread_id: Option<&str>) -> Result<(), String> {
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
    let (attachments_ptr, attachments_len) = lower_empty_list();
    // SAFETY: every pointer was allocated by `lower` or `lower_empty_list` and
    // is handed over to the shim, which lifts it into owned values. The shim
    // returns a live return area that is decoded before, and released by,
    // `__post_return`.
    unsafe {
        let area = _export_on_broadcast_cabi::<DiscordChannel>(
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
        __post_return_on_broadcast::<DiscordChannel>(area);
        result
    }
}

#[test]
fn a_broadcast_reports_that_it_is_not_implemented() {
    let error = broadcast("user", "hello", None).expect_err("Discord cannot broadcast");
    assert_eq!(error, "broadcast not yet implemented for Discord channel");
}

#[test]
fn a_broadcast_with_a_thread_lifts_and_drops_every_field() {
    let result = broadcast("user-é", "héllo wörld", Some("thread-1"));
    assert!(result.is_err());
}

#[test]
fn a_status_update_lifts_and_drops_its_strings() {
    let (message_ptr, message_len) = lower("thinking about it");
    let (metadata_ptr, metadata_len) = lower(r#"{"chat_id": 1}"#);
    // SAFETY: both strings were allocated by `lower` and are handed over to the
    // shim, which lifts them into owned `String`s and drops them. The
    // discriminant `0` is the first `status-type` case, so it is in range, and
    // `on_status` returns nothing, so there is no return area to release.
    unsafe {
        _export_on_status_cabi::<DiscordChannel>(
            0,
            message_ptr,
            message_len,
            metadata_ptr,
            metadata_len,
        );
    }
}

#[test]
fn an_empty_status_message_crosses_the_abi() {
    let (message_ptr, message_len) = lower("");
    let (metadata_ptr, metadata_len) = lower("");
    // SAFETY: as for `a_status_update_lifts_and_drops_its_strings`; empty
    // strings are dangling, aligned pointers with length zero.
    unsafe {
        _export_on_status_cabi::<DiscordChannel>(
            10,
            message_ptr,
            message_len,
            metadata_ptr,
            metadata_len,
        );
    }
}
