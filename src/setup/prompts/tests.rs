use std::io;

use crossterm::event::{KeyCode, KeyModifiers};
use proptest::prelude::*;
use rstest::rstest;

use super::{SecretInputEffect, apply_secret_input_effect, apply_secret_key_event};

#[test]
fn test_header_length_calculation() {
    // Just verify it doesn't panic with various inputs
    super::print_header("Test");
    super::print_header("A longer header text");
    super::print_header("");
}

#[test]
fn test_step_indicator() {
    super::print_step(1, 3, "Test Step");
    super::print_step(3, 3, "Final Step");
}

#[test]
fn test_print_functions_do_not_panic() {
    super::print_success("operation completed");
    super::print_error("something went wrong");
    super::print_info("here is some information");
    // Also test with empty strings
    super::print_success("");
    super::print_error("");
    super::print_info("");
}

#[rstest]
#[case(
    "",
    KeyCode::Backspace,
    KeyModifiers::empty(),
    "",
    SecretInputEffect::None
)]
#[case(
    "abc",
    KeyCode::Backspace,
    KeyModifiers::empty(),
    "ab",
    SecretInputEffect::Backspace
)]
#[case(
    "abc",
    KeyCode::Char('c'),
    KeyModifiers::CONTROL,
    "abc",
    SecretInputEffect::Interrupt
)]
#[case(
    "ab",
    KeyCode::Char('c'),
    KeyModifiers::empty(),
    "abc",
    SecretInputEffect::MaskChar
)]
fn test_apply_secret_key_event(
    #[case] input: &str,
    #[case] code: KeyCode,
    #[case] modifiers: KeyModifiers,
    #[case] expected_input: &str,
    #[case] expected_effect: SecretInputEffect,
) {
    let (next_input, effect) = apply_secret_key_event(input, code, modifiers);
    assert_eq!(next_input, expected_input);
    assert_eq!(effect, expected_effect);
}

proptest! {
    #[test]
    fn prop_apply_secret_key_event_obeys_transition_invariants(
        input in proptest::collection::vec(
            prop::sample::select(vec!['a', 'b', 'c', '1', '_']),
            0..8,
        ).prop_map(|chars| chars.into_iter().collect::<String>()),
        event in prop_oneof![
            Just((KeyCode::Backspace, KeyModifiers::empty())),
            Just((KeyCode::Enter, KeyModifiers::empty())),
            Just((KeyCode::Char('c'), KeyModifiers::CONTROL)),
            prop::sample::select(vec!['a', 'b', 'c', '1', '_'])
                .prop_map(|c| (KeyCode::Char(c), KeyModifiers::empty())),
        ],
    ) {
        let (code, modifiers) = event;
        let (next_input, effect) = apply_secret_key_event(&input, code, modifiers);

        match effect {
            SecretInputEffect::Backspace => {
                let expected_len = if input.is_empty() {
                    input.len()
                } else {
                    input.len() - 1
                };
                prop_assert_eq!(next_input.len(), expected_len);
            }
            SecretInputEffect::Submit | SecretInputEffect::Interrupt => {
                prop_assert_eq!(next_input, input);
            }
            SecretInputEffect::MaskChar => {
                if let KeyCode::Char(c) = code {
                    prop_assert!(!modifiers.contains(KeyModifiers::CONTROL));
                    prop_assert_eq!(next_input.len(), input.len() + 1);
                    prop_assert_eq!(next_input, format!("{input}{c}"));
                } else {
                    prop_assert!(false, "masking requires a character input");
                }
            }
            SecretInputEffect::None => {
                prop_assert_eq!(next_input, input);
            }
        }
    }
}

#[test]
fn test_apply_secret_input_effect_emits_backspace_sequence() -> io::Result<()> {
    let mut stdout = Vec::new();
    apply_secret_input_effect(&mut stdout, &SecretInputEffect::Backspace)?;
    assert_eq!(stdout, b"\x08 \x08");
    Ok(())
}

#[test]
fn test_apply_secret_input_effect_emits_mask_character() -> io::Result<()> {
    let mut stdout = Vec::new();
    apply_secret_input_effect(&mut stdout, &SecretInputEffect::MaskChar)?;
    assert_eq!(stdout, b"*");
    Ok(())
}

#[test]
fn test_apply_secret_input_effect_sequence_snapshot() -> io::Result<()> {
    let mut stdout = Vec::new();
    apply_secret_input_effect(&mut stdout, &SecretInputEffect::MaskChar)?;
    apply_secret_input_effect(&mut stdout, &SecretInputEffect::Backspace)?;
    apply_secret_input_effect(&mut stdout, &SecretInputEffect::Submit)?;
    insta::assert_debug_snapshot!(stdout, @r###"
    [
        42,
        8,
        32,
        8,
    ]
    "###);
    Ok(())
}
