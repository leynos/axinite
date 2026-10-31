//! Property tests for schema path joining, parameter-name round-trips, and
//! validator diagnostic completeness and ordering.

use proptest::collection::{btree_map, btree_set};
use proptest::prelude::*;
use serde_json::{Map, Value};

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

/// A generated schema tree. Property names are `p0`..`p7`, so their
/// lexicographic order matches their index order and the dotted diagnostic
/// paths they produce stay unambiguous.
#[derive(Debug, Clone)]
enum Node {
    Object {
        props: Vec<(String, Prop)>,
        required: Vec<String>,
    },
}

#[derive(Debug, Clone)]
enum Prop {
    Scalar,
    Freeform,
    Object(Box<Node>),
    ArrayObject(Box<Node>),
    ArrayScalar,
    ArrayMissingItems,
}

const PROP_NAMES: usize = 8;
const REQUIRED_NAMES: usize = 10;
const MAX_DEPTH: u32 = 3;

fn prop_strategy(depth: u32) -> BoxedStrategy<Prop> {
    if depth == 0 {
        prop_oneof![
            Just(Prop::Scalar),
            Just(Prop::Freeform),
            Just(Prop::ArrayScalar),
            Just(Prop::ArrayMissingItems),
        ]
        .boxed()
    } else {
        prop_oneof![
            Just(Prop::Scalar),
            Just(Prop::Freeform),
            Just(Prop::ArrayScalar),
            Just(Prop::ArrayMissingItems),
            node_strategy(depth - 1).prop_map(|node| Prop::Object(Box::new(node))),
            node_strategy(depth - 1).prop_map(|node| Prop::ArrayObject(Box::new(node))),
        ]
        .boxed()
    }
}

fn node_strategy(depth: u32) -> BoxedStrategy<Node> {
    (
        btree_map(0..PROP_NAMES, prop_strategy(depth), 0..=4),
        btree_set(0..REQUIRED_NAMES, 0..=4),
    )
        .prop_map(|(props, required)| Node::Object {
            props: props
                .into_iter()
                .map(|(index, prop)| (format!("p{index}"), prop))
                .collect(),
            required: required
                .into_iter()
                .map(|index| format!("p{index}"))
                .collect(),
        })
        .boxed()
}

fn prop_to_json(prop: &Prop) -> Value {
    match prop {
        Prop::Scalar => serde_json::json!({ "type": "string" }),
        Prop::Freeform => serde_json::json!({ "description": "freeform" }),
        Prop::Object(node) => node_to_json(node),
        Prop::ArrayObject(node) => serde_json::json!({
            "type": "array",
            "items": node_to_json(node),
        }),
        Prop::ArrayScalar => serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
        }),
        Prop::ArrayMissingItems => serde_json::json!({ "type": "array" }),
    }
}

fn node_to_json(node: &Node) -> Value {
    let Node::Object { props, required } = node;
    let mut properties = Map::new();
    for (name, prop) in props {
        properties.insert(name.clone(), prop_to_json(prop));
    }

    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::String("object".to_owned()));
    schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".to_owned(),
            Value::Array(required.iter().cloned().map(Value::String).collect()),
        );
    }
    Value::Object(schema)
}

/// Reference model of the documented validation rules: orphan required keys at
/// each level, arrays without `items`, and recursion into object properties and
/// object-typed array items.
fn expected_diagnostics(node: &Node, path: &str, out: &mut Vec<String>) {
    let Node::Object { props, required } = node;
    for name in required {
        if !props.iter().any(|(prop_name, _)| prop_name == name) {
            out.push(format!(
                "{path}: required key \"{name}\" not found in properties"
            ));
        }
    }

    for (name, prop) in props {
        let prop_path = format!("{path}.{name}");
        match prop {
            Prop::Object(child) => expected_diagnostics(child, &prop_path, out),
            Prop::ArrayObject(child) => {
                expected_diagnostics(child, &format!("{prop_path}.items"), out);
            }
            Prop::ArrayMissingItems => {
                out.push(format!("{prop_path}: array property missing \"items\""));
            }
            Prop::Scalar | Prop::Freeform | Prop::ArrayScalar => {}
        }
    }
}

/// Diagnostic messages are always `<path>: <detail>`.
fn diagnostic_path(message: &str) -> &str {
    message
        .split_once(": ")
        .map(|(path, _)| path)
        .expect("diagnostic messages carry a path prefix")
}

/// True when `candidate` names a strict descendant of `ancestor`, using the
/// same dotted joining as `SchemaPath::child`.
fn is_proper_descendant(ancestor: &str, candidate: &str) -> bool {
    candidate
        .strip_prefix(ancestor)
        .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// For generated schema trees the validator reports exactly the
    /// violations the documented rules predict, and every diagnostic at a
    /// node appears before any diagnostic at its descendants.
    #[test]
    fn prop_validate_tool_schema_diagnostics_match_rules_and_preorder(
        node in node_strategy(MAX_DEPTH),
    ) {
        let schema = node_to_json(&node);
        let actual = validate_tool_schema(&schema, "root");

        let mut expected = Vec::new();
        expected_diagnostics(&node, "root", &mut expected);

        let mut actual_sorted = actual.clone();
        let mut expected_sorted = expected;
        actual_sorted.sort();
        expected_sorted.sort();
        prop_assert_eq!(
            &actual_sorted,
            &expected_sorted,
            "diagnostics differ for schema {}",
            schema,
        );

        let paths: Vec<&str> = actual.iter().map(|message| diagnostic_path(message)).collect();
        for (earlier_index, earlier) in paths.iter().enumerate() {
            for (later_index, later) in paths.iter().enumerate().skip(earlier_index + 1) {
                prop_assert!(
                    !is_proper_descendant(later, earlier),
                    "diagnostic at ancestor {:?} (index {}) appeared after descendant {:?} (index {}) for schema {}",
                    later,
                    later_index,
                    earlier,
                    earlier_index,
                    schema,
                );
            }
        }
    }
}
