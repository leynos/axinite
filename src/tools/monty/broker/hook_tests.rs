//! Hook-interaction tests for the callback gate.
//!
//! A hook may rewrite a callback's arguments, so every rewrite is re-validated
//! before dispatch, and hooks only ever see redacted sensitive values.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use monty_protocol::DenialCode;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use crate::context::JobContext;
use crate::hooks::{
    Hook, HookContext, HookEvent, HookFuture, HookOutcome, HookPoint, HookRegistry,
};
use crate::tools::{ApprovalRequirement, Tool};

use super::{CountingTool, broker, policy, query_schema, tool};

const TOOL_CALL_POINTS: &[HookPoint] = &[HookPoint::BeforeToolCall];

struct RewriteHook {
    replacement: Value,
    seen: Arc<Mutex<Option<Value>>>,
}

impl Hook for RewriteHook {
    fn name(&self) -> &str {
        "rewrite"
    }

    fn hook_points(&self) -> &[HookPoint] {
        TOOL_CALL_POINTS
    }

    fn execute<'a>(&'a self, event: &'a HookEvent, _ctx: &'a HookContext) -> HookFuture<'a> {
        let parameters = match event {
            HookEvent::ToolCall { parameters, .. } => parameters.clone(),
            _ => Value::Null,
        };
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(parameters);
        }
        let replacement = self.replacement.to_string();
        Box::pin(async move { Ok(HookOutcome::modify(replacement)) })
    }
}

#[tokio::test]
async fn hook_rewrite_is_revalidated_before_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let hooks = Arc::new(HookRegistry::new());
    hooks
        .register(Arc::new(RewriteHook {
            replacement: json!({"query": 7}),
            seen: Arc::new(Mutex::new(None)),
        }))
        .await;
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        Some(hooks),
    )
    .await;

    let denial = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::ContractError);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn hook_sees_redacted_sensitive_arguments() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(None));
    let hooks = Arc::new(HookRegistry::new());
    hooks
        .register(Arc::new(RewriteHook {
            replacement: json!({"secret": "hunter2"}),
            seen: Arc::clone(&seen),
        }))
        .await;
    let tool: Arc<dyn Tool> = Arc::new(CountingTool {
        name: "alpha",
        calls: Arc::clone(&calls),
        approval: ApprovalRequirement::Never,
        schema: json!({
            "type": "object",
            "properties": {"secret": {"type": "string"}},
            "required": ["secret"],
            "additionalProperties": false
        }),
        sensitive: &["secret"],
    });
    let broker = broker(vec![tool], &["alpha"], policy(), Some(hooks)).await;

    broker
        .mediate(
            "alpha",
            &json!({"secret": "hunter2"}),
            &JobContext::default(),
        )
        .await
        .expect("allowed");

    let observed = seen.lock().expect("seen").clone().expect("observed");
    assert_eq!(observed, json!({"secret": "[REDACTED]"}));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejecting_hook_denies_without_effects() {
    struct Reject;
    impl Hook for Reject {
        fn name(&self) -> &str {
            "reject"
        }
        fn hook_points(&self) -> &[HookPoint] {
            TOOL_CALL_POINTS
        }
        fn execute<'a>(&'a self, _event: &'a HookEvent, _ctx: &'a HookContext) -> HookFuture<'a> {
            Box::pin(async {
                Ok(HookOutcome::Reject {
                    reason: "no".to_string(),
                })
            })
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let hooks = Arc::new(HookRegistry::new());
    hooks.register(Arc::new(Reject)).await;
    let broker = broker(
        vec![tool(
            "alpha",
            &calls,
            ApprovalRequirement::Never,
            query_schema(),
        )],
        &["alpha"],
        policy(),
        Some(hooks),
    )
    .await;

    let denial = broker
        .mediate("alpha", &json!({"query": "x"}), &JobContext::default())
        .await
        .expect_err("denied");
    assert_eq!(denial.code, DenialCode::PolicyDenied);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
