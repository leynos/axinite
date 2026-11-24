//! Shared scaffolding for the `exec_code` end-to-end and supervisor tests.
//!
//! The helper executable is discovered from `AXINITE_MONTY_HELPER`, which
//! `make test-monty-e2e` sets after building the out-of-workspace package.
//! Without it, a test reports itself as skipped so the ordinary suite does not
//! fail on a host that never built the helper.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axinite::config::{EnvContext, SafetyConfig};
use axinite::context::JobContext;
use axinite::safety::SafetyLayer;
use axinite::tools::monty::broker::{BrokerContext, CallerPolicy};
use axinite::tools::monty::exec_code::ExecCodeTool;
use axinite::tools::{ApprovalRequirement, NativeTool, Tool, ToolError, ToolOutput, ToolRegistry};
use serde_json::{Value, json};

/// The environment variable naming the built helper.
pub const HELPER_ENV: &str = "AXINITE_MONTY_HELPER";

/// The helper path, if `AXINITE_MONTY_HELPER` names a readable file.
pub fn helper_path() -> Option<PathBuf> {
    let path = PathBuf::from(EnvContext::capture_ambient().get(HELPER_ENV)?);
    path.is_file().then_some(path)
}

/// The helper path, or `None` after reporting that the test is skipped.
pub fn skip_without_helper() -> Option<PathBuf> {
    let helper = helper_path();
    if helper.is_none() {
        eprintln!("skipping: {HELPER_ENV} does not name a helper executable");
    }
    helper
}

/// A tool stub whose observable contract is a call counter and a fixed result.
pub struct Stub {
    name: &'static str,
    calls: Arc<AtomicUsize>,
    approval: ApprovalRequirement,
    result: Value,
    failure: Option<ToolError>,
}

impl NativeTool for Stub {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "end-to-end stub"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"message": {"type": "string"}},
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, _ctx: &JobContext) -> Result<ToolOutput, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.failure {
            return Err(match error {
                ToolError::NotAuthorized(message) => ToolError::NotAuthorized(message.clone()),
                other => ToolError::ExecutionFailed(other.to_string()),
            });
        }
        let result = if self.result.is_null() {
            json!({"echoed": params.get("message").cloned().unwrap_or(Value::Null)})
        } else {
            self.result.clone()
        };
        Ok(ToolOutput::success(result, Duration::from_millis(1)))
    }

    fn requires_approval(&self, _params: &Value) -> ApprovalRequirement {
        self.approval
    }
}

/// Builds a stub that echoes `message` unless a fixed result is supplied.
pub fn stub(
    name: &'static str,
    calls: &Arc<AtomicUsize>,
    approval: ApprovalRequirement,
    result: Value,
) -> Arc<dyn Tool> {
    Arc::new(Stub {
        name,
        calls: Arc::clone(calls),
        approval,
        result,
        failure: None,
    })
}

/// Builds a stub that fails with the caller-facing remote rejection.
pub fn failing_stub(
    name: &'static str,
    calls: &Arc<AtomicUsize>,
    failure: ToolError,
) -> Arc<dyn Tool> {
    Arc::new(Stub {
        name,
        calls: Arc::clone(calls),
        approval: ApprovalRequirement::Never,
        result: Value::Null,
        failure: Some(failure),
    })
}

/// The caller policy used by these tests.
pub fn policy() -> CallerPolicy {
    CallerPolicy {
        user_id: "e2e".to_string(),
        hook_context: "chat".to_string(),
        auto_approved: Default::default(),
    }
}

/// Builds a job context carrying a broker for the requested tools.
pub async fn context_with(tools: Vec<Arc<dyn Tool>>, requested: &[&str]) -> JobContext {
    context_with_policy(tools, requested, policy()).await
}

/// Builds a job context with an explicit caller policy.
pub async fn context_with_policy(
    tools: Vec<Arc<dyn Tool>>,
    requested: &[&str],
    policy: CallerPolicy,
) -> JobContext {
    let registry = ToolRegistry::new();
    for tool in tools {
        registry.register_sync(tool);
    }
    let names: Vec<String> = requested.iter().map(|name| (*name).to_string()).collect();
    let broker = BrokerContext::new(
        Arc::new(registry),
        Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        None,
        &names,
        policy,
    )
    .await
    .expect("broker");
    JobContext {
        broker: Some(Arc::new(broker)),
        ..JobContext::default()
    }
}

/// Runs one guest program against `helper`.
pub async fn run(
    helper: &Path,
    ctx: &JobContext,
    code: &str,
    allowed: &[&str],
) -> Result<ToolOutput, ToolError> {
    let params = json!({"code": code, "allowed_tools": allowed});
    NativeTool::execute(&ExecCodeTool::with_helper(helper), params, ctx).await
}

/// Writes an executable shell script into `directory`.
pub fn write_script(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, body).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
    }
    path
}

/// Encodes one length-framed protocol frame.
pub fn frame(payload: &str) -> Vec<u8> {
    let length = u32::try_from(payload.len()).expect("frame length fits");
    let mut bytes = length.to_be_bytes().to_vec();
    bytes.extend_from_slice(payload.as_bytes());
    bytes
}

/// Writes a binary frame file a fake helper can replay with `cat`.
pub fn write_frames(path: &Path, payloads: &[&str]) {
    let mut bytes = Vec::new();
    for payload in payloads {
        bytes.extend_from_slice(&frame(payload));
    }
    std::fs::write(path, bytes).expect("write frames");
}

/// The `hello` frame every helper sends first.
pub fn hello_frame() -> Vec<u8> {
    frame(r#"{"kind":"hello","protocol_version":1}"#)
}
