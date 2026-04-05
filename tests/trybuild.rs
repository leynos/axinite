//! Compile-time regression coverage for the public DB trait surface.

#[test]
fn db_surface_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders.rs");
    cases.pass("tests/trybuild/settings_compat.rs");
}
