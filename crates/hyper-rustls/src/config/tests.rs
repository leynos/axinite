//! Property contracts for filtering malformed native certificate roots.
//!
//! Each generated boolean selects the same known-good certificate or a malformed
//! DER value. Repetition exercises root counts; reversing each input checks that
//! malformed entries and their order do not change acceptance.

use anyhow::Result;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseResult, TestRunner};
use rustls::pki_types::CertificateDer;

use super::native_root_store;

/// Check that the store accepts exactly sequences containing usable roots.
#[test]
fn native_roots_match_generated_certificate_sequences() -> Result<()> {
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let valid = generated.cert.der().clone();
    let strategy = (
        prop::collection::vec(any::<bool>(), 0..32),
        prop::collection::vec(any::<u8>(), 0..32),
    );
    let mut runner = TestRunner::new(Config {
        source_file: Some(file!()),
        ..Config::with_cases(128)
    });

    runner.run(&strategy, |(validity, invalid_tail)| {
        // A certificate must be a top-level DER SEQUENCE; the zero tag stays invalid.
        let malformed = std::iter::once(0u8)
            .chain(invalid_tail)
            .collect::<Vec<_>>();
        let certificates = validity
            .iter()
            .map(|is_valid| {
                if *is_valid {
                    valid.clone()
                } else {
                    CertificateDer::from(malformed.clone())
                }
            })
            .collect::<Vec<_>>();
        let usable = validity
            .iter()
            .filter(|is_valid| **is_valid)
            .count();
        assert_store_result(&certificates, usable)?;
        let reversed = certificates
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        assert_store_result(&reversed, usable)
    })?;
    Ok(())
}

/// Check acceptance and usable-root counts for either input ordering.
fn assert_store_result(certificates: &[CertificateDer<'static>], usable: usize) -> TestCaseResult {
    match native_root_store(certificates.iter().cloned()) {
        Ok(roots) => {
            prop_assert!(
                usable > 0,
                "a root store needs at least one usable certificate"
            );
            prop_assert_eq!(roots.len(), usable);
        }
        Err(error) => {
            prop_assert_eq!(usable, 0, "usable certificates must produce a root store");
            prop_assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        }
    }
    Ok(())
}
