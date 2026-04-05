//! Edge-case and regression tests for `cosine_similarity` and related search
//! cosine helpers.

use rstest::rstest;

use super::*;

#[rstest]
#[case::identical([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], 1.0, true)]
#[case::opposite([1.0, 2.0, 3.0], [-1.0, -2.0, -3.0], -1.0, true)]
#[case::orthogonal([1.0, 0.0], [0.0, 1.0], 0.0, true)]
#[case::different_lengths([1.0, 2.0, 3.0], [1.0, 2.0], 0.0, false)]
#[case::zero_vector([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], 0.0, false)]
#[case::both_zero([0.0, 0.0], [0.0, 0.0], 0.0, false)]
fn test_cosine_similarity_cases(
    #[case] a: impl AsRef<[f32]>,
    #[case] b: impl AsRef<[f32]>,
    #[case] expected: f32,
    #[case] use_approx: bool,
) {
    let sim = cosine_similarity(a.as_ref(), b.as_ref());
    if use_approx {
        assert!(
            (sim - expected).abs() < 0.001,
            "Expected ~{}, got {}",
            expected,
            sim
        );
    } else {
        assert_eq!(sim, expected, "Expected exact {}, got {}", expected, sim);
    }
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
/// guard that maps the result to 0.0. This test uses a Cartesian sweep
/// over problematic special scalars to ensure comprehensive coverage.
#[test]
fn test_cosine_similarity_special_values_do_not_return_nan() {
    // Only test truly problematic special values that should produce NaN or undefined results
    let special_scalars = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];

    for &a_val in &special_scalars {
        for &b_val in &special_scalars {
            let a = [a_val, 1.0];
            let b = [b_val, 1.0];
            let sim = cosine_similarity(&a, &b);
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
}
