//! Tests for the SSE listener's chunk decoding and event accumulation state.

use crate::channels::signal::listener::{SseState, utf8_split_points};

use super::{MAX_SSE_BUFFER_SIZE, MAX_SSE_EVENT_SIZE};

#[test]
fn split_points_cover_complete_text() {
    let bytes = "héllo".as_bytes();
    assert_eq!(utf8_split_points(bytes), (bytes.len(), bytes.len()));
}

#[test]
fn split_points_carry_an_incomplete_trailing_sequence() {
    // "é" is two bytes; cut it after the first.
    let mut bytes = b"ab".to_vec();
    bytes.push("é".as_bytes()[0]);
    assert_eq!(utf8_split_points(&bytes), (2, 2));
}

#[test]
fn split_points_skip_an_invalid_byte() {
    // 0xFF is never valid UTF-8; the valid prefix ends before it and the
    // carry starts after it.
    assert_eq!(utf8_split_points(b"ab\xFFcd"), (2, 3));
}

#[test]
fn ingest_decodes_a_multibyte_character_split_across_chunks() {
    let bytes = "é".as_bytes();
    let mut state = SseState::new();

    assert!(state.ingest(&bytes[..1]));
    assert_eq!(state.buffer, "");
    assert_eq!(state.utf8_carry, &bytes[..1]);

    assert!(state.ingest(&bytes[1..]));
    assert_eq!(state.buffer, "é");
    assert!(state.utf8_carry.is_empty());
}

#[test]
fn ingest_resets_everything_when_the_buffer_would_overflow() {
    let mut state = SseState::new();
    state.current_data.push_str("partial event");
    state.buffer = "x".repeat(MAX_SSE_BUFFER_SIZE);

    assert!(!state.ingest(b"y"));

    assert!(state.buffer.is_empty());
    assert!(state.current_data.is_empty());
    assert!(state.utf8_carry.is_empty());
}

#[test]
fn push_data_line_joins_lines_with_newlines_and_trims_leading_space() {
    let mut state = SseState::new();
    state.push_data_line(" first");
    state.push_data_line("second");
    assert_eq!(state.current_data, "first\nsecond");
}

#[test]
fn push_data_line_drops_an_oversized_event() {
    let mut state = SseState::new();
    state.push_data_line("small");

    state.push_data_line(&"z".repeat(MAX_SSE_EVENT_SIZE));

    assert!(state.current_data.is_empty());
}
