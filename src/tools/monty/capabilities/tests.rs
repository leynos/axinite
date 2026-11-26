//! Capability-selection tests, including properties over the proved kernel.
//!
//! The kernel's exactness is proved by Verus; these properties check that the
//! shipped table construction uses it faithfully, and that a deliberately
//! wrong projection is detectable.

use std::sync::Arc;

use monty_capabilities::{MAX_SELECTED_TOOLS, admits, mask_from_indices, project};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::tools::monty::capabilities::{SelectedTools, SelectionError, ToolEntry};
use crate::tools::{ApprovalRequirement, NativeTool, ToolError, ToolOutput, ToolRegistry};

struct Stub {
    name: &'static str,
    schema: Value,
}

impl NativeTool for Stub {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "stub"
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(
        &self,
        _params: Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, ToolError> {
        unreachable!("stubs are never executed in capability tests")
    }

    fn requires_approval(&self, _params: &Value) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }
}

fn entry(name: &'static str, schema: Value) -> ToolEntry {
    ToolEntry::new(Arc::new(Stub { name, schema }))
}

fn object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"query": {"type": "string"}},
        "required": ["query"],
        "additionalProperties": false
    })
}

/// A deliberately wrong projection that omits the caller's request.
fn project_omitting_request(available: u64, validated: u64) -> u64 {
    available & validated
}

/// A deliberately wrong projection that always selects nothing.
fn project_always_empty(_available: u64, _requested: u64, _validated: u64) -> u64 {
    0
}

fn with_bit(value: u64, index: usize) -> u64 {
    value | (1_u64 << index)
}

#[test]
fn table_is_sorted_and_admits_every_entry() {
    let table = SelectedTools::from_entries(vec![
        entry("zeta", object_schema()),
        entry("alpha", object_schema()),
    ])
    .expect("table");
    let names: Vec<_> = table.entries().iter().map(ToolEntry::name).collect();
    assert_eq!(names, ["alpha", "zeta"]);
    assert!(table.admits(0));
    assert!(table.admits(1));
    assert!(!table.admits(2));
    assert_eq!(table.index_of("zeta"), Some(1));
}

#[test]
fn duplicate_names_are_refused() {
    let error = SelectedTools::from_entries(vec![
        entry("alpha", object_schema()),
        entry("alpha", object_schema()),
    ])
    .expect_err("duplicate");
    assert!(matches!(error, SelectionError::Duplicate { .. }));
}

#[test]
fn reserved_names_are_refused() {
    for name in ["params", "main", "__axinite_host__", "exec_code"] {
        let error =
            SelectedTools::from_entries(vec![entry(name, object_schema())]).expect_err("reserved");
        assert!(matches!(error, SelectionError::InvalidName { .. }));
    }
}

#[test]
fn non_identifier_names_are_refused() {
    let error = SelectedTools::from_entries(vec![entry("file.read", object_schema())])
        .expect_err("not an identifier");
    assert!(matches!(error, SelectionError::InvalidName { .. }));
}

#[test]
fn unsupported_schema_is_refused() {
    let schema = json!({
        "type": "object",
        "properties": {"value": {"oneOf": [{"type": "string"}, {"type": "number"}]}}
    });
    let error = SelectedTools::from_entries(vec![entry("lookup", schema)]).expect_err("schema");
    assert!(matches!(error, SelectionError::UnsupportedSchema { .. }));
}

#[test]
fn too_many_entries_are_refused() {
    let entries = (0..=MAX_SELECTED_TOOLS)
        .map(|index| {
            entry(
                Box::leak(format!("tool_{index}").into_boxed_str()),
                object_schema(),
            )
        })
        .collect();
    let error = SelectedTools::from_entries(entries).expect_err("too many");
    assert!(matches!(
        error,
        SelectionError::TooMany {
            count,
            max: MAX_SELECTED_TOOLS
        } if count == MAX_SELECTED_TOOLS + 1
    ));
}

#[test]
fn definitions_follow_table_order() {
    let table = SelectedTools::from_entries(vec![
        entry("zeta", object_schema()),
        entry("alpha", object_schema()),
    ])
    .expect("table");
    let names: Vec<_> = table
        .definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect();
    assert_eq!(names, ["alpha", "zeta"]);
}

