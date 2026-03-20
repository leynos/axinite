use super::*;
use crate::llm::ToolCall;

const PLANNING_TEXT_LIMIT: u32 = 2;
const COMPLETION_MARKERS: [&str; 5] = [
    "build complete",
    "completed successfully",
    "successfully built",
    "build succeeded",
    "all tests pass",
];
const FAILURE_MARKERS: [&str; 3] = ["error:", "error[", "failed"];

struct BuildLoopInputs {
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

    async fn prepare_reasoning_context(
        &self,
        requirement: &BuildRequirement,
    ) -> (Reasoning, ReasoningContext, Vec<BuildLog>) {
        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());

        let tool_defs = self.get_build_tools().await;
        let mut reason_ctx = ReasoningContext::new().with_tools(tool_defs);

        reason_ctx
            .messages
            .push(ChatMessage::system(self.build_system_prompt(requirement)));

        let logs = vec![BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Analyzing,
            message: "Starting build process".into(),
            details: None,
        }];

        (reasoning, reason_ctx, logs)
    }

    fn fail_max_iterations(
        &self,
        inputs: &BuildLoopInputs,
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
        inputs: &BuildLoopInputs,
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
        inputs: &BuildLoopInputs,
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

    async fn handle_text_response(
        &self,
        response: String,
        inputs: &BuildLoopInputs,
        state: &mut BuildLoopState,
        reason_ctx: &mut ReasoningContext,
    ) -> std::ops::ControlFlow<BuildResult> {
        reason_ctx.messages.push(ChatMessage::assistant(&response));

        if !state.tools_executed {
            state.consecutive_text_responses += 1;

            if state.consecutive_text_responses >= PLANNING_TEXT_LIMIT {
                return std::ops::ControlFlow::Break(self.fail_planning_stuck(inputs, state));
            }

            Self::push_force_tool_use(reason_ctx, state.consecutive_text_responses);
            return std::ops::ControlFlow::Continue(());
        }

        state.consecutive_text_responses = 0;

        if is_completion_signal(&response.to_lowercase()) {
            return std::ops::ControlFlow::Break(
                self.build_success_result(inputs, state, response).await,
            );
        }

        reason_ctx
            .messages
            .push(ChatMessage::user("Continue with the next step."));
        std::ops::ControlFlow::Continue(())
    }

    async fn handle_tool_calls(
        &self,
        inputs: &BuildLoopInputs,
        reason_ctx: &mut ReasoningContext,
        state: &mut BuildLoopState,
        tool_calls: Vec<ToolCall>,
        content: Option<String>,
    ) {
        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        for tc in tool_calls {
            state.logs.push(BuildLog {
                timestamp: Utc::now(),
                phase: state.current_phase,
                message: format!("Executing: {}", tc.name),
                details: Some(format!("{:?}", tc.arguments)),
            });

            match self
                .execute_build_tool(&tc.name, &tc.arguments, &inputs.project_dir)
                .await
            {
                Ok(output) => {
                    let output_str =
                        serde_json::to_string_pretty(&output.result).unwrap_or_default();

                    reason_ctx.messages.push(ChatMessage::tool_result(
                        &tc.id,
                        &tc.name,
                        output_str.clone(),
                    ));

                    state.current_phase =
                        infer_phase_from_tool(tc.name.as_str(), &tc.arguments, state.current_phase);

                    let output_lower = output_str.to_lowercase();
                    if has_failure_marker(&output_lower) {
                        state.last_error = Some(output_str);
                        state.current_phase = BuildPhase::Fixing;
                    }
                }
                Err(e) => {
                    let error_msg = format!("Tool error: {}", e);
                    state.last_error = Some(error_msg.clone());

                    reason_ctx.messages.push(ChatMessage::tool_result(
                        &tc.id,
                        &tc.name,
                        format!("Error: {}", e),
                    ));

                    state.logs.push(BuildLog {
                        timestamp: Utc::now(),
                        phase: BuildPhase::Fixing,
                        message: "Tool execution failed".into(),
                        details: Some(error_msg),
                    });

                    state.current_phase = BuildPhase::Fixing;
                }
            }
        }
    }

    /// Execute the build loop.
    pub(super) async fn execute_build_loop(
        &self,
        requirement: &BuildRequirement,
        project_dir: &Path,
    ) -> Result<BuildResult, AgentToolError> {
        let build_id = Uuid::new_v4();
        let iteration = 0u32;
        let (reasoning, mut reason_ctx, logs) = self.prepare_reasoning_context(requirement).await;
        let started_at = logs
            .first()
            .map(|log| log.timestamp)
            .unwrap_or_else(Utc::now);
        let inputs = BuildLoopInputs {
            build_id,
            started_at,
            requirement: requirement.clone(),
            project_dir: project_dir.to_path_buf(),
        };
        let mut state = BuildLoopState {
            logs,
            iteration,
            current_phase: BuildPhase::Scaffolding,
            last_error: None,
            tools_executed: false,
            consecutive_text_responses: 0,
        };

        // Add initial user message - directive to force immediate tool use
        reason_ctx.messages.push(ChatMessage::user(format!(
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
            reason_ctx.available_tools = self.get_build_tools().await;

            // Get response from LLM (may be text or tool calls)
            let result = reasoning
                .respond_with_tools(&reason_ctx)
                .await
                .map_err(|e| {
                    AgentToolError::BuilderFailed(format!("LLM response failed: {}", e))
                })?;

            match result.result {
                RespondResult::Text(response) => {
                    match self
                        .handle_text_response(response, &inputs, &mut state, &mut reason_ctx)
                        .await
                    {
                        std::ops::ControlFlow::Break(done) => return Ok(done),
                        std::ops::ControlFlow::Continue(()) => {}
                    }
                }
                RespondResult::ToolCalls {
                    tool_calls,
                    content,
                } => {
                    state.tools_executed = true;
                    self.handle_tool_calls(
                        &inputs,
                        &mut reason_ctx,
                        &mut state,
                        tool_calls,
                        content,
                    )
                    .await;
                }
            }
        }
    }

    /// Execute a build tool.
    async fn execute_build_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        project_dir: &Path,
    ) -> Result<ToolOutput, ToolError> {
        let ctx = JobContext::default();
        let project_root = project_dir.to_path_buf();

        match tool_name {
            "read_file" => {
                let tool = crate::tools::builtin::ReadFileTool::new().with_base_dir(project_root);
                tool.execute(params.clone(), &ctx).await
            }
            "write_file" => {
                let tool = crate::tools::builtin::WriteFileTool::new().with_base_dir(project_root);
                tool.execute(params.clone(), &ctx).await
            }
            "list_dir" => {
                let tool = crate::tools::builtin::ListDirTool::new().with_base_dir(project_root);
                tool.execute(params.clone(), &ctx).await
            }
            "apply_patch" => {
                let tool = crate::tools::builtin::ApplyPatchTool::new().with_base_dir(project_root);
                tool.execute(params.clone(), &ctx).await
            }
            "shell" => {
                let tool = crate::tools::builtin::ShellTool::new().with_working_dir(project_root);
                tool.execute(params.clone(), &ctx).await
            }
            _ => {
                let tool = self.tools.get(tool_name).await.ok_or_else(|| {
                    ToolError::ExecutionFailed(format!("Tool not found: {}", tool_name))
                })?;
                tool.execute(params.clone(), &ctx).await
            }
        }
    }

    /// Find the build artifact based on project type.
    async fn find_artifact(&self, requirement: &BuildRequirement, project_dir: &Path) -> PathBuf {
        match (&requirement.software_type, &requirement.language) {
            (SoftwareType::WasmTool, Language::Rust) => {
                // WASM output location
                crate::tools::wasm::wasm_artifact_path(
                    project_dir,
                    &requirement.name.to_string().replace('-', "_"),
                )
            }
            (SoftwareType::CliBinary, Language::Rust) => project_dir.join(format!(
                "target/release/{}",
                requirement.name.to_string().replace('-', "_")
            )),
            (SoftwareType::Script, Language::Python) => {
                project_dir.join(format!("{}.py", requirement.name))
            }
            (SoftwareType::Script, Language::Bash) => {
                project_dir.join(format!("{}.sh", requirement.name))
            }
            _ => project_dir.to_path_buf(),
        }
    }
}
