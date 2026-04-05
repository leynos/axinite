//! Edge-case and regression tests for `cosine_similarity` and related search
//! cosine helpers.

use super::*;

#[test]
fn test_cosine_similarity_identical() {
    let a = [1.0, 2.0, 3.0];
    let b = [1.0, 2.0, 3.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - 1.0).abs() < 0.001);
}

#[test]
fn test_cosine_similarity_opposite() {
    let a = [1.0, 2.0, 3.0];
    let b = [-1.0, -2.0, -3.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - (-1.0)).abs() < 0.001);
}

#[test]
fn test_cosine_similarity_orthogonal() {
    let a = [1.0, 0.0];
    let b = [0.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - 0.0).abs() < 0.001);
}

#[test]
fn test_cosine_similarity_different_lengths() {
    let a = [1.0, 2.0, 3.0];
    let b = [1.0, 2.0];
    let sim = cosine_similarity(&a, &b);
    assert_eq!(sim, 0.0);
}

#[test]
fn test_cosine_similarity_zero_vector() {
    let a = [0.0, 0.0, 0.0];
    let b = [1.0, 2.0, 3.0];
    let sim = cosine_similarity(&a, &b);
    assert_eq!(sim, 0.0);
}

#[test]
fn test_cosine_similarity_both_zero() {
    let a = [0.0, 0.0];
    let b = [0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert_eq!(sim, 0.0);
}

/// Regression: `cosine_similarity` previously contained a `debug_assert_eq!`
/// on vector lengths that panicked in debug/test builds before the graceful
/// `return 0.0` path could execute. The fix replaced the assertion with
/// `tracing::warn!`. This test verifies the function does not panic and
/// returns the expected fallback value.
#[test]
fn test_cosine_similarity_different_lengths_does_not_panic() {
    let result = std::panic::catch_unwind(|| cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0]));
    assert!(
        result.is_ok(),
        "cosine_similarity must not panic on length-mismatched vectors"
    );
    assert_eq!(
        result.expect("already asserted Ok"),
        0.0,
        "cosine_similarity must return 0.0 for length-mismatched vectors"
    );
}

/// Regression: `cosine_similarity` could return `NaN` when both vectors
/// contained infinity values, producing `inf / inf`. The fix added a NaN
/// guard that maps the result to 0.0.
#[test]
fn test_cosine_similarity_special_values_do_not_return_nan() {
    let cases: Vec<(&[f32], &[f32])> = vec![
        (&[f32::INFINITY, 0.0], &[f32::INFINITY, 0.0]),
        (&[f32::NEG_INFINITY, 1.0], &[f32::NEG_INFINITY, 1.0]),
        (&[f32::INFINITY, f32::NEG_INFINITY], &[1.0, 1.0]),
        (&[f32::NAN, 1.0], &[1.0, 1.0]),
    ];

    for (a, b) in cases {
        let sim = cosine_similarity(a, b);
        assert_eq!(
            sim, 0.0,
            "cosine_similarity should return 0.0 fallback for special values, got {sim} for a={a:?}, b={b:?}"
        );
        assert!(
            !sim.is_nan(),
            "cosine_similarity returned NaN for a={a:?}, b={b:?}"
        );
    }
}
