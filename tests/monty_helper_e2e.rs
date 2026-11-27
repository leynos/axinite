//! End-to-end behaviour of `exec_code` against the real Monty helper.
//!
//! These tests need the helper executable. `make test-monty-e2e` builds it
//! and exports `AXINITE_MONTY_HELPER`; without that variable each test reports
//! itself as skipped and passes, so the ordinary suite does not fail on a host
//! that never built the out-of-workspace package.
//!
//! The helper enforces Linux limits and is documented as the first supported
//! platform, so the file is Linux-only.

#![cfg(target_os = "linux")]

#[path = "support/monty.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axinite::config::SafetyConfig;
use axinite::context::JobContext;
use axinite::safety::SafetyLayer;
use axinite::tools::monty::broker::{BrokerContext, BrokerServices, CallerPolicy};
use axinite::tools::{ApprovalRequirement, ToolError, ToolRegistry};
use serde_json::{Value, json};
use support::{context_with, failing_stub, policy, run, skip_without_helper, stub};

#[tokio::test]
async fn runs_a_guest_that_calls_an_allowed_tool() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let ctx = context_with(
        vec![stub(
            "echo",
            &calls,
            ApprovalRequirement::Never,
            Value::Null,
        )],
        &["echo"],
    )
    .await;

    let output = run(
        &helper,
        &ctx,
        "answer = await echo(message='hi')\nreturn {'result': answer, 'state': {'seen': answer}}",
        &["echo"],
    )
    .await
    .expect("run");

    assert_eq!(
        output.result,
        json!({
            "result": {"echoed": "hi"},
            "state": {"seen": {"echoed": "hi"}}
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn approval_required_callbacks_are_denied_without_effects() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let ctx = context_with(
        vec![stub(
            "guarded",
            &calls,
            ApprovalRequirement::Always,
            Value::Null,
        )],
        &["guarded"],
    )
    .await;

    let error = run(
        &helper,
        &ctx,
        "await guarded(message='x')\nreturn {'state': {}}",
        &["guarded"],
    )
    .await
    .expect_err("denied");

    assert!(matches!(error, ToolError::NotAuthorized(_)), "{error:?}");
    assert!(error.to_string().contains("approval_required"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_remote_tool_rejection_reaches_the_caller() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let ctx = context_with(
        vec![failing_stub(
            "remote",
            &calls,
            ToolError::NotAuthorized("403: approval required upstream".to_string()),
        )],
        &["remote"],
    )
    .await;

    let error = run(
        &helper,
        &ctx,
        "await remote(message='x')\nreturn {'state': {}}",
        &["remote"],
    )
    .await
    .expect_err("remote rejection");

    assert!(matches!(error, ToolError::NotAuthorized(_)), "{error:?}");
    assert!(error.to_string().contains("tool_failed"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_missing_broker_rejects_before_spawning() {
    let ctx = JobContext::default();
    let error = run(
        Path::new("/nonexistent/axinite-monty-helper"),
        &ctx,
        "return {'state': {}}",
        &[],
    )
    .await
    .expect_err("no authority");

    assert!(matches!(error, ToolError::NotAuthorized(_)), "{error:?}");
    assert!(
        error.to_string().contains("caller-issued broker"),
        "{error}"
    );
}

#[tokio::test]
async fn unknown_allowed_tools_are_rejected_at_setup() {
    let registry = ToolRegistry::new();
    let services = BrokerServices {
        tools: Arc::new(registry),
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        hooks: None,
        policy: policy(),
    };
    let error = BrokerContext::new(services, &["missing".to_string()])
        .await
        .expect_err("unknown");
    assert!(error.to_string().contains("unknown tool"), "{error}");
}

#[tokio::test]
async fn recursion_is_refused_at_selection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::new();
    registry.register_sync(stub(
        "exec_code",
        &calls,
        ApprovalRequirement::Never,
        Value::Null,
    ));
    let services = BrokerServices {
        tools: Arc::new(registry),
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        hooks: None,
        policy: CallerPolicy::default(),
    };
    let error = BrokerContext::new(services, &["exec_code".to_string()])
        .await
        .expect_err("reserved");
    assert!(error.to_string().contains("cannot be exposed"), "{error}");
}

#[tokio::test]
async fn a_tool_result_is_sanitized_before_the_guest_sees_it() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let secret = "sk-proj-test1234567890abcdefghij";
    let ctx = context_with(
        vec![stub(
            "leak",
            &calls,
            ApprovalRequirement::Never,
            json!({"key": secret}),
        )],
        &["leak"],
    )
    .await;

    let output = run(
        &helper,
        &ctx,
        "value = await leak(message='x')\nreturn {'result': value, 'state': {}}",
        &["leak"],
    )
    .await
    .expect("run");

    let serialized = serde_json::to_string(&output.result).expect("json");
    assert!(
        !serialized.contains(secret),
        "raw secret reached the guest: {serialized}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// A guest `int` spanning the exact 64-bit range survives the round trip.
///
/// Python integers are arbitrary precision, so `u64::MAX` and `i64::MIN` are
/// ordinary values. The boundary carries them exactly; only a value outside
/// `i64::MIN ..= u64::MAX` is refused.
#[tokio::test]
async fn exact_64_bit_integers_survive_the_guest_round_trip() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;

    let output = run(
        &helper,
        &ctx,
        "return {'result': [18446744073709551615, -9223372036854775808, 9223372036854775808], 'state': {}}",
        &[],
    )
    .await
    .expect("run");

    assert_eq!(
        output.result["result"],
        json!([u64::MAX, i64::MIN, 9_223_372_036_854_775_808_u64])
    );
}

/// A guest integer wider than 64 bits has no exact JSON form, so the run
/// reports a typed failure rather than returning a rounded float.
#[tokio::test]
async fn an_integer_wider_than_64_bits_fails_the_run() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;

    let error = run(
        &helper,
        &ctx,
        "return {'result': 18446744073709551616, 'state': {}}",
        &[],
    )
    .await
    .expect_err("too wide");
    assert!(
        error.to_string().contains("64-bit"),
        "expected an exact-range refusal, got {error}"
    );
}

#[tokio::test]
async fn a_script_error_is_reported_as_a_failed_run() {
    let Some(helper) = skip_without_helper() else {
        return;
    };
    let ctx = context_with(vec![], &[]).await;

    let error = run(&helper, &ctx, "raise ValueError('boom')", &[])
        .await
        .expect_err("script error");
    assert!(matches!(error, ToolError::ExecutionFailed(_)), "{error:?}");
    assert!(error.to_string().contains("boom"), "{error}");
}
