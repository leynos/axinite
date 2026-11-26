//! Protocol integrity and process lifecycle of the Monty supervisor.
//!
//! These tests forge frames with a fake helper, so they exercise the parent's
//! own checks rather than the interpreter. They are Linux-only because the
//! fake helper is a shell script and the lifecycle assertion reads `/proc`.

#![cfg(target_os = "linux")]

#[path = "support/monty.rs"]
mod support;

use std::time::Duration;

use axinite::tools::ToolError;
use axinite::tools::monty::runner::{HelperProgram, RunError, RunLimits, RunRequest, Supervisor};
use serde_json::json;
use support::{context_with, run, skip_without_helper, write_frames, write_script};

fn request(run_id: &str) -> RunRequest {
    RunRequest {
        run_id: run_id.to_string(),
        code: "return {'state': {}}".to_string(),
        entrypoint: "main".to_string(),
        params: json!({}),
        state: json!({}),
        limits: RunLimits::default(),
    }
}

#[tokio::test]
async fn a_malformed_helper_fails_the_run() {
    let Some(_) = skip_without_helper() else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let script = write_script(temp.path(), "malformed", "#!/bin/sh\nprintf 'nonsense'\n");
    let ctx = context_with(vec![], &[]).await;

    let error = run(&script, &ctx, "return {'state': {}}", &[])
        .await
        .expect_err("malformed");
    assert!(matches!(error, ToolError::ExecutionFailed(_)), "{error:?}");
}

#[tokio::test]
async fn a_callback_for_another_run_is_refused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let frames = temp.path().join("frames.bin");
    write_frames(
        &frames,
        &[
            r#"{"kind":"hello","protocol_version":1}"#,
            r#"{"kind":"host_call","run_id":"other-run","call_id":1,"name":"ghost","arguments":{}}"#,
        ],
    );
    let script = write_script(
        temp.path(),
        "wrong-run",
        &format!("#!/bin/sh\ncat {}\nexec sleep 30\n", frames.display()),
    );

    let ctx = context_with(vec![], &[]).await;
    let broker = ctx.broker.clone().expect("broker");
    let error = Supervisor::with_program(HelperProgram::at(&script))
        .run(request("fixed-run"), &broker, &ctx)
        .await
        .expect_err("foreign callback");

    assert!(
        matches!(&error, RunError::RuntimeCrash(message) if message.contains("callback for another run")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_replayed_callback_id_is_refused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let frames = temp.path().join("frames.bin");
    write_frames(
        &frames,
        &[
            r#"{"kind":"hello","protocol_version":1}"#,
            r#"{"kind":"host_call","run_id":"fixed-run","call_id":1,"name":"echo","arguments":{}}"#,
            r#"{"kind":"host_call","run_id":"fixed-run","call_id":1,"name":"echo","arguments":{}}"#,
        ],
    );
    let script = write_script(
        temp.path(),
        "replayed",
        &format!("#!/bin/sh\ncat {}\nexec sleep 30\n", frames.display()),
    );

    // The tool must succeed on its first call, or the denial would end the run
    // before the replayed id could be read; a replay is what this test refuses.
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tool = support::stub(
        "echo",
        &calls,
        axinite::tools::ApprovalRequirement::Never,
        json!({"ok": true}),
    );
    let ctx = context_with(vec![tool], &["echo"]).await;
    let broker = ctx.broker.clone().expect("broker");
    let error = Supervisor::with_program(HelperProgram::at(&script))
        .run(request("fixed-run"), &broker, &ctx)
        .await
        .expect_err("replayed callback id");

    assert!(
        matches!(&error, RunError::RuntimeCrash(message) if message.contains("unexpected host callback")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_terminal_for_another_run_is_refused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let frames = temp.path().join("frames.bin");
    write_frames(
        &frames,
        &[
            r#"{"kind":"hello","protocol_version":1}"#,
            r#"{"kind":"terminal","run_id":"other-run","outcome":{"status":"completed","result":null,"state":{}}}"#,
        ],
    );
    let script = write_script(
        temp.path(),
        "wrong-terminal",
        &format!("#!/bin/sh\ncat {}\nexec sleep 30\n", frames.display()),
    );

    let ctx = context_with(vec![], &[]).await;
    let broker = ctx.broker.clone().expect("broker");
    let error = Supervisor::with_program(HelperProgram::at(&script))
        .run(request("fixed-run"), &broker, &ctx)
        .await
        .expect_err("foreign terminal");

    assert!(
        matches!(&error, RunError::RuntimeCrash(message) if message.contains("terminal for another run")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_dropped_run_future_leaves_no_helper_process() {
    let Some(_) = skip_without_helper() else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let frames = temp.path().join("frames.bin");
    let pidfile = temp.path().join("helper.pid");
    write_frames(&frames, &[r#"{"kind":"hello","protocol_version":1}"#]);
    let script = write_script(
        temp.path(),
        "slow",
        &format!(
            "#!/bin/sh\necho $$ > {}\ncat {}\nexec sleep 60\n",
            pidfile.display(),
            frames.display()
        ),
    );

    let ctx = context_with(vec![], &[]).await;
    let cancelled = tokio::time::timeout(
        Duration::from_millis(400),
        run(&script, &ctx, "return {'state': {}}", &[]),
    )
    .await;
    assert!(cancelled.is_err(), "the run should outlive the timeout");

    let pid = std::fs::read_to_string(&pidfile)
        .expect("the fake helper records its pid")
        .trim()
        .to_string();
    let proc_entry = format!("/proc/{pid}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&proc_entry).exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "helper process {pid} survived cancellation"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
