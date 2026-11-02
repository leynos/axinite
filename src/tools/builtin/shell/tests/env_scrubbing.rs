//! Environment scrubbing tests: only safe variables reach the child process,
//! and secrets never leak through the inherited process environment.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(windows)]
use crate::config::EnvContext;
use crate::context::JobContext;
use crate::tools::tool::NativeTool;

use super::super::ShellTool;
use super::shell_context;

#[tokio::test]
async fn test_env_scrubbing_hides_secrets() -> anyhow::Result<()> {
    let secret_var = "AXINITE_TEST_SECRET_KEY";
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env.with_env(secret_var, "super_secret_value_12345"));
    let ctx = JobContext::default();

    // Run `env` (or `printenv`) and check the output
    let result = tool
        .execute(serde_json::json!({"command": "env"}), &ctx)
        .await
        .unwrap();

    let output = result.result.get("output").unwrap().as_str().unwrap();

    // The secret should NOT appear in the child process environment
    assert!(
        !output.contains("super_secret_value_12345"),
        "Secret leaked through env scrubbing! Output contained the secret value."
    );
    assert!(
        !output.contains(secret_var),
        "Secret variable name leaked through env scrubbing!"
    );

    // But PATH should still be there (it's in SAFE_ENV_VARS)
    assert!(
        output.contains("PATH="),
        "PATH should be forwarded to child processes"
    );
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_forwards_explicitly_empty_safe_var() -> anyhow::Result<()> {
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env.with_env("TMPDIR", ""));
    let ctx = JobContext::default();

    let result = tool
        .execute(
            serde_json::json!({"command": "printf 'TMPDIR=[%s]' \"${TMPDIR-<unset>}\""}),
            &ctx,
        )
        .await
        .expect("an empty safe variable should still be forwarded");

    assert_eq!(
        result.result["output"], "TMPDIR=[]",
        "an explicitly empty safe variable must be forwarded as empty, not dropped"
    );
    Ok(())
}

/// Windows environment variable names are case-insensitive, so a snapshot
/// captured with the conventional `Path` spelling must still forward `PATH`.
#[cfg(windows)]
#[tokio::test]
async fn test_env_scrubbing_forwards_mixed_case_path_on_windows() -> anyhow::Result<()> {
    let home_dir = tempfile::tempdir()?;
    let env = EnvContext::default()
        .with_env("Path", r"C:\Windows\System32")
        .with_env("HOME", home_dir.path().to_string_lossy());
    let tool = ShellTool::from_context(env);
    let ctx = JobContext::default();

    let result = tool
        .execute(serde_json::json!({"command": "echo %PATH%"}), &ctx)
        .await
        .expect("echo should run");

    let output = result.result["output"].as_str().unwrap_or_default();
    assert!(
        output.contains(r"C:\Windows\System32"),
        "a mixed-case Path entry must still be forwarded to the child: {output}"
    );
    Ok(())
}

const INHERITED_SECRET_CHILD_MARKER: &str = "AXINITE_SHELL_INHERITED_SECRET_CHILD";
const INHERITED_SECRET_VAR: &str = "AXINITE_SHELL_INHERITED_SECRET";
const INHERITED_SECRET_VALUE: &str = "inherited_secret_value_98765";

