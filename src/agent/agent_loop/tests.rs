//! Unit tests for the agent loop's message handling and lifecycle.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use super::{Agent, AgentDeps};
use crate::channels::{
    ChannelManager, IncomingMessage, MessageStream, NativeChannel, OutgoingResponse,
};
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::context::ContextManager;
use crate::error::ChannelError;
use crate::hooks::HookRegistry;
use crate::safety::SafetyLayer;
use crate::testing::StubLlm;
use crate::tools::ToolRegistry;

type BroadcastLog = Arc<Mutex<Vec<(String, OutgoingResponse)>>>;

struct BroadcastCaptureChannel {
    name: String,
    rx: tokio::sync::Mutex<Option<mpsc::Receiver<IncomingMessage>>>,
    broadcasts: BroadcastLog,
}

impl BroadcastCaptureChannel {
    fn new(name: impl Into<String>) -> (Self, mpsc::Sender<IncomingMessage>, BroadcastLog) {
        let (tx, rx) = mpsc::channel(16);
        let broadcasts = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                name: name.into(),
                rx: tokio::sync::Mutex::new(Some(rx)),
                broadcasts: Arc::clone(&broadcasts),
            },
            tx,
            broadcasts,
        )
    }
}

impl NativeChannel for BroadcastCaptureChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let rx = self
            .rx
            .lock()
            .await
            .take()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: self.name.clone(),
                reason: "start() already called".to_string(),
            })?;
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        _response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.broadcasts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((user_id.to_string(), response));
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

fn make_test_agent(
    channels: Arc<ChannelManager>,
    context_manager: Arc<ContextManager>,
    repair_check_interval: Duration,
    stuck_threshold: Duration,
) -> Agent {
    let deps = AgentDeps {
        store: None,
        llm: Arc::new(StubLlm::new("ok")),
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
        cost_guard: Arc::new(crate::agent::cost_guard::CostGuard::new(
            crate::agent::cost_guard::CostGuardConfig::default(),
        )),
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
            stuck_threshold,
            repair_check_interval,
            max_repair_attempts: 2,
            use_planning: false,
            session_idle_timeout: Duration::from_secs(300),
            allow_local_tools: false,
            max_cost_per_day_cents: None,
            max_actions_per_hour: None,
            max_tool_iterations: 4,
            auto_approve_tools: false,
            default_timezone: "UTC".to_string(),
            max_tokens_per_job: 0,
        },
        deps,
        channels,
        None,
        None,
        None,
        Some(context_manager),
        None,
    )
}

#[tokio::test]
async fn agent_run_forwards_self_repair_notifications_and_shuts_down_cleanly() {
    let context_manager = Arc::new(ContextManager::new(1));
    let job_id = context_manager
        .create_job("Stuck job", "Needs recovery")
        .await
        .expect("failed to create stuck job");
    context_manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
                .expect("failed to transition job into progress");
            ctx.mark_stuck("simulated stall")
                .expect("failed to mark job stuck");
        })
        .await
        .expect("failed to update stuck job context");

    let channels = Arc::new(ChannelManager::new());
    let (channel, sender, broadcasts) = BroadcastCaptureChannel::new("test");
    channels.add(Box::new(channel)).await;

    let agent = make_test_agent(
        Arc::clone(&channels),
        Arc::clone(&context_manager),
        Duration::from_millis(10),
        Duration::ZERO,
    );
    let agent_handle = tokio::spawn(agent.run());

    let broadcast = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some((user_id, response)) = broadcasts
                .lock()
                .expect("broadcast capture should not be poisoned")
                .first()
                .cloned()
            {
                return (user_id, response);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("self-repair notification should be forwarded");

    assert_eq!(broadcast.0, "default");
    assert!(
        broadcast.1.content.starts_with("Self-Repair: Job "),
        "unexpected notification content: {}",
        broadcast.1.content
    );
    assert!(
        broadcast.1.content.contains("recovery succeeded"),
        "unexpected notification content: {}",
        broadcast.1.content
    );

    sender
        .send(IncomingMessage::new("test", "default", "/quit"))
        .await
        .expect("quit message should send successfully");

    tokio::time::timeout(Duration::from_secs(2), agent_handle)
        .await
        .expect("agent should shut down without deadlocking")
        .expect("agent task should join cleanly")
        .expect("agent run should exit successfully");
}
