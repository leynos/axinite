//! Compile-time regression coverage for the public DB trait surface.
//!
//! Each trybuild case spawns a fresh `rustc` against the full crate, so the
//! wall-clock cost is high (~7 min locally). The default nextest profile
//! excludes this binary; the `ci` profile includes it. See
//! `.config/nextest.toml`.

use rstest::rstest;

#[rstest]
#[case("tests/trybuild/db_forwarders.rs")]
#[case("tests/trybuild/db_forwarders_postgres.rs")]
#[case("tests/trybuild/db_forwarders_libsql.rs")]
#[case("tests/trybuild/settings_compat.rs")]
fn db_surface_compile_contracts(#[case] fixture: &str) {
    let cases = trybuild::TestCases::new();
    cases.pass(fixture);
}
