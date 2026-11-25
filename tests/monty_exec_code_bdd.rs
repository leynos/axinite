//! Behaviour tests for `exec_code` caller policy and callback mediation.
//!
//! These scenarios exercise the broker directly, so they need neither the
//! helper process nor an interpreter: they prove the parent's gate refuses a
//! call before any tool effect.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axinite::config::SafetyConfig;
use axinite::context::JobContext;
use axinite::hooks::{
    Hook, HookContext, HookEvent, HookFuture, HookOutcome, HookPoint, HookRegistry,
};
use axinite::safety::SafetyLayer;
use axinite::tools::monty::broker::{BrokerContext, BrokerDenial, BrokerServices, CallerPolicy};
use axinite::tools::{ApprovalRequirement, NativeTool, Tool, ToolError, ToolOutput, ToolRegistry};
use monty_protocol::DenialCode;
use rstest::fixture;
use rstest_bdd_macros::{given, scenario, then, when};
use serde_json::{Value, json};

struct Stub {
    name: &'static str,
    calls: Arc<AtomicUsize>,
    approval: ApprovalRequirement,
}

impl NativeTool for Stub {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "behaviour-test stub"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"],
            "additionalProperties": false
        })
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
}

struct BrokerWorld {
    runtime: tokio::runtime::Runtime,
    broker: Option<BrokerContext>,
    outcome: Option<Result<Value, BrokerDenial>>,
    echo_calls: Arc<AtomicUsize>,
    write_calls: Arc<AtomicUsize>,
}

impl BrokerWorld {
    fn stub(
        &self,
        name: &'static str,
        calls: &Arc<AtomicUsize>,
        approval: ApprovalRequirement,
    ) -> Arc<dyn Tool> {
        Arc::new(Stub {
            name,
            calls: Arc::clone(calls),
            approval,
        })
    }

    fn install(
        &mut self,
        tools: Vec<Arc<dyn Tool>>,
        requested: &[&str],
        hooks: Option<Arc<HookRegistry>>,
    ) {
        let registry = ToolRegistry::new();
        for tool in tools {
            registry.register_sync(tool);
        }
        let names: Vec<String> = requested.iter().map(|name| (*name).to_string()).collect();
        self.broker = Some(
            self.runtime
                .block_on(BrokerContext::new(
                    BrokerServices {
                        tools: Arc::new(registry),
                        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                            max_output_length: 100_000,
                            injection_check_enabled: false,
                        })),
                        hooks,
                        policy: CallerPolicy {
                            user_id: "bdd".to_string(),
                            hook_context: "chat".to_string(),
                            auto_approved: Default::default(),
                        },
                    },
                    &names,
                ))
                .expect("Given step builds a valid broker"),
        );
    }

    fn call(&mut self, name: &str) -> Result<Value, BrokerDenial> {
        let broker = self.broker.as_ref().expect("Given step builds the broker");
        let result = self.runtime.block_on(broker.mediate(
            name,
            &json!({"query": "x"}),
            &JobContext::default(),
        ));
        self.outcome = Some(result.clone());
        result
    }

    fn denial(&self) -> &BrokerDenial {
        match self.outcome.as_ref() {
            Some(Err(denial)) => denial,
            other => panic!("the callback should have been refused, got {other:?}"),
        }
    }

    fn total_calls(&self) -> usize {
        self.echo_calls.load(Ordering::SeqCst) + self.write_calls.load(Ordering::SeqCst)
    }
}

#[fixture]
fn broker_world() -> BrokerWorld {
    BrokerWorld {
        runtime: tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime"),
        broker: None,
        outcome: None,
        echo_calls: Arc::new(AtomicUsize::new(0)),
        write_calls: Arc::new(AtomicUsize::new(0)),
    }
}

const TOOL_CALL_POINTS: &[HookPoint] = &[HookPoint::BeforeToolCall];

struct OutOfSchemaHook;

impl Hook for OutOfSchemaHook {
    fn name(&self) -> &str {
        "out-of-schema"
    }

    fn hook_points(&self) -> &[HookPoint] {
        TOOL_CALL_POINTS
    }

