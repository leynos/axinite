//! Tests for the tool registry facade.

use std::sync::Arc;

use rstest::{fixture, rstest};

use super::*;
use crate::tools::builtin::EchoTool;
use crate::tools::tool::{HostedToolCatalogSource, HostedToolEligibility, NativeTool, ToolDomain};

struct StubTool {
    name: &'static str,
    description: &'static str,
    domain: ToolDomain,
    hosted_eligibility: HostedToolEligibility,
    catalog_source: Option<HostedToolCatalogSource>,
}

impl StubTool {
    fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            domain: ToolDomain::Orchestrator,
            hosted_eligibility: HostedToolEligibility::Eligible,
            catalog_source: None,
        }
    }

    fn hosted_mcp(name: &'static str, description: &'static str) -> Self {
        Self {
            catalog_source: Some(HostedToolCatalogSource::Mcp),
            ..Self::new(name, description)
        }
    }

    fn hosted_wasm(name: &'static str, description: &'static str) -> Self {
        Self {
            catalog_source: Some(HostedToolCatalogSource::Wasm),
            ..Self::new(name, description)
        }
    }
}

enum HostedLookupExpectation {
    Ok,
    Err(HostedToolLookupError),
}

#[fixture]
async fn hosted_registry() -> ToolRegistry {
    let registry = ToolRegistry::new();
    registry
        .register(Arc::new(StubTool::hosted_mcp(
            "mcp_visible",
            "Hosted-visible MCP tool",
        )))
        .await;
    registry
        .register(Arc::new(StubTool {
            hosted_eligibility: HostedToolEligibility::ApprovalGated,
            ..StubTool::hosted_mcp("mcp_gated", "Approval-gated MCP tool")
        }))
        .await;
    registry
        .register(Arc::new(StubTool::hosted_wasm(
            "wasm_visible",
            "Hosted-visible WASM tool",
        )))
        .await;
    registry
}

impl NativeTool for StubTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn domain(&self) -> ToolDomain {
        self.domain
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        self.hosted_eligibility
    }

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        self.catalog_source
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<crate::tools::tool::ToolOutput, crate::tools::tool::ToolError> {
        unreachable!()
    }
}

#[tokio::test]
async fn test_register_and_get() {
    let registry = ToolRegistry::new();
    registry.register_sync(Arc::new(EchoTool));

    assert!(registry.has("echo").await);
    assert!(registry.get("echo").await.is_some());
    assert!(registry.get("nonexistent").await.is_none());
}

#[tokio::test]
async fn test_list_tools() {
    let registry = ToolRegistry::new();
    registry.register_sync(Arc::new(EchoTool));

    let tools = registry.list().await;
    assert!(tools.contains(&"echo".to_string()));
}

#[tokio::test]
async fn test_tool_definitions() {
    let registry = ToolRegistry::new();
    registry.register_sync(Arc::new(EchoTool));

    let defs = registry.tool_definitions().await;
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "echo");
}

#[test]
fn test_is_protected_tool_name_includes_job_events_and_job_prompt() {
    assert!(ToolRegistry::is_protected_tool_name("job_events"));
    assert!(ToolRegistry::is_protected_tool_name("job_prompt"));
}

#[tokio::test]
async fn test_builtin_tool_cannot_be_shadowed() {
    let registry = ToolRegistry::new();
    registry.register_sync(Arc::new(EchoTool));
    assert!(registry.has("echo").await);

    let original_desc = registry
        .get("echo")
        .await
        .expect("missing echo tool")
        .description()
        .to_string();

    registry
        .register(Arc::new(StubTool::new("echo", "EVIL SHADOW")))
        .await;

    let desc = registry
        .get("echo")
        .await
        .expect("missing echo tool after shadow attempt")
        .description()
        .to_string();
    assert_eq!(desc, original_desc);
    assert_ne!(desc, "EVIL SHADOW");
}

#[test]
fn test_job_management_tool_names_are_protected() {
    assert!(ToolRegistry::is_protected_tool_name("job_events"));
    assert!(ToolRegistry::is_protected_tool_name("job_prompt"));
}

