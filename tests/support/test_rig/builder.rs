//! Builder utilities for assembling a [`TestRig`] with realistic shared test
//! infrastructure.
//!
//! `TestRigBuilder` composes [`TestRig`], [`TestChannelHandle`],
//! [`InstrumentedLlm`], [`TestChannel`], and trace-backed providers such as
//! [`TraceLlm`]. Use it when a test needs a fully wired agent loop, optionally
//! replaying `LlmTrace` steps and HTTP exchanges through
//! `ReplayingHttpInterceptor`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ironclaw::agent::{Agent, AgentDeps};
use ironclaw::app::{AppBuilder, AppBuilderFlags, AppComponents};
use ironclaw::channels::web::log_layer::LogBroadcaster;
use ironclaw::config::Config;
use ironclaw::llm::recording::{HttpExchange, ReplayingHttpInterceptor};
use ironclaw::llm::{LlmProvider, SessionConfig, SessionManager};
use ironclaw::tools::Tool;

use crate::support::instrumented_llm::InstrumentedLlm;
use crate::support::test_channel::TestChannel;
use crate::support::trace_llm::{LlmTrace, TraceLlm, TraceResponse, TraceStep};

use super::{TestChannelHandle, TestRig};

/// Builder for constructing a `TestRig`.
pub struct TestRigBuilder {
    trace: Option<LlmTrace>,
    llm: Option<Arc<dyn LlmProvider>>,
    max_tool_iterations: usize,
    injection_check: bool,
    auto_approve_tools: Option<bool>,
    enable_skills: bool,
    enable_routines: bool,
    http_exchanges: Vec<HttpExchange>,
    extra_tools: Vec<Arc<dyn Tool>>,
}

// Private: register the minimal set of job tools needed by the tests.
fn register_job_tools_for_tests(
    components: &ironclaw::app::AppComponents,
    scheduler_slot: &ironclaw::tools::builtin::SchedulerSlot,
) {
    components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsConfig {
            context_manager: Arc::clone(&components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: None,
            store: components.db.clone(),
            job_event_tx: None,
            inject_tx: None,
            prompt_queue: None,
            secrets_store: None,
        });
}

impl TestRigBuilder {
    /// Create a new builder with defaults.
    pub fn new() -> Self {
        Self {
            trace: None,
            llm: None,
            max_tool_iterations: 10,
            injection_check: false,
            auto_approve_tools: None,
            enable_skills: false,
            enable_routines: false,
            http_exchanges: Vec::new(),
            extra_tools: Vec::new(),
        }
    }

    /// Set the LLM trace to replay.
    pub fn with_trace(mut self, trace: LlmTrace) -> Self {
        self.trace = Some(trace);
        self
    }

    /// Override the LLM provider directly (takes precedence over trace).
    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Set the maximum number of tool iterations per agentic loop invocation.
    pub fn with_max_tool_iterations(mut self, n: usize) -> Self {
        self.max_tool_iterations = n;
        self
    }