    fn execute<'a>(&'a self, _event: &'a HookEvent, _ctx: &'a HookContext) -> HookFuture<'a> {
        Box::pin(async { Ok(HookOutcome::modify(r#"{"query": 7}"#.to_string())) })
    }
}

#[given("a caller that authorizes the echo tool")]
fn caller_authorizes_echo(broker_world: &mut BrokerWorld) {
    let echo = broker_world.stub(
        "echo",
        &broker_world.echo_calls.clone(),
        ApprovalRequirement::Never,
    );
    broker_world.install(vec![echo], &["echo"], None);
}

#[given("a caller that authorizes only the echo tool")]
fn caller_authorizes_only_echo(broker_world: &mut BrokerWorld) {
    let echo = broker_world.stub(
        "echo",
        &broker_world.echo_calls.clone(),
        ApprovalRequirement::Never,
    );
    let write = broker_world.stub(
        "write",
        &broker_world.write_calls.clone(),
        ApprovalRequirement::Never,
    );
    broker_world.install(vec![echo, write], &["echo"], None);
}

#[given("a caller that authorizes a tool requiring approval")]
fn caller_authorizes_guarded(broker_world: &mut BrokerWorld) {
    let guarded = broker_world.stub(
        "guarded",
        &broker_world.echo_calls.clone(),
        ApprovalRequirement::Always,
    );
    broker_world.install(vec![guarded], &["guarded"], None);
}

#[given("a caller whose hook rewrites the arguments out of schema")]
fn caller_with_rewriting_hook(broker_world: &mut BrokerWorld) {
    let hooks = Arc::new(HookRegistry::new());
    broker_world
        .runtime
        .block_on(hooks.register(Arc::new(OutOfSchemaHook)));
    let echo = broker_world.stub(
        "echo",
        &broker_world.echo_calls.clone(),
        ApprovalRequirement::Never,
    );
    broker_world.install(vec![echo], &["echo"], Some(hooks));
}

#[when("the guest calls the selected echo tool")]
fn guest_calls_echo(broker_world: &mut BrokerWorld) {
    broker_world.call("echo").ok();
}

#[when("the guest calls the unselected write tool")]
fn guest_calls_write(broker_world: &mut BrokerWorld) {
    broker_world.call("write").ok();
}

#[when("the guest calls the selected guarded tool")]
fn guest_calls_guarded(broker_world: &mut BrokerWorld) {
    broker_world.call("guarded").ok();
}

#[when("the guest calls exec_code")]
fn guest_calls_exec_code(broker_world: &mut BrokerWorld) {
    broker_world.call("exec_code").ok();
}

#[then("the echo tool has run once")]
fn echo_ran_once(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.echo_calls.load(Ordering::SeqCst), 1);
}

#[then("the guest receives the tool's JSON result")]
fn guest_receives_result(broker_world: &BrokerWorld) {
    match broker_world.outcome.as_ref() {
        Some(Ok(value)) => assert_eq!(value, &json!({"ok": true})),
        other => panic!("the callback should have succeeded, got {other:?}"),
    }
}

#[then("the call is refused as not_allowed")]
fn refused_not_allowed(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.denial().code, DenialCode::NotAllowed);
}

#[then("the call is refused as approval_required")]
fn refused_approval_required(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.denial().code, DenialCode::ApprovalRequired);
}

#[then("the call is refused as recursion_denied")]
fn refused_recursion_denied(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.denial().code, DenialCode::RecursionDenied);
}

#[then("the call is refused as contract_error")]
fn refused_contract_error(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.denial().code, DenialCode::ContractError);
}

#[then("no tool has run")]
fn no_tool_ran(broker_world: &BrokerWorld) {
    assert_eq!(broker_world.total_calls(), 0);
}

#[scenario(
    path = "src/tools/monty/features/exec_code.feature",
    name = "A selected tool callback runs once"
)]
fn selected_tool_callback_runs_once(broker_world: BrokerWorld) {
    drop(broker_world);
}

#[scenario(
    path = "src/tools/monty/features/exec_code.feature",
    name = "An unselected tool callback is refused"
)]
fn unselected_tool_callback_is_refused(broker_world: BrokerWorld) {
    drop(broker_world);
}

#[scenario(
    path = "src/tools/monty/features/exec_code.feature",
    name = "A tool that always requires approval is refused before dispatch"
)]
fn approval_required_is_refused(broker_world: BrokerWorld) {
    drop(broker_world);
}

#[scenario(
    path = "src/tools/monty/features/exec_code.feature",
    name = "Recursive code execution is refused"
)]
fn recursion_is_refused(broker_world: BrokerWorld) {
    drop(broker_world);
}

#[scenario(
    path = "src/tools/monty/features/exec_code.feature",
    name = "A hook rewrite is revalidated before dispatch"
)]
fn hook_rewrite_is_revalidated(broker_world: BrokerWorld) {
    drop(broker_world);
}
