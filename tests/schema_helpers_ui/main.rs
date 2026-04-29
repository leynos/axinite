#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/schema_helpers_ui/pass/*.rs");
}
