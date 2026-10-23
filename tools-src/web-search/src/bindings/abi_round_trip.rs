//! Round trips through the generated canonical-ABI glue, run under Miri.
//!
//! The host calls the exported shims with pointers into guest linear memory.
//! These tests play the host: they lower each argument into an
//! allocator-owned buffer exactly as the host's `cabi_realloc` would, call the
//! generated `_export_*_cabi` shim, decode the return area it fills in, and
//! pass that area to the generated `__post_return_*` shim. `make miri` then
//! checks pointer provenance, the ownership transfer of lowered strings, and
//! that every allocation is freed exactly once.
//!
//! Only paths that make no host import are exercised, because on a
//! non-WebAssembly target every import is `unreachable`. The generator lays
//! the return area out for WebAssembly's 4-byte pointers, so `make miri` runs
//! these tests for a 32-bit target (`MIRI_ABI_TARGET`, `i686-unknown-linux-gnu`)
//! and with Stacked Borrows off; the Makefile explains both. A `string` is a
//! pointer word followed by a length word in the return area decoded here,
//! and an `option<string>` is a discriminant word followed by that pair.

use core::mem::size_of;
use core::{ptr, slice};

use super::exports::near::agent::tool::{
    __post_return_description, __post_return_execute, __post_return_schema,
    _export_description_cabi, _export_execute_cabi, _export_schema_cabi, Guest,
};
use crate::WebSearchTool;

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

/// Read the `option<string>` whose discriminant is at word `word`.
///
/// # Safety
///
/// As for [`read_string`], with a discriminant word before the string words.
unsafe fn read_option(area: *const u8, word: usize) -> Option<String> {
    // SAFETY: the caller guarantees the layout described above.
    unsafe {
        match *area.add(word * size_of::<usize>()).cast::<u8>() {
            0 => None,
            1 => Some(read_string(area, word + 1)),
            other => panic!("invalid option discriminant {other}"),
        }
    }
}

/// Call `execute` through the ABI and return its `(output, error)` pair.
fn execute(params: &str, context: Option<&str>) -> (Option<String>, Option<String>) {
    let (params_ptr, params_len) = lower(params);
    let (present, context_ptr, context_len) = match context {
        Some(text) => {
            let (pointer, len) = lower(text);
            (1, pointer, len)
        }
        None => (0, ptr::null_mut(), 0),
    };
    // SAFETY: both strings were allocated by `lower` and are handed over to
    // the shim, which lifts them into owned `String`s. The shim returns a live
    // return area that is decoded before, and released by, `__post_return`.
    unsafe {
        let area = _export_execute_cabi::<WebSearchTool>(
            params_ptr,
            params_len,
            present,
            context_ptr,
            context_len,
        );
        let result = (read_option(area, 0), read_option(area, 3));
        __post_return_execute::<WebSearchTool>(area);
        result
    }
}

#[test]
fn description_crosses_the_abi_and_is_freed() {
    // SAFETY: the shim returns a live return area holding one string, which is
    // copied out and then released exactly once by `__post_return`.
    let text = unsafe {
        let area = _export_description_cabi::<WebSearchTool>();
        let text = read_string(area, 0);
        __post_return_description::<WebSearchTool>(area);
        text
    };
    assert_eq!(text, <WebSearchTool as Guest>::description());
}

#[test]
fn schema_crosses_the_abi_and_is_freed() {
    // SAFETY: as for `description_crosses_the_abi_and_is_freed`.
    let text = unsafe {
        let area = _export_schema_cabi::<WebSearchTool>();
        let text = read_string(area, 0);
        __post_return_schema::<WebSearchTool>(area);
        text
    };
    assert_eq!(text, <WebSearchTool as Guest>::schema());
}

#[test]
fn invalid_parameters_come_back_as_an_error_string() {
    let (output, error) = execute("not json", None);
    assert_eq!(output, None);
    assert!(error.is_some_and(|message| message.starts_with("Invalid parameters")));
}

#[test]
fn an_empty_parameter_string_crosses_the_abi() {
    let (output, error) = execute("", None);
    assert_eq!(output, None);
    assert!(error.is_some_and(|message| message.starts_with("Invalid parameters")));
}

#[test]
fn a_present_context_is_lifted_and_dropped() {
    let (output, error) = execute(r#"{"query": ""}"#, Some(r#"{"job": "j"}"#));
    assert_eq!(output, None);
    assert_eq!(error.as_deref(), Some("'query' must not be empty"));
}

#[test]
fn a_long_multibyte_query_crosses_the_abi_intact() {
    let query = "é".repeat(1100);
    let params = format!(r#"{{"query": "{query}"}}"#);
    let (output, error) = execute(&params, None);
    assert_eq!(output, None);
    assert_eq!(
        error.as_deref(),
        Some("'query' exceeds maximum length of 2000 characters")
    );
}
