//! Callback-gate tests: denials must leave the side-effect count at zero.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use monty_protocol::DenialCode;
use pretty_assertions::assert_eq;
use rstest::rstest;
use serde_json::{Value, json};

use crate::context::JobContext;
use crate::hooks::HookRegistry;
use crate::safety::SafetyLayer;
use crate::tools::monty::broker::{BrokerContext, BrokerServices, CallerPolicy};
use crate::tools::{ApprovalRequirement, NativeTool, Tool, ToolError, ToolOutput, ToolRegistry};

struct CountingTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
    approval: ApprovalRequirement,
    schema: Value,
    sensitive: &'static [&'static str],
}

impl NativeTool for CountingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "counting stub"
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, _params: Value, _ctx: &JobContext) -> Result<ToolOutput, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::success(
            json!({"ok": true}),
            Duration::from_millis(1),
        ))
    }

    fn requires_approval(&self, _params: &Value) -> ApprovalRequirement {
        self.approval
    }

    fn sensitive_params(&self) -> &[&str] {
        self.sensitive
    }
}

fn query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"query": {"type": "string"}},
        "required": ["query"],
        "additionalProperties": false
    })
}

fn test_safety() -> SafetyLayer {
    SafetyLayer::new(&crate::config::SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    })
}

fn tool(
    name: &'static str,
    calls: &Arc<AtomicUsize>,
    approval: ApprovalRequirement,
    schema: Value,
) -> Arc<dyn Tool> {
    Arc::new(CountingTool {
        name,
        calls: Arc::clone(calls),
        approval,
        schema,
        sensitive: &[],
    })
}

async fn broker(
    tools: Vec<Arc<dyn Tool>>,
    requested: &[&str],
    policy: CallerPolicy,
    hooks: Option<Arc<HookRegistry>>,
) -> Arc<BrokerContext> {
    let registry = ToolRegistry::new();
    for registered in tools {
        registry.register_sync(registered);
    }
    let names: Vec<String> = requested.iter().map(|name| (*name).to_string()).collect();
    Arc::new(
        BrokerContext::new(
            BrokerServices {
                tools: Arc::new(registry),
                safety: Arc::new(test_safety()),
                hooks,
                policy,
            },
            &names,
        )
        .await
        .expect("broker"),
    )
}

fn policy() -> CallerPolicy {
    CallerPolicy {
        user_id: "user".to_string(),
        hook_context: "chat".to_string(),
        auto_approved: Default::default(),
        permitted: Default::default(),
    }
}

#[tokio::test]
async fn selected_tool_is_invoked_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        None,
    )
    .await;

    let value = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect("allowed");
    assert_eq!(value, json!({"ok": true}));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[rstest]
#[case("beta", DenialCode::NotAllowed)]
#[case("exec_code", DenialCode::RecursionDenied)]
#[tokio::test]
async fn unselected_or_recursive_callbacks_are_denied_without_effects(
    #[case] name: &str,
    #[case] expected: DenialCode,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        None,
    )
    .await;

    let denial = broker
        .mediate(name, &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, expected);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn empty_table_denies_every_callback() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &[],
        policy(),
        None,
    )
    .await;

    let denial = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::NotAllowed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn replaced_registration_is_refused() {
    let calls = Arc::new(AtomicUsize::new(0));
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ToolRegistry::new());
    registry.register_sync(tool(
        "alpha",
        &calls,
        ApprovalRequirement::Never,
        query_schema(),
    ));
    let broker = Arc::new(
        BrokerContext::new(
            BrokerServices {
                tools: Arc::clone(&registry),
                safety: Arc::new(test_safety()),
                hooks: None,
                policy: policy(),
            },
            &["alpha".to_string()],
        )
        .await
        .expect("broker"),
    );
    registry.register_sync(tool(
        "alpha",
        &replacement_calls,
        ApprovalRequirement::Never,
        query_schema(),
    ));

    let denial = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::PolicyDenied);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(replacement_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn always_approval_is_denied_before_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Always,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        None,
    )
    .await;

    let denial = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::ApprovalRequired);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unless_auto_approved_needs_the_caller_snapshot() {
    let calls = Arc::new(AtomicUsize::new(0));
    let tools = vec![tool(
        "alpha",
        &calls,
        ApprovalRequirement::UnlessAutoApproved,
        query_schema(),
    )];
    let denied = broker(tools.clone(), &["alpha"], policy(), None).await;
    let denial = denied
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::ApprovalRequired);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut approved_policy = policy();
    approved_policy.auto_approved.insert("alpha".to_string());
    let approved = broker(tools, &["alpha"], approved_policy, None).await;
    approved
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect("allowed");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn schema_violation_is_a_contract_error_without_effects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        None,
    )
    .await;

    for arguments in [json!({"query": 7}), json!({}), json!("not an object")] {
        let denial = broker
            .mediate("alpha", &arguments, &JobContext::default())
            .await
            .expect_err("denied");
        assert!(
            matches!(
                denial.code,
                DenialCode::ContractError | DenialCode::NotAllowed
            ),
            "unexpected denial: {denial:?}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[path = "hook_tests.rs"]
mod hooks;
