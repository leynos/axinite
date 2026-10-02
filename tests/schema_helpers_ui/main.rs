//! Compile-time contracts for the schema helper's accepted call-site types.
//!
//! This session shares trybuild's generated project with `tests/trybuild.rs`,
//! so `.config/nextest.toml` schedules both binaries in one group.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/schema_helpers_ui/pass/*.rs");
}