/// A secret that exists only in the real process environment must not reach
/// the shell's child process.
///
/// The test re-executes the test binary so the secret is genuinely inherited
/// rather than injected into the `EnvContext` snapshot. Removing
/// `env_clear()` would let the inherited value through and fail the child.
#[tokio::test]
async fn test_env_scrubbing_hides_inherited_process_secret() -> anyhow::Result<()> {
    let is_child = {
        #[expect(
            clippy::disallowed_methods,
            reason = "child-process test mode marker set by the parent test binary"
        )]
        std::env::var_os(INHERITED_SECRET_CHILD_MARKER)
    }
    .is_some();

    if is_child {
        return run_inherited_secret_child().await;
    }

    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .args([
            "--exact",
            "tools::builtin::shell::tests::env_scrubbing::test_env_scrubbing_hides_inherited_process_secret",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(INHERITED_SECRET_CHILD_MARKER, "1")
        .env(INHERITED_SECRET_VAR, INHERITED_SECRET_VALUE)
        .status()?;

    assert!(
        status.success(),
        "child process must confirm the inherited secret is scrubbed: {status}"
    );
    Ok(())
}

async fn run_inherited_secret_child() -> anyhow::Result<()> {
    // The snapshot deliberately omits the secret while the real process
    // environment contains it, so only `env_clear()` keeps it out of the
    // shell's child.
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env);
    let ctx = JobContext::default();

    let result = tool
        .execute(serde_json::json!({"command": "env"}), &ctx)
        .await
        .expect("env should run in the child shell");

    let output = result.result["output"].as_str().unwrap_or_default();
    assert!(
        !output.contains(INHERITED_SECRET_VALUE),
        "a secret present only in the real process environment leaked into the shell"
    );
    assert!(
        !output.contains(INHERITED_SECRET_VAR),
        "the inherited secret's variable name leaked into the shell"
    );
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_forwards_safe_vars() -> anyhow::Result<()> {
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env);
    let ctx = JobContext::default();

    // HOME should be forwarded
    let result = tool
        .execute(serde_json::json!({"command": "echo $HOME"}), &ctx)
        .await
        .unwrap();

    let output = result
        .result
        .get("output")
        .unwrap()
        .as_str()
        .unwrap()
        .trim();
    assert!(
        !output.is_empty(),
        "HOME should be available in child process"
    );
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_forwards_explicit_extra_env() -> anyhow::Result<()> {
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env);
    let extra_env = HashMap::from([(
        "AXINITE_INJECTED_TEST_VALUE".to_string(),
        "allowed".to_string(),
    )]);
    let ctx = JobContext {
        extra_env: Arc::new(extra_env),
        ..JobContext::default()
    };

    let result = tool
        .execute(
            serde_json::json!({"command": "printf '%s' \"$AXINITE_INJECTED_TEST_VALUE\""}),
            &ctx,
        )
        .await
        .expect("explicit extra environment should be forwarded");
    assert_eq!(result.result["output"], "allowed");
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_common_secret_patterns() -> anyhow::Result<()> {
    // Simulate common secret env vars that agents/tools might set
    let secrets = [
        ("OPENAI_API_KEY", "sk-test-fake-key-123"),
        ("NEARAI_SESSION_TOKEN", "sess_fake_token_abc"),
        ("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI/fake"),
        ("DATABASE_URL", "postgres://user:pass@localhost/db"),
    ];

    let (_home_dir, env) = shell_context()?;
    let env = secrets
        .iter()
        .fold(env, |ctx, (name, value)| ctx.with_env(*name, *value));
    let tool = ShellTool::from_context(env);
    let ctx = JobContext::default();

    let result = tool
        .execute(serde_json::json!({"command": "env"}), &ctx)
        .await
        .unwrap();

    let output = result.result.get("output").unwrap().as_str().unwrap();

    for (name, value) in &secrets {
        assert!(
            !output.contains(value),
            "{name} value leaked through env scrubbing!"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_custom_var_hidden() -> anyhow::Result<()> {
    // Verify that arbitrary values in the input snapshot are not forwarded.
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env.with_env("AXINITE_QA_TEST_SECRET", "supersecret123"));
    let ctx = JobContext::default();

    let result = tool
        .execute(serde_json::json!({"command": "env"}), &ctx)
        .await
        .unwrap();

    let output = result.result.get("output").unwrap().as_str().unwrap();
    assert!(
        !output.contains("AXINITE_QA_TEST_SECRET"),
        "env scrubbing must hide non-safe vars from child processes"
    );
    assert!(
        !output.contains("supersecret123"),
        "secret value must not appear in child env output"
    );
    Ok(())
}

#[tokio::test]
async fn test_env_scrubbing_path_preserved() -> anyhow::Result<()> {
    // PATH must be preserved for commands to resolve
    let (_home_dir, env) = shell_context()?;
    let tool = ShellTool::from_context(env);
    let ctx = JobContext::default();

    let result = tool
        .execute(serde_json::json!({"command": "env"}), &ctx)
        .await
        .unwrap();

    let output = result.result.get("output").unwrap().as_str().unwrap();
    assert!(
        output.contains("PATH="),
        "PATH must be preserved in child env"
    );
    Ok(())
}
