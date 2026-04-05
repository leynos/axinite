//! Compile-time regression coverage for the public DB trait surface.
//!
//! Each trybuild case spawns a fresh `rustc` against the full crate, so the
//! wall-clock cost is high (~7 min locally). The default nextest profile
//! excludes this binary; the `ci` profile includes it. See
//! `.config/nextest.toml`.

#[test]
fn db_surface_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders.rs");
    cases.pass("tests/trybuild/settings_compat.rs");
}

#[test]
fn db_surface_compile_contracts_postgres() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders_postgres.rs");
}

#[test]
fn db_surface_compile_contracts_libsql() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders_libsql.rs");
}