/// Exhaustively classifies every subset of five names.
///
/// The class counts make the generator's non-vacuity measurable: empty,
/// singleton and multi-entry tables all occur, and each table's view is the
/// exact intersection of its requested and validated masks.
#[test]
fn generated_cases_cover_every_class() {
    const NAMES: [&str; 5] = ["alpha", "beta", "gamma", "delta", "epsilon"];
    let mut counts = [0_usize; 3];

    for mask in 0_u16..(1 << NAMES.len()) {
        let names: Vec<&'static str> = NAMES
            .iter()
            .enumerate()
            .filter(|(index, _)| mask >> index & 1 == 1)
            .map(|(_, name)| *name)
            .collect();
        let entries = names
            .iter()
            .map(|name| entry(name, object_schema()))
            .collect();
        let table = SelectedTools::from_entries(entries).expect("table");
        counts[match table.len() {
            0 => 0,
            1 => 1,
            _ => 2,
        }] += 1;

        assert_eq!(table.view(), mask_from_indices(0..table.len()));
        for (index, _) in table.entries().iter().enumerate() {
            assert!(admits(table.view(), index as u64));
        }
        // `from_entries` sorts by name so an index is stable for a given set,
        // whatever order the caller requested the tools in.
        let mut expected = names.clone();
        expected.sort_unstable();
        assert_eq!(
            table
                .entries()
                .iter()
                .map(ToolEntry::name)
                .collect::<Vec<_>>(),
            expected
        );
    }

    assert_eq!(counts[0], 1, "one empty table");
    assert_eq!(counts[1], NAMES.len(), "one table per singleton");
    assert!(counts[2] > 0, "multi-entry tables are generated");
}

#[tokio::test]
async fn unknown_names_are_refused_before_selection() {
    let registry = ToolRegistry::new();
    let error = SelectedTools::resolve(&registry, &["missing".to_string()])
        .await
        .expect_err("unknown");
    assert!(matches!(error, SelectionError::Unknown { name } if name == "missing"));
}

proptest! {
    #[test]
    fn projection_matches_mathematical_intersection(
        available in any::<u64>(),
        requested in any::<u64>(),
        validated in any::<u64>(),
    ) {
        prop_assert_eq!(
            project(available, requested, validated),
            available & requested & validated
        );
    }

    #[test]
    fn projection_cannot_widen_any_input(
        available in any::<u64>(),
        requested in any::<u64>(),
        validated in any::<u64>(),
    ) {
        let view = project(available, requested, validated);
        prop_assert_eq!(view & !available, 0);
        prop_assert_eq!(view & !requested, 0);
        prop_assert_eq!(view & !validated, 0);
    }

    #[test]
    fn omitted_request_bit_is_removed_and_others_survive(
        index in 0_usize..MAX_SELECTED_TOOLS,
        available in any::<u64>(),
        requested in any::<u64>(),
        validated in any::<u64>(),
    ) {
        let available = with_bit(available, index);
        let requested = with_bit(requested, index);
        let validated = with_bit(validated, index);
        let full = project(available, requested, validated);
        let omitted = project(available, requested & !(1_u64 << index), validated);

        prop_assert!(admits(full, index as u64));
        prop_assert!(!admits(omitted, index as u64));
        prop_assert_eq!(omitted, full & !(1_u64 << index));
    }

    #[test]
    fn seeded_omitted_set_mutation_is_rejected(
        index in 0_usize..MAX_SELECTED_TOOLS,
        available in any::<u64>(),
        requested in any::<u64>(),
        validated in any::<u64>(),
    ) {
        let available = with_bit(available, index);
        let requested = with_bit(requested, index);
        let validated = with_bit(validated, index);
        let omitted = requested & !(1_u64 << index);

        prop_assert_ne!(
            project(available, omitted, validated),
            project_omitting_request(available, validated),
            "dropping the request mask must be observable"
        );
    }

    #[test]
    fn seeded_empty_output_mutation_is_rejected(
        index in 0_usize..MAX_SELECTED_TOOLS,
        available in any::<u64>(),
        requested in any::<u64>(),
        validated in any::<u64>(),
    ) {
        let available = with_bit(available, index);
        let requested = with_bit(requested, index);
        let validated = with_bit(validated, index);

        prop_assert_ne!(project(available, requested, validated), 0);
        prop_assert_ne!(
            project(available, requested, validated),
            project_always_empty(available, requested, validated)
        );
    }
}
