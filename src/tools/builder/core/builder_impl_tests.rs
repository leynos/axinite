//! Tests for the builder's response-scanning helpers.
//!
//! `extract_first_json_object` tolerates prose around the payload and keeps
//! scanning after a candidate fails to parse, so a malformed first brace must
//! not hide a later valid object.

use super::extract_first_json_object;

/// A candidate that fails to parse must not end the scan.
#[test]
fn skips_a_malformed_candidate_and_returns_the_next_object() {
    let response = r#"Here you go: {"broken": } and then {"name":"ok"}"#;

    let found = extract_first_json_object(response);

    assert_eq!(found, Some(r#"{"name":"ok"}"#));
}

/// When no candidate parses, the scan reports no object.
#[test]
fn returns_none_when_no_candidate_parses() {
    let response = r#"{"broken": } then {"also_broken": }"#;

    let found = extract_first_json_object(response);

    assert_eq!(found, None);
}

/// A well-formed object embedded in prose is returned unchanged.
#[test]
fn returns_an_object_wrapped_in_prose() {
    let response = "Result follows\n{\"name\":\"ok\",\"count\":2}\nthat is all";

    let found = extract_first_json_object(response);

    assert_eq!(found, Some("{\"name\":\"ok\",\"count\":2}"));
}
