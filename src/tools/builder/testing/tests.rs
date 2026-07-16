//! Unit tests for builder testing helpers such as JSON path lookup.

use super::*;

#[test]
fn test_get_json_path() {
    let json = serde_json::json!({
        "foo": {
            "bar": [1, 2, 3],
            "baz": "hello"
        }
    });

    assert_eq!(
        get_json_path(&json, "foo.baz"),
        Some(&serde_json::json!("hello"))
    );
    assert_eq!(
        get_json_path(&json, "foo.bar[0]"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        get_json_path(&json, "foo.bar[2]"),
        Some(&serde_json::json!(3))
    );
    assert_eq!(get_json_path(&json, "foo.missing"), None);
}

#[test]
fn test_test_suite_builder() {
    let mut suite = TestSuite::new("my_tests");
    suite
        .add_io_test(
            "basic",
            serde_json::json!({"x": 1}),
            serde_json::json!({"y": 2}),
        )
        .add_error_test("invalid", serde_json::json!({}), "required");

    assert_eq!(suite.tests.len(), 2);
    assert!(!suite.tests[0].expect_error);
    assert!(suite.tests[1].expect_error);
}

#[test]
fn test_generate_basic_tests() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"}
        },
        "required": ["name"]
    });

    let suite = generate_basic_tests("my_tool", &schema);
    assert!(!suite.tests.is_empty());
}
