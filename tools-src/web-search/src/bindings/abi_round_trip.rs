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
//! Two guests are exercised. [`WebSearchTool`] is the shipped implementation,
//! and its round trips check the logic the host will actually reach. The
//! import-free [`RecordingTool`] returns what it received instead of
//! searching, which makes `params` and `context` observable in the return
//! area: the shipped tool ignores `context`, so its own result cannot show
//! whether the optional argument survived the ABI.
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
//! followed by a length word in the return area decoded here, and an
//! `option<string>` is a discriminant word followed by that pair.

use core::mem::size_of;
use core::{ptr, slice};

use super::exports::near::agent::tool::{
    __post_return_description, __post_return_execute, __post_return_schema,
    _export_description_cabi, _export_execute_cabi, _export_schema_cabi, Guest, Request, Response,
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

/// Call `execute` through the ABI as guest `G` and return its `(output, error)`.
fn execute_as<G: Guest>(params: &str, context: Option<&str>) -> (Option<String>, Option<String>) {
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
        let area =
            _export_execute_cabi::<G>(params_ptr, params_len, present, context_ptr, context_len);
        let result = (read_option(area, 0), read_option(area, 3));
        __post_return_execute::<G>(area);
        result
    }
}

/// Call `execute` through the ABI as the shipped tool.
fn execute(params: &str, context: Option<&str>) -> (Option<String>, Option<String>) {
    execute_as::<WebSearchTool>(params, context)
}

/// An import-free stand-in for the tool guest that echoes what it received.
///
/// The shipped tool validates `params` and ignores `context`, so its result
/// cannot show whether either argument was lifted correctly. This guest has no
/// host import either, which keeps it runnable on a non-WebAssembly target, and
/// it returns both fields so each is observable in `output`.
struct RecordingTool;

impl Guest for RecordingTool {
    fn description() -> String {
        "recording tool".to_string()
    }

    fn schema() -> String {
        "{}".to_string()
    }

    fn execute(req: Request) -> Response {
        Response {
            output: Some(echo(&req.params, req.context.as_deref())),
            error: None,
        }
    }
}

/// Encode what a guest received, so a test can assert it character for
/// character.
///
/// The ASCII unit separator cannot appear in the fixtures, and an absent
/// context is marked `<none>` while an empty one is marked `<some>` with no
/// text, so the two states stay distinct.
fn echo(params: &str, context: Option<&str>) -> String {
    let state = match context {
        None => "<none>".to_string(),
        Some(text) => format!("<some>{text}"),
    };
    format!("{params}\u{1f}{state}")
}

/// The result [`RecordingTool`] produces when called without the ABI.
fn recording_guest_result(params: &str, context: Option<&str>) -> Response {
    <RecordingTool as Guest>::execute(Request {
        params: params.to_string(),
        context: context.map(str::to_owned),
    })
}

/// Require the shim to hand [`RecordingTool`] exactly the arguments it was
/// given.
///
/// `execute_as` releases the return area through `__post_return_execute` for
/// every case, so each generated input also exercises the post-return cleanup
/// of the lowered `params` and `context`.
fn assert_lifted_unchanged(params: &str, context: Option<&str>) {
    let (output, error) = execute_as::<RecordingTool>(params, context);
    let expected = recording_guest_result(params, context);
    assert_eq!(output, expected.output, "output differs");
    assert_eq!(error, expected.error, "error differs");
}

/// A deterministic generator of request strings.
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
}

/// Lengths on both sides of the allocator's small and medium size classes.
const GENERATED_LENGTHS: [usize; 16] = [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65];

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
    let (output, error) = execute_as::<RecordingTool>("p", Some(r#"{"job": "j"}"#));
    assert_eq!(output.as_deref(), Some("p\u{1f}<some>{\"job\": \"j\"}"));
    assert_eq!(error, None);
}

#[test]
fn absent_empty_and_present_contexts_are_distinct() {
    let absent = execute_as::<RecordingTool>("p", None)
        .0
        .expect("the recording guest returns an output");
    let empty = execute_as::<RecordingTool>("p", Some(""))
        .0
        .expect("the recording guest returns an output");
    let present = execute_as::<RecordingTool>("p", Some("ctx"))
        .0
        .expect("the recording guest returns an output");
    assert_eq!(absent, "p\u{1f}<none>");
    assert_eq!(empty, "p\u{1f}<some>");
    assert_eq!(present, "p\u{1f}<some>ctx");
    assert_ne!(absent, empty);
    assert_ne!(empty, present);
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

/// Require the shim's result to equal the guest function's own result.
///
/// Every input here fails validation before the guest reaches a host import,
/// which is `unreachable` on a non-WebAssembly target. The comparison is
/// therefore against the same validation the shim lifts its strings into,
/// reached without the ABI. The shipped tool ignores `context`, so these cases
/// exercise its ownership transfer rather than its value; the recording guest
/// in `generated_requests_cross_the_abi_unchanged` observes the value.
fn assert_matches_the_guest(params: &str, context: Option<&str>) {
    let (output, error) = execute(params, context);
    let guest = crate::execute_inner(params);
    assert_eq!(output, guest.as_ref().ok().cloned(), "outputs differ");
    assert_eq!(error, guest.err(), "errors differ");
}

#[test]
fn parameter_strings_of_every_length_class_cross_the_abi() {
    let units = ["a", "é", "€", "🦀"];
    // Lengths either side of the allocator's small and medium size classes.
    for unit in units {
        for repeat in [0usize, 1, 2, 7, 8, 9, 15, 16, 17, 255, 256, 1000] {
            // An empty query fails validation before any host import, so the
            // whole parameter string is lifted and dropped without a trap.
            let padding = unit.repeat(repeat);
            let params = format!("{padding}{}", r#"{"query": ""}"#);
            assert_matches_the_guest(&params, None);
        }
    }
    // A query of every content class above the byte limit fails validation
    // after the shim has lifted the whole string, exercising arbitrary UTF-8.
    for unit in units {
        let query = unit.repeat(2001);
        let params = format!(r#"{{"query": "{query}"}}"#);
        assert_matches_the_guest(&params, None);
    }
}

#[test]
fn every_optional_context_state_crosses_the_abi() {
    let long = "é".repeat(1000);
    let contexts = [None, Some(""), Some("j"), Some("🦀"), Some(long.as_str())];
    for context in contexts {
        assert_matches_the_guest("", context);
        assert_matches_the_guest(r#"{"query": ""}"#, context);
    }
}

#[test]
fn generated_requests_cross_the_abi_unchanged() {
    let mut cases = Cases::new(0x5eed_1234_9abc_def0);
    for round in 0..40usize {
        let scalars = GENERATED_LENGTHS[round % GENERATED_LENGTHS.len()];
        let params = cases.string(scalars);
        // Cycle the optional state so `None`, `Some("")` and a non-empty
        // `Some` appear in every run, as distinct cases.
        let context = match round % 3 {
            0 => None,
            1 => Some(cases.string(scalars)),
            _ => Some(String::new()),
        };
        assert_lifted_unchanged(&params, context.as_deref());
    }
}
