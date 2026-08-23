//! Tests for prompt rendering helpers and secret-input behaviour in the setup prompts module.

use std::io;

use crossterm::event::{KeyCode, KeyModifiers};
use proptest::prelude::*;
use rstest::rstest;

use super::{SecretInputEffect, apply_secret_input_effect, apply_secret_key_event};

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
    insta::assert_debug_snapshot!(stdout, @"
    [
        42,
        8,
        32,
        8,
    ]
    ");
    Ok(())
}
