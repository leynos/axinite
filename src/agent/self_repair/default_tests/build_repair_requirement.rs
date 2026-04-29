//! Tests for repair build requirement construction.

use rstest::rstest;

use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::error::RepairError;
use crate::tools::{Language, SoftwareType};

use super::helpers::stub_broken_tool;

// === build_repair_requirement ===

#[rstest]
#[case("my-tool", None, 3, true, Some("Unknown error"))]
#[case("my-tool", Some("segfault"), 3, true, Some("segfault"))]
#[case("my-tool", None, 7, true, Some("7"))]
#[case("", None, 3, false, None)]
#[case("bad name", None, 3, false, None)]
fn build_repair_requirement_reflects_tool_state(
    #[case] name: &str,
    #[case] last_error: Option<&str>,
    #[case] failure_count: usize,
    #[case] expect_ok: bool,
    #[case] expected_substring: Option<&str>,
) {
    let mut tool = stub_broken_tool(name, last_error, 0);
    tool.failure_count = failure_count as u32;

    let result = DefaultSelfRepair::build_repair_requirement(&tool);

    if expect_ok {
        let req = result.expect("valid name should succeed");
        assert_eq!(req.name.as_str(), name);
        assert_eq!(req.software_type, SoftwareType::WasmTool);
        assert_eq!(req.language, Language::Rust);
        assert!(req.capabilities.contains(&"http".to_string()));
        assert!(req.capabilities.contains(&"workspace".to_string()));
        assert!(req.dependencies.is_empty());

        if let Some(expected_substring) = expected_substring {
            assert!(
                req.description.contains(expected_substring),
                "description should contain {expected_substring:?}",
            );
        }

        if name == "my-tool" && last_error.is_none() && failure_count == 3 {
            assert_eq!(
                req.description,
                concat!(
                    "Repair broken WASM tool.\n\n",
                    "Tool name: my-tool\n",
                    "Previous error: Unknown error\n",
                    "Failure count: 3\n\n",
                    "Analyze the error, fix the implementation, and rebuild."
                )
            );
        }
    } else {
        let err = result.expect_err("invalid name should be rejected");
        assert!(
            matches!(err, RepairError::Failed { .. }),
            "expected RepairError::Failed, got: {err:?}",
        );
    }
}
