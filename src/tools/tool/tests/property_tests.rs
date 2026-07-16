//! Property tests for schema path joining and parameter-name round-trips.

use proptest::prelude::*;

use super::super::*;

proptest! {
    /// SchemaPath::child always produces "<parent>.<segment>".
    #[test]
    fn prop_schema_path_child_dot_joins(
        root in "[a-z][a-z0-9_]{0,15}",
        seg  in "[a-z][a-z0-9_]{0,15}",
    ) {
        let path = SchemaPath::from(root.as_str()).child(&seg);
        let expected = format!("{root}.{seg}");
        prop_assert_eq!(path.as_str(), expected.as_str());
    }

    /// ParamName round-trips through as_ref and to_string.
    #[test]
    fn prop_param_name_round_trips(s in "[a-z][a-z0-9_]{0,31}") {
        let name = ParamName::from(s.as_str());
        prop_assert_eq!(name.as_ref(), s.as_str());
        prop_assert_eq!(name.to_string(), s);
    }

    /// ToolName round-trips through as_ref and to_string.
    #[test]
    fn prop_tool_name_round_trips(s in "[a-z][a-z0-9_]{0,31}") {
        let name = ToolName::from(s.as_str());
        prop_assert_eq!(name.as_ref(), s.as_str());
        prop_assert_eq!(name.to_string(), s);
    }

    /// validate_tool_schema on a minimal valid schema never panics
    /// and returns no errors regardless of the path string used.
    #[test]
    fn prop_validate_tool_schema_minimal_valid_never_panics(
        path in "[a-z][a-z0-9_.]{0,31}",
    ) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {}
        });
        let errors = validate_tool_schema(&schema, path.as_str());
        prop_assert!(errors.is_empty());
    }
}
