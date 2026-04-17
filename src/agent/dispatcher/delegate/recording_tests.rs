//! Tests for dispatcher tool-result recording and post-flight handling.

use std::sync::Arc;
use std::time::Duration;

use rstest::rstest;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::*;
use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::session::Session;
use crate::channels::{ChannelManager, IncomingMessage};
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::context::{ContextManager, JobContext};
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::testing::StubLlm;
use crate::tools::ToolRegistry;

fn make_test_agent() -> Agent {
    let deps = AgentDeps {
        store: None,
        llm: Arc::new(StubLlm::new("ok")) as Arc<dyn LlmProvider>,
        cheap_llm: None,
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        tools: Arc::new(ToolRegistry::new()),
        workspace: None,
        extension_manager: None,
        skill_registry: None,
        skill_catalog: None,
        skills_config: SkillsConfig::default(),
        hooks: Arc::new(HookRegistry::new()),
        cost_guard: Arc::new(CostGuard::new(CostGuardConfig::default())),
        sse_tx: None,
        http_interceptor: None,
        transcription: None,
        document_extraction: None,
    };

    Agent::new(
        AgentConfig {
            name: "test-agent".to_string(),
            max_parallel_jobs: 1,
            job_timeout: Duration::from_secs(60),
            stuck_threshold: Duration::from_secs(60),
            repair_check_interval: Duration::from_secs(30),
            max_repair_attempts: 1,
            use_planning: false,
            session_idle_timeout: Duration::from_secs(300),
            allow_local_tools: false,
            max_cost_per_day_cents: None,
            max_actions_per_hour: None,
            max_tool_iterations: 5,
            auto_approve_tools: false,
            default_timezone: "UTC".to_string(),
            max_tokens_per_job: 0,
        },
        deps,
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    )
}

fn make_delegate<'a>(
    agent: &'a Agent,
    session: Arc<Mutex<Session>>,
    thread_id: Uuid,
    message: &'a IncomingMessage,
) -> ChatDelegate<'a> {
    ChatDelegate {
        agent,
        session,
        thread_id,
        message,
        job_ctx: JobContext::with_user(&message.user_id, &message.channel, "test session"),
        active_skills: vec![],
        cached_prompt: String::new(),
        cached_prompt_no_tools: String::new(),
        nudge_at: 0,
        force_text_at: 0,
        user_tz: chrono_tz::UTC,
    }
}

fn make_tool_call(name: &str) -> crate::llm::ToolCall {
    crate::llm::ToolCall {
        id: "call-1".to_string(),
        name: name.to_string(),
        arguments: serde_json::json!({}),
    }
}

async fn make_delegate_harness(
    tool_name: &str,
) -> (
    Agent,
    Arc<Mutex<Session>>,
    Uuid,
    IncomingMessage,
    crate::llm::ToolCall,
) {
    let agent = make_test_agent();
    let message = IncomingMessage::new("web", "user-1", "run tool");
    let mut session = Session::new("user-1");
    let thread_id = {
        let thread = session.create_thread();
        thread.start_turn("run tool");
        thread
            .last_turn_mut()
            .expect("newly started turn should be available")
            .record_tool_call(tool_name, serde_json::json!({}));
        thread.id
    };

    (
        agent,
        Arc::new(Mutex::new(session)),
        thread_id,
        message,
        make_tool_call(tool_name),
    )
}

#[rstest]
#[tokio::test]
async fn process_runnable_tool_records_successful_json_results_as_objects() {
    let (agent, session, thread_id, message, tool_call) = make_delegate_harness("echo").await;
    let delegate = make_delegate(&agent, Arc::clone(&session), thread_id, &message);
    let mut reason_ctx = ReasoningContext::default();

    let instructions = delegate
        .process_runnable_tool(
            &tool_call,
            Ok(r#"{"key":"value"}"#.to_string()),
            &mut reason_ctx,
        )
        .await;

    assert!(
        instructions.is_none(),
        "plain successful tool output should not trigger auth flow"
    );

    let sess = session.lock().await;
    let recorded = sess.threads[&thread_id]
        .last_turn()
        .expect("turn should exist after processing")
        .tool_calls
        .first()
        .and_then(|call| call.result.clone())
        .expect("successful tool result should be recorded");

    assert!(
        matches!(recorded, serde_json::Value::Object(_)),
        "successful JSON results should be stored as structured objects"
    );
}

#[rstest]
#[tokio::test]
async fn process_runnable_tool_records_non_json_results_as_strings() {
    let (agent, session, thread_id, message, tool_call) = make_delegate_harness("echo").await;
    let delegate = make_delegate(&agent, Arc::clone(&session), thread_id, &message);
    let mut reason_ctx = ReasoningContext::default();
    let raw_output = "plain text output";
    let (_preview, expected_wrapped) = delegate.sanitize_output(&tool_call.name, raw_output);

    let instructions = delegate
        .process_runnable_tool(&tool_call, Ok(raw_output.to_string()), &mut reason_ctx)
        .await;

    assert!(
        instructions.is_none(),
        "plain successful tool output should not trigger auth flow"
    );

    let sess = session.lock().await;
    let recorded = sess.threads[&thread_id]
        .last_turn()
        .expect("turn should exist after processing")
        .tool_calls
        .first()
        .and_then(|call| call.result.clone())
        .expect("successful tool result should be recorded");

    assert_eq!(
        recorded,
        serde_json::Value::String(expected_wrapped),
        "non-JSON output should remain a string after sanitisation"
    );
}
