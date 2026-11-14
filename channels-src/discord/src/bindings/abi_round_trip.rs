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
//! Two guests are exercised. `DiscordChannel` is the shipped implementation,
//! and its round trips check the logic the host will actually reach. The
//! import-free [`RecordingChannel`] returns what it received instead: the
//! shipped channel refuses every broadcast with a constant, so its result
//! cannot show whether `user_id`, `content` or `thread_id` survived the ABI.
//! The recording guest echoes them, including every attachment's filename,
//! MIME type and bytes, and records what `on_status` lifted.
//!
//! The harness is split across the 32-bit test modules declared beside this
//! one: `abi_round_trip_layout` lowers arguments and decodes return areas,
//! `abi_round_trip_guest` holds the recording guest, and
//! `abi_round_trip_support` drives a whole case and generates its arguments.
//! This module holds the test cases themselves.
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

use super::abi_round_trip_guest::{record_status, status_recordings_with, RecordingChannel};
use super::abi_round_trip_layout::AttachmentFixture;
use super::abi_round_trip_support::{
    assert_matches_the_guest, broadcast_as, Cases, GENERATED_LENGTHS,
};
use crate::DiscordChannel;

#[test]
fn a_broadcast_reports_that_it_is_not_implemented() {
    let error = broadcast_as::<DiscordChannel>("user", "hello", None, &[])
        .expect_err("Discord cannot broadcast");
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

#[test]
fn strings_of_every_length_class_cross_the_abi() {
    let units = ["a", "é", "€", "🦀"];
    // Lengths either side of the allocator's small and medium size classes.
    for unit in units {
        for repeat in [0usize, 1, 2, 7, 8, 9, 15, 16, 17, 255, 256, 1000] {
            let text = unit.repeat(repeat);
            assert_matches_the_guest::<DiscordChannel>(&text, &text, Some(&text), &[]);
            assert_matches_the_guest::<DiscordChannel>(&text, &text, None, &[]);
        }
    }
}

#[test]
fn every_thread_state_crosses_the_abi() {
    let long = "é".repeat(1000);
    let states = [None, Some(""), Some("t"), Some("🦀"), Some(long.as_str())];
    for thread_id in states {
        assert_matches_the_guest::<DiscordChannel>("user", "content", thread_id, &[]);
        assert_matches_the_guest::<DiscordChannel>("", "", thread_id, &[]);
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
        assert_matches_the_guest::<RecordingChannel>(
            &user_id,
            &content,
            thread_id.as_deref(),
            &attachments,
        );
    }
}
