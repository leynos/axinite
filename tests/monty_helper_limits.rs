//! Resource limits, admission and cancellation for `exec_code`.
//!
//! These tests drive the real helper through the supervisor, so they need
//! `AXINITE_MONTY_HELPER` in the form `make test-monty-e2e` provides. Without
//! it each test reports itself as skipped and passes.

#![cfg(target_os = "linux")]

#[path = "support/monty.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axinite::tools::monty::exec_code::ExecCodeTool;
use axinite::tools::{ApprovalRequirement, NativeTool, ToolError};
use serde_json::{Value, json};
use support::{context_with, context_with_policy, policy, run, skip_without_helper, stub};

#[tokio::test]
async fn a_run_past_its_deadline_is_terminated() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;
    let params = json!({
        "code": "while True:\n    pass",
        "allowed_tools": [],
        "limits": {"deadline_ms": 250}
    });

    let started = Instant::now();
    let error = NativeTool::execute(&ExecCodeTool::with_helper(&helper), params, &ctx)
        .await
        .expect_err("timeout");
    assert!(matches!(error, ToolError::ExecutionFailed(_)), "{error:?}");
    assert!(error.to_string().contains("deadline"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn a_deep_recursion_hits_the_guest_limit() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;
    let params = json!({
        "code": "def f(n):\n    return f(n + 1)\n\nf(0)\nreturn {'state': {}}",
        "allowed_tools": [],
        "limits": {"max_recursion_depth": 20, "deadline_ms": 10_000}
    });

    let error = NativeTool::execute(&ExecCodeTool::with_helper(&helper), params, &ctx)
        .await
        .expect_err("recursion limit");
    assert!(matches!(error, ToolError::ExecutionFailed(_)), "{error:?}");
    let message = error.to_string().to_lowercase();
    assert!(message.contains("recursion"), "{error}");
}

#[tokio::test]
async fn a_guest_that_exceeds_its_memory_budget_fails() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;
    let params = json!({
        "code": "text = 'x' * (32 * 1024 * 1024)\nreturn {'state': {'len': len(text)}}",
        "allowed_tools": [],
        "limits": {"max_memory_bytes": 8_388_608, "deadline_ms": 10_000}
    });

    let error = NativeTool::execute(&ExecCodeTool::with_helper(&helper), params, &ctx)
        .await
        .expect_err("memory limit");
    assert!(matches!(error, ToolError::ExecutionFailed(_)), "{error:?}");
    let message = error.to_string().to_lowercase();
    assert!(
        message.contains("memory") || message.contains("resource"),
        "{error}"
    );
}

#[tokio::test]
async fn a_cancelled_run_leaves_the_pool_usable() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;
    let params = json!({
        "code": "while True:\n    pass",
        "allowed_tools": [],
        "limits": {"deadline_ms": 60_000}
    });

    let cancelled = tokio::time::timeout(
        Duration::from_millis(300),
        NativeTool::execute(&ExecCodeTool::with_helper(&helper), params, &ctx),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "the outer future should have been dropped"
    );

    run(&helper, &ctx, "return {'state': {'alive': True}}", &[])
        .await
        .expect("admission is restored after cancellation");
}

#[tokio::test]
async fn concurrent_runs_keep_their_grants_disjoint() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let a_calls = Arc::new(AtomicUsize::new(0));
    let b_calls = Arc::new(AtomicUsize::new(0));
    let only_a = context_with(
        vec![stub(
            "alpha",
            &a_calls,
            ApprovalRequirement::Never,
            Value::Null,
        )],
        &["alpha"],
    )
    .await;
    let only_b = context_with(
        vec![stub(
            "beta",
            &b_calls,
            ApprovalRequirement::Never,
            Value::Null,
        )],
        &["beta"],
    )
    .await;

    let crossing = run(
        &helper,
        &only_a,
        "await beta(message='x')\nreturn {'state': {}}",
        &["beta"],
    );
    let permitted = run(
        &helper,
        &only_b,
        "answer = await beta(message='x')\nreturn {'result': answer, 'state': {}}",
        &["beta"],
    );
    let (crossing, permitted) = tokio::join!(crossing, permitted);

    assert!(crossing.is_err(), "a run must not call another run's tool");
    permitted.expect("the owning run succeeds");
    assert_eq!(a_calls.load(Ordering::SeqCst), 0);
    assert_eq!(b_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn pre_approved_tools_run_without_interactive_approval() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut caller = policy();
    caller.auto_approved.insert("guarded".to_string());
    let ctx = context_with_policy(
        vec![stub(
            "guarded",
            &calls,
            ApprovalRequirement::UnlessAutoApproved,
            Value::Null,
        )],
        &["guarded"],
        caller,
    )
    .await;

    let output = run(
        &helper,
        &ctx,
        "answer = await guarded(message='ok')\nreturn {'result': answer, 'state': {}}",
        &["guarded"],
    )
    .await
    .expect("pre-approved call");

    assert_eq!(
        output.result,
        json!({"result": {"echoed": "ok"}, "state": {}})
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn executions_beyond_the_permit_pool_fail_fast() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = Arc::new(context_with(vec![], &[]).await);
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let helper = helper.clone();
        let ctx = Arc::clone(&ctx);
        tasks.push(tokio::spawn(async move {
            let params = json!({
                "code": "while True:\n    pass",
                "allowed_tools": [],
                "limits": {"deadline_ms": 2000}
            });
            NativeTool::execute(&ExecCodeTool::with_helper(helper), params, &ctx).await
        }));
    }
    let mut overloaded = 0;
    for task in tasks {
        let result = task.await.expect("task");
        if let Err(ToolError::ExecutionFailed(message)) = &result
            && message.contains("too many code runs")
        {
            overloaded += 1;
        }
    }
    assert!(
        overloaded > 0,
        "the admission pool must fail fast when full"
    );
}
