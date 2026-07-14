//! Iterative build loop for the software builder, tracking phases and
//! detecting completion or failure signals in LLM responses.

use super::*;
mod responses;

const PLANNING_TEXT_LIMIT: u32 = 2;
const COMPLETION_MARKERS: [&str; 5] = [
    "build complete",
    "completed successfully",
    "successfully built",
    "build succeeded",
    "all tests pass",
];
const FAILURE_MARKERS: [&str; 3] = ["error:", "error[", "failed"];

struct BuildLoopParams {
    build_id: Uuid,
    started_at: DateTime<Utc>,
    requirement: BuildRequirement,
    project_dir: PathBuf,
}

struct BuildLoopState {
    logs: Vec<BuildLog>,
    iteration: u32,
    current_phase: BuildPhase,
    tools_executed: bool,
    consecutive_text_responses: u32,
    last_error: Option<String>,
}

struct ReasoningBundle {
    reasoning: Reasoning,
    ctx: ReasoningContext,
}

fn is_completion_signal(lower: &str) -> bool {
    COMPLETION_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

fn has_failure_marker(lower: &str) -> bool {
    FAILURE_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn infer_phase_from_tool(name: &str, args: &serde_json::Value, current: BuildPhase) -> BuildPhase {
    match name {
        "write_file" => BuildPhase::Implementing,
        "shell" => {
            let text = args.to_string();
            if text.contains("build") {
                BuildPhase::Building
            } else if text.contains("test") {
                BuildPhase::Testing
            } else {
                current
            }
        }
        _ => current,
    }
}

impl LlmSoftwareBuilder {
    #[inline]
    fn push_force_tool_use(reason_ctx: &mut ReasoningContext, n: u32) {
        tracing::debug!(
            "Builder: no tools executed (text response #{}/{}), forcing tool use",
            n,
            PLANNING_TEXT_LIMIT
        );

        reason_ctx.messages.push(ChatMessage::user(
            "STOP. Do NOT output text, JSON specs, or explanations. \
             Call the write_file tool RIGHT NOW to create Cargo.toml. \
             Just call the tool—no commentary.",
        ));
    }

    async fn prepare_reasoning_context(&self, inputs: &BuildLoopParams) -> ReasoningBundle {
        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());

        let tool_defs = self.get_build_tools().await;
        let mut reason_ctx = ReasoningContext::new().with_tools(tool_defs);

        reason_ctx.messages.push(ChatMessage::system(
            self.build_system_prompt(&inputs.requirement),
        ));

        ReasoningBundle {
            reasoning,
            ctx: reason_ctx,
        }
    }

    fn fail_max_iterations(
        &self,
        inputs: &BuildLoopParams,
        state: &mut BuildLoopState,
    ) -> BuildResult {
        state.logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Failed,
            message: "Maximum iterations exceeded".into(),
            details: state.last_error.clone(),
        });

        BuildResult {
            build_id: inputs.build_id,
            requirement: inputs.requirement.clone(),
            artifact_path: inputs.project_dir.clone(),
            logs: state.logs.clone(),
            success: false,
            error: Some("Maximum iterations exceeded".into()),
            started_at: inputs.started_at,
            completed_at: Utc::now(),
            iterations: state.iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    fn fail_planning_stuck(
        &self,
        inputs: &BuildLoopParams,
        state: &mut BuildLoopState,
    ) -> BuildResult {
        state.logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Failed,
            message: "Builder stuck in planning mode".into(),
            details: Some(format!(
                "LLM returned {} consecutive text responses without calling tools. \
                 Try a more specific requirement.",
                state.consecutive_text_responses
            )),
        });

        BuildResult {
            build_id: inputs.build_id,
            requirement: inputs.requirement.clone(),
            artifact_path: inputs.project_dir.clone(),
            logs: state.logs.clone(),
            success: false,
            error: Some("LLM not executing tools - stuck in planning mode".into()),
            started_at: inputs.started_at,
            completed_at: Utc::now(),
            iterations: state.iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    async fn build_success_result(
        &self,
        inputs: &BuildLoopParams,
        state: &mut BuildLoopState,
        response: String,
    ) -> BuildResult {
        state.logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Complete,
            message: "Build completed successfully".into(),
            details: Some(response),
        });

        let artifact_path = self
            .find_artifact(&inputs.requirement, &inputs.project_dir)
            .await;

        BuildResult {
            build_id: inputs.build_id,
            requirement: inputs.requirement.clone(),
            artifact_path,
            logs: state.logs.clone(),
            success: true,
            error: None,
            started_at: state
                .logs
                .first()
                .map(|log| log.timestamp)
                .unwrap_or_else(Utc::now),
            completed_at: Utc::now(),
            iterations: state.iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    /// Execute the build loop.
    pub(super) async fn execute_build_loop(
        &self,
        requirement: &BuildRequirement,
        project_dir: &Path,
    ) -> Result<BuildResult, AgentToolError> {
        let inputs = BuildLoopParams {
            build_id: Uuid::new_v4(),
            started_at: Utc::now(),
            requirement: requirement.clone(),
            project_dir: project_dir.to_path_buf(),
        };
        let mut state = BuildLoopState {
            logs: vec![BuildLog {
                timestamp: inputs.started_at,
                phase: BuildPhase::Analyzing,
                message: "Starting build process".into(),
                details: None,
            }],
            iteration: 0,
            current_phase: BuildPhase::Scaffolding,
            tools_executed: false,
            consecutive_text_responses: 0,
            last_error: None,
        };
        let mut bundle = self.prepare_reasoning_context(&inputs).await;

        // Add initial user message - directive to force immediate tool use
        bundle.ctx.messages.push(ChatMessage::user(format!(
            "Build the {} in directory: {}\n\n\
             Requirements:\n- {}\n\n\
             IMPORTANT: Use the write_file tool NOW to create Cargo.toml. \
             Do not explain, plan, or output JSON—immediately call write_file.",
            inputs.requirement.name,
            inputs.project_dir.display(),
            inputs.requirement.description
        )));

        loop {
            state.iteration += 1;

            if state.iteration > self.config.max_iterations {
                return Ok(self.fail_max_iterations(&inputs, &mut state));
            }

            // Refresh tool definitions each iteration
            bundle.ctx.available_tools = self.get_build_tools().await;

            // Get response from LLM (may be text or tool calls)
            let result = bundle
                .reasoning
                .respond_with_tools(&bundle.ctx)
                .await
                .map_err(|e| {
                    AgentToolError::BuilderFailed(format!("LLM response failed: {}", e))
                })?;

            match result.result {
                RespondResult::Text(response) => {
                    let text_ctx = responses::TextResponseContext {
                        inputs: &inputs,
                        state: &mut state,
                        reason_ctx: &mut bundle.ctx,
                    };
                    match self.handle_text_response(response, text_ctx).await {
                        std::ops::ControlFlow::Break(done) => return Ok(done),
                        std::ops::ControlFlow::Continue(()) => {}
                    }
                }
                RespondResult::ToolCalls {
                    tool_calls,
                    content,
                } => {
                    state.tools_executed = true;
                    self.handle_tool_calls(&inputs, &mut bundle, &mut state, tool_calls, content)
                        .await;
                }
            }
        }
    }
}