    /// Register additional custom tools (for example stub tools for testing).
    pub fn with_extra_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.extra_tools = tools;
        self
    }

    /// Enable prompt injection detection in the safety layer.
    pub fn with_injection_check(mut self, enable: bool) -> Self {
        self.injection_check = enable;
        self
    }

    /// Override agent-level automatic approval of `UnlessAutoApproved` tools.
    pub fn with_auto_approve_tools(mut self, enable: bool) -> Self {
        self.auto_approve_tools = Some(enable);
        self
    }

    /// Enable skill discovery and registration for this test rig.
    pub fn with_skills(mut self) -> Self {
        self.enable_skills = true;
        self
    }

    /// Enable the routines system so the scheduler is wired with a `RoutineEngine`.
    pub fn with_routines(mut self) -> Self {
        self.enable_routines = true;
        self
    }

    /// Add pre-recorded HTTP exchanges for the `ReplayingHttpInterceptor`.
    pub fn with_http_exchanges(mut self, exchanges: Vec<HttpExchange>) -> Self {
        self.http_exchanges = exchanges;
        self
    }

    fn resolve_llm(&self) -> (Arc<dyn LlmProvider>, Option<Arc<TraceLlm>>) {
        if let Some(llm) = &self.llm {
            return (Arc::clone(llm), None);
        }

        let trace = self.trace.clone().unwrap_or_else(|| {
            LlmTrace::single_turn(
                "test-rig-default",
                "(default)",
                vec![TraceStep {
                    request_hint: None,
                    response: TraceResponse::Text {
                        content: "Hello from test rig!".to_string(),
                        input_tokens: 10,
                        output_tokens: 5,
                    },
                    expected_tool_results: Vec::new(),
                }],
            )
        });
        let trace_llm = Arc::new(TraceLlm::from_trace(trace));
        (
            Arc::clone(&trace_llm) as Arc<dyn LlmProvider>,
            Some(trace_llm),
        )
    }

    fn build_http_interceptor(&self, trace: Option<&LlmTrace>) -> Option<ReplayingHttpInterceptor> {
        if !self.http_exchanges.is_empty() {
            return Some(ReplayingHttpInterceptor::new(self.http_exchanges.clone()));
        }

        let exchanges = trace
            .map(|trace| trace.http_exchanges.clone())
            .filter(|exchanges| !exchanges.is_empty())?;
        Some(ReplayingHttpInterceptor::new(exchanges))
    }

    async fn register_optional_subsystems(
        &self,
        components: &mut AppComponents,
        db: &Arc<dyn ironclaw::db::Database>,
        temp_dir: &tempfile::TempDir,
    ) {
        if let (Some(_db), Some(workspace)) = (&components.db, &components.workspace) {
            use ironclaw::agent::routine_engine::RoutineEngine;
            use ironclaw::config::RoutineConfig;

            let routine_config = RoutineConfig::default();
            let (notify_tx, _notify_rx) = tokio::sync::mpsc::channel(16);
            let engine = Arc::new(RoutineEngine::new(
                routine_config,
                Arc::clone(db),
                components.llm.clone(),
                Arc::clone(workspace),
                notify_tx,
                None,
                components.tools.clone(),
                components.safety.clone(),
            ));
            components
                .tools
                .register_routine_tools(Arc::clone(db), engine);
        }

        if self.enable_skills {
            let registry = Arc::new(std::sync::RwLock::new(
                ironclaw::skills::SkillRegistry::new(temp_dir.path().join("skills"))
                    .with_installed_dir(temp_dir.path().join("installed_skills")),
            ));
            let catalog = ironclaw::skills::catalog::shared_catalog();
            components
                .tools
                .register_skill_tools(Arc::clone(&registry), Arc::clone(&catalog));
            components.skill_registry = Some(registry);
            components.skill_catalog = Some(catalog);
        }

        for tool in &self.extra_tools {
            components.tools.register(Arc::clone(tool)).await;
        }
    }

    /// Build the test rig, creating a real agent and spawning it in the background.
    #[cfg(feature = "libsql")]
    pub async fn build(self) -> TestRig {
        use ironclaw::channels::ChannelManager;
        use ironclaw::db::Database;
        use ironclaw::db::libsql::LibSqlBackend;

        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("test_rig.db");
        let backend = LibSqlBackend::new_local(&db_path)
            .await
            .expect("failed to create test LibSqlBackend");
        backend
            .run_migrations()
            .await
            .expect("failed to run migrations");
        let db: Arc<dyn ironclaw::db::Database> = Arc::new(backend);

        let skills_dir = temp_dir.path().join("skills");
        let installed_skills_dir = temp_dir.path().join("installed_skills");
        let _ = tokio::fs::create_dir_all(&skills_dir).await;
        let _ = tokio::fs::create_dir_all(&installed_skills_dir).await;
        let mut config = Config::for_testing(db_path, skills_dir, installed_skills_dir);
        config.agent.max_tool_iterations = self.max_tool_iterations;
        config.safety.injection_check_enabled = self.injection_check;
        config.skills.enabled = self.enable_skills;
        if let Some(value) = self.auto_approve_tools {
            config.agent.auto_approve_tools = value;
        }

        let session = Arc::new(SessionManager::new(SessionConfig::default()));
        let log_broadcaster = Arc::new(LogBroadcaster::new());
        let (base_llm, trace_llm_ref) = self.resolve_llm();
        let instrumented = Arc::new(InstrumentedLlm::new(base_llm));
        let llm: Arc<dyn LlmProvider> = Arc::clone(&instrumented) as Arc<dyn LlmProvider>;

        let mut builder = AppBuilder::new(
            config,
            AppBuilderFlags::default(),
            None,
            session,
            log_broadcaster,
        );
        builder.with_database(Arc::clone(&db));
        builder.with_llm(llm);
        let mut components = builder
            .build_all()
            .await
            .expect("AppBuilder::build_all() failed in test rig");

        let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
            Arc::new(tokio::sync::RwLock::new(None));

        register_job_tools_for_tests(&components, &scheduler_slot);
        self.register_optional_subsystems(&mut components, &db, &temp_dir)
            .await;

        let db_ref = components.db.clone().expect("test rig requires a database");
        let workspace_ref = components.workspace.clone();
        let http_replay = self.build_http_interceptor(self.trace.as_ref());

        let deps = AgentDeps {
            store: components.db,
            llm: components.llm,
            cheap_llm: components.cheap_llm,
            safety: components.safety,
            tools: components.tools,
            workspace: components.workspace,
            extension_manager: components.extension_manager,
            skill_registry: components.skill_registry,
            skill_catalog: components.skill_catalog,
            skills_config: components.config.skills.clone(),
            hooks: components.hooks,
            cost_guard: components.cost_guard,
            sse_tx: None,
            http_interceptor: http_replay.map(|interceptor| {
                Arc::new(interceptor) as Arc<dyn ironclaw::llm::recording::HttpInterceptor>
            }),
            transcription: None,
            document_extraction: None,
        };

        let test_channel = Arc::new(TestChannel::new());
        let handle = TestChannelHandle::new(Arc::clone(&test_channel));
        let channel_manager = ChannelManager::new();
        channel_manager.add(Box::new(handle)).await;
        let channels = Arc::new(channel_manager);

        deps.tools
            .register_message_tools(Arc::clone(&channels))
            .await;

        let routine_config = if self.enable_routines {
            Some(ironclaw::config::RoutineConfig {
                enabled: true,
                cron_check_interval_secs: 60,
                max_concurrent_routines: 3,
                default_cooldown_secs: 300,
                max_lightweight_tokens: 4096,
                lightweight_tools_enabled: true,
                lightweight_max_iterations: 3,
            })
        } else {
            None
        };
        let agent = Agent::new(
            components.config.agent.clone(),
            deps,
            channels,
            None,
            None,
            routine_config,
            Some(Arc::clone(&components.context_manager)),
            None,
        );

        *scheduler_slot.write().await = Some(agent.scheduler());

        let agent_handle = tokio::spawn(async move {
            if let Err(error) = agent.run().await {
                eprintln!("[TestRig] Agent exited with error: {error}");
            }
        });

        if let Some(rx) = test_channel.take_ready_rx().await {
            let _ = tokio::time::timeout(Duration::from_secs(5), rx).await;
        }

        TestRig {
            channel: test_channel,
            instrumented_llm: instrumented,
            start_time: Instant::now(),
            max_tool_iterations: self.max_tool_iterations,
            agent_handle: Some(agent_handle),
            db: db_ref,
            workspace: workspace_ref,
            trace_llm: trace_llm_ref,
            _temp_dir: temp_dir,
        }
    }
}

impl Default for TestRigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
