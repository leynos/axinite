//! Compile-time regression coverage for the public DB trait surface.
//!
//! Related fixtures share one `TestCases` session so trybuild prepares its
//! generated Cargo project once for the database surface. Keep this batch
//! separate from the support fixtures so each session retains its own timeout
//! and test name. Nextest queues both sessions with `schema_helpers_ui`;
//! the non-Unix startup fixture keeps its own session as well. The Windows
//! lane runs that fixture through direct `cargo test`, so the job timeout
//! applies instead of nextest's per-test allowance. See `.config/nextest.toml`
//! for the shared lock and Linux nextest timeout policy.

/// Pays for trybuild's generated project once, before the contract sessions.
///
/// The first session in a fresh target directory compiles the whole dependency
/// tree of the generated project, which is the cold cost that the contract
/// sessions' allowances were sized around. This test builds one small fixture
/// so that cost lands here, and the sessions below start from a built tree. It
/// runs under the same features as they do, because trybuild reads them from
/// the build that produced this binary. Under nextest the leading `a_` makes
/// it sort, and so run, first: nextest lists tests alphabetically and the
/// `compile-contracts` group in `.config/nextest.toml` runs them one at a time,
/// with this test given the same allowance as the database-surface session.
/// Plain `cargo test` runs the tests in parallel and gives no ordering
/// guarantee, so on that lane whichever session takes trybuild's lock first
/// pays the cold build.
#[test]
fn a_nested_fixture_preparation() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/sandbox_mode_from_str.rs");
}

#[test]
fn db_surface_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/trybuild/db_forwarders.rs");
    cases.pass("tests/trybuild/sandbox_mode_from_str.rs");
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
