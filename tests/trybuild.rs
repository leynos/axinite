//! Compile-time regression coverage for the public DB trait surface.
//!
//! Related fixtures share one `TestCases` session so trybuild prepares its
//! generated Cargo project once for the database surface. Keep this batch
//! separate from the support fixtures so each session retains its own timeout
//! and test name. Nextest queues both sessions with `schema_helpers_ui`;
//! the non-Unix startup fixture keeps its own session and timeout as well.
//! See `.config/nextest.toml` for the shared lock and timeout policy.

#[test]
fn db_surface_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders.rs");
    #[cfg(feature = "postgres")]
    cases.pass("tests/trybuild/db_forwarders_postgres.rs");
    #[cfg(feature = "libsql")]
    cases.pass("tests/trybuild/db_forwarders_libsql.rs");
    cases.pass("tests/trybuild/settings_compat.rs");
}

#[cfg(not(unix))]
#[test]
fn startup_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/startup_run_non_unix.rs");
}

#[test]
fn harness_compile_contracts() {
    let cases = trybuild::TestCases::new();
    #[cfg(feature = "libsql")]
    cases.pass("tests/trybuild/e2e_traces.rs");
    cases.pass("tests/trybuild/infrastructure.rs");
    cases.pass("tests/trybuild/support_unit.rs");
}
