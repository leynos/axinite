//! Tests for helper functions in the safety module.

use crate::safety::{char_boundary_truncation, update_if_changed};
use proptest::prelude::*;

proptest! {
    #[test]
    fn char_boundary_truncation_returns_valid_boundary(
        s in any::<String>(),  // Arbitrary valid UTF-8 strings
        max_len in 0usize..100usize
    ) {
        let cut = char_boundary_truncation(&s, max_len);
        // Invariant: cut must be <= max_len
        prop_assert!(cut <= max_len);
        // Invariant: cut must be at a valid UTF-8 char boundary
        prop_assert!(s.is_char_boundary(cut));
    }

    #[test]
    fn char_boundary_truncation_edge_cases(
        s in any::<String>()  // Any valid UTF-8 string
    ) {
        // Edge case: max_len == 0
        let cut = char_boundary_truncation(&s, 0);
        prop_assert_eq!(cut, 0);
        prop_assert!(s.is_char_boundary(cut));

        // Edge case: max_len > string length
        let max_len = s.len() + 10;
        let cut = char_boundary_truncation(&s, max_len);
        prop_assert!(cut <= s.len());
        prop_assert!(s.is_char_boundary(cut));
    }
}

#[test]
fn test_char_boundary_truncation_multibyte_utf8() {
    // "café": c a f é (é is 2 bytes: 0xc3 0xa9)
    // Byte positions: 0 1 2 3 4
    // char positions: 0 1 2 3
    assert_eq!(char_boundary_truncation("café", 4), 3); // Would cut into é, so back up to 3
    assert_eq!(char_boundary_truncation("café", 3), 3); // Safe cut after 'f'
    assert_eq!(char_boundary_truncation("café", 2), 2); // Safe cut after 'a'

    // "a🦀b": a 🦀 b (🦀 is 4 bytes)
    // Byte positions: 0 1 2 3 4 5
    // char positions: 0 1 2
    assert_eq!(char_boundary_truncation("a🦀b", 4), 1); // Would cut into 🦀, so back up to 1 (after 'a')
    assert_eq!(char_boundary_truncation("a🦀b", 5), 5); // Safe cut after 🦀
    assert_eq!(char_boundary_truncation("a🦀b", 1), 1); // Safe cut after 'a'
}

#[test]
fn test_update_if_changed_changed() {
    let (content, modified) = update_if_changed("old".to_string(), "new".to_string(), false);
    assert_eq!(content, "new");
    assert!(modified);
}

#[test]
fn test_update_if_changed_unchanged_was_modified_false() {
    let (content, modified) = update_if_changed("same".to_string(), "same".to_string(), false);
    assert_eq!(content, "same");
    assert!(!modified);
}

#[test]
fn test_update_if_changed_unchanged_was_modified_true() {
    let (content, modified) = update_if_changed("same".to_string(), "same".to_string(), true);
    assert_eq!(content, "same");
    assert!(modified); // Preserve prior modification flag
}