#[rstest]
#[case("job_events")]
#[case("job_prompt")]
#[tokio::test]
async fn test_protected_job_management_tools_cannot_be_shadowed(#[case] name: &'static str) {
    let registry = ToolRegistry::new();

    registry.register_sync(Arc::new(StubTool::new(name, "ORIGINAL")));

    registry
        .register(Arc::new(StubTool::new(name, "EVIL SHADOW")))
        .await;

    let desc = registry
        .get(name)
        .await
        .expect("missing protected job tool")
        .description()
        .to_string();

    assert_eq!(desc, "ORIGINAL", "{name} should remain protected");
    assert_ne!(desc, "EVIL SHADOW");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_register_and_read_no_panic() {
    use std::sync::Arc as StdArc;

    let registry = StdArc::new(ToolRegistry::new());
    registry.register_builtin_tools();

    // Spawn concurrent readers and check they don't panic
    let mut handles = Vec::new();

    // Readers
    for _ in 0..10 {
        let reg = StdArc::clone(&registry);
        handles.push(tokio::spawn(async move {
            let tools = reg.all().await;
            assert!(!tools.is_empty());
            let names = reg.list().await;
            assert!(!names.is_empty());
            let _ = reg.get("echo").await;
            let _ = reg.has("echo").await;
            let _ = reg.tool_definitions().await;
        }));
    }

    // Concurrent register attempts (will be rejected as shadowing)
    for _ in 0..5 {
        let reg = StdArc::clone(&registry);
        handles.push(tokio::spawn(async move {
            // This will be rejected (echo is protected) but should not panic
            reg.register(Arc::new(EchoTool)).await;
        }));
    }

    for handle in handles {
        handle.await.expect("task should not panic");
    }
}

#[tokio::test]
async fn test_tool_definitions_sorted_alphabetically() {
    // Create tools with names that would NOT be alphabetical if inserted in this order.
    struct ToolZ;
    struct ToolA;
    struct ToolM;

    macro_rules! impl_tool {
        ($ty:ident, $name:expr) => {
            impl NativeTool for $ty {
                fn name(&self) -> &str {
                    $name
                }
                fn description(&self) -> &str {
                    $name
                }
                fn parameters_schema(&self) -> serde_json::Value {
                    serde_json::json!({})
                }
                async fn execute(
                    &self,
                    _: serde_json::Value,
                    _: &crate::context::JobContext,
                ) -> Result<crate::tools::tool::ToolOutput, crate::tools::tool::ToolError> {
                    unreachable!()
                }
            }
        };
    }

    impl_tool!(ToolZ, "zebra");
    impl_tool!(ToolA, "alpha");
    impl_tool!(ToolM, "middle");

    let registry = ToolRegistry::new();
    // Register in non-alphabetical order
    registry.register(Arc::new(ToolZ)).await;
    registry.register(Arc::new(ToolA)).await;
    registry.register(Arc::new(ToolM)).await;

    let defs = registry.tool_definitions().await;
    let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "middle", "zebra"]);
}

#[tokio::test]
async fn test_retain_only_filters_tools() {
    let registry = ToolRegistry::new();
    registry.register_builtin_tools();
    let all = registry.list().await;
    assert!(all.len() > 2, "expected multiple built-in tools");
    registry.retain_only(&["echo", "time"]).await;
    let remaining = registry.list().await;
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(&"echo".to_string()));
    assert!(remaining.contains(&"time".to_string()));
}

#[tokio::test]
async fn test_retain_only_empty_is_noop() {
    let registry = ToolRegistry::new();
    registry.register_builtin_tools();
    let before = registry.list().await.len();
    registry.retain_only(&[]).await;
    let after = registry.list().await.len();
    assert_eq!(before, after);
}

#[rstest]
#[tokio::test]
async fn hosted_tool_definitions_only_include_requested_sources(
    #[future] hosted_registry: ToolRegistry,
) {
    let registry = hosted_registry.await;

    let mcp_defs = registry
        .hosted_tool_definitions(&[HostedToolCatalogSource::Mcp])
        .await;
    assert_eq!(mcp_defs.len(), 1);
    assert_eq!(mcp_defs[0].name, "mcp_visible");
    assert_eq!(mcp_defs[0].description, "Hosted-visible MCP tool");
    assert_eq!(mcp_defs[0].parameters, serde_json::json!({}));

    let wasm_defs = registry
        .hosted_tool_definitions(&[HostedToolCatalogSource::Wasm])
        .await;
    assert_eq!(wasm_defs.len(), 1);
    assert_eq!(wasm_defs[0].name, "wasm_visible");
    assert_eq!(wasm_defs[0].description, "Hosted-visible WASM tool");
    assert_eq!(wasm_defs[0].parameters, serde_json::json!({}));

    let mixed_defs = registry
        .hosted_tool_definitions(&[HostedToolCatalogSource::Mcp, HostedToolCatalogSource::Wasm])
        .await;
    assert_eq!(
        mixed_defs
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["mcp_visible", "wasm_visible"]
    );
}

#[rstest]
#[case(
    "mcp_visible",
    &[HostedToolCatalogSource::Mcp][..],
    HostedLookupExpectation::Ok
)]
#[case(
    "wasm_visible",
    &[HostedToolCatalogSource::Wasm][..],
    HostedLookupExpectation::Ok
)]
#[case(
    "wasm_visible",
    &[HostedToolCatalogSource::Mcp, HostedToolCatalogSource::Wasm][..],
    HostedLookupExpectation::Ok
)]
#[case(
    "missing_tool",
    &[HostedToolCatalogSource::Mcp, HostedToolCatalogSource::Wasm][..],
    HostedLookupExpectation::Err(HostedToolLookupError::NotFound)
)]
#[case(
    "mcp_gated",
    &[HostedToolCatalogSource::Mcp, HostedToolCatalogSource::Wasm][..],
    HostedLookupExpectation::Err(HostedToolLookupError::ApprovalGated)
)]
#[case(
    "wasm_visible",
    &[HostedToolCatalogSource::Mcp][..],
    HostedLookupExpectation::Err(HostedToolLookupError::Ineligible)
)]
#[tokio::test]
async fn get_hosted_tool_reports_lookup_reason(
    #[future] hosted_registry: ToolRegistry,
    #[case] name: &'static str,
    #[case] allowed_sources: &[HostedToolCatalogSource],
    #[case] expected: HostedLookupExpectation,
) {
    let registry = hosted_registry.await;
    let result = registry.get_hosted_tool(name, allowed_sources).await;

    match expected {
        HostedLookupExpectation::Ok => assert!(result.is_ok(), "expected Ok for {name}"),
        HostedLookupExpectation::Err(expected_error) => {
            assert!(matches!(result, Err(error) if error == expected_error));
        }
    }
}
