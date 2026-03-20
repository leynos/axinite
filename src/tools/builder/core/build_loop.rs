use super::*;
use crate::llm::ToolCall;

const PLANNING_TEXT_LIMIT: u32 = 2;
const COMPLETION_MARKERS: [&str; 4] = [
    "build complete",
    "successfully built",
    "all tests pass",
    "complete",
];
const FAILURE_MARKERS: [&str; 3] = ["error:", "error[", "failed"];

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

    // The helper signature is fixed by the review request.
    #[allow(clippy::too_many_arguments)]
    fn fail_max_iterations(
        &self,
        build_id: Uuid,
        requirement: &BuildRequirement,
        project_dir: &Path,
        started_at: DateTime<Utc>,
        iteration: u32,
        last_error: Option<String>,
        logs: &mut Vec<BuildLog>,
    ) -> BuildResult {
        logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Failed,
            message: "Maximum iterations exceeded".into(),
            details: last_error,
        });

        BuildResult {
            build_id,
            requirement: requirement.clone(),
            artifact_path: project_dir.to_path_buf(),
            logs: logs.clone(),
            success: false,
            error: Some("Maximum iterations exceeded".into()),
            started_at,
            completed_at: Utc::now(),
            iterations: iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    // The helper signature is fixed by the review request.
    #[allow(clippy::too_many_arguments)]
    fn fail_planning_stuck(
        &self,
        build_id: Uuid,
        requirement: &BuildRequirement,
        project_dir: &Path,
        started_at: DateTime<Utc>,
        iteration: u32,
        logs: &mut Vec<BuildLog>,
        consecutive_text_responses: u32,
    ) -> BuildResult {
        logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Failed,
            message: "Builder stuck in planning mode".into(),
            details: Some(format!(
                "LLM returned {} consecutive text responses without calling tools. \
                 Try a more specific requirement.",
                consecutive_text_responses
            )),
        });

        BuildResult {
            build_id,
            requirement: requirement.clone(),
            artifact_path: project_dir.to_path_buf(),
            logs: logs.clone(),
            success: false,
            error: Some("LLM not executing tools - stuck in planning mode".into()),
            started_at,
            completed_at: Utc::now(),
            iterations: iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    async fn build_success_result(
        &self,
        build_id: Uuid,
        requirement: &BuildRequirement,
        project_dir: &Path,
        logs: &mut Vec<BuildLog>,
        iteration: u32,
        response: String,
    ) -> BuildResult {
        logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Complete,
            message: "Build completed successfully".into(),
            details: Some(response),
        });

        let artifact_path = self.find_artifact(requirement, project_dir).await;

        BuildResult {
            build_id,
            requirement: requirement.clone(),
            artifact_path,
            logs: logs.clone(),
            success: true,
            error: None,
            started_at: logs
                .first()
                .map(|log| log.timestamp)
                .unwrap_or_else(Utc::now),
            completed_at: Utc::now(),
            iterations: iteration,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    // The helper signature is fixed by the review request.
    #[allow(clippy::too_many_arguments)]
    async fn handle_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        content: Option<String>,
        project_dir: &Path,
        reason_ctx: &mut ReasoningContext,
        logs: &mut Vec<BuildLog>,
        current_phase: &mut BuildPhase,
        last_error: &mut Option<String>,
    ) {
        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        for tc in tool_calls {
            logs.push(BuildLog {
                timestamp: Utc::now(),
                phase: *current_phase,
                message: format!("Executing: {}", tc.name),
                details: Some(format!("{:?}", tc.arguments)),
            });

            match self
                .execute_build_tool(&tc.name, &tc.arguments, project_dir)
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

                    *current_phase =
                        infer_phase_from_tool(tc.name.as_str(), &tc.arguments, *current_phase);

                    let output_lower = output_str.to_lowercase();
                    if has_failure_marker(&output_lower) {
                        *last_error = Some(output_str);
                        *current_phase = BuildPhase::Fixing;
                    }
                }
                Err(e) => {
                    let error_msg = format!("Tool error: {}", e);
                    *last_error = Some(error_msg.clone());

                    reason_ctx.messages.push(ChatMessage::tool_result(
                        &tc.id,
                        &tc.name,
                        format!("Error: {}", e),
                    ));

                    logs.push(BuildLog {
                        timestamp: Utc::now(),
                        phase: BuildPhase::Fixing,
                        message: "Tool execution failed".into(),
                        details: Some(error_msg),
                    });

                    *current_phase = BuildPhase::Fixing;
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
        let mut iteration = 0u32;
        let (reasoning, mut reason_ctx, mut logs) =
            self.prepare_reasoning_context(requirement).await;
        let started_at = logs
            .first()
            .map(|log| log.timestamp)
            .unwrap_or_else(Utc::now);

        // Add initial user message - directive to force immediate tool use
        reason_ctx.messages.push(ChatMessage::user(format!(
            "Build the {} in directory: {}\n\n\
             Requirements:\n- {}\n\n\
             IMPORTANT: Use the write_file tool NOW to create Cargo.toml. \
             Do not explain, plan, or output JSON—immediately call write_file.",
            requirement.name,
            project_dir.display(),
            requirement.description
        )));

        // Main build loop
        let mut current_phase = BuildPhase::Scaffolding;
        let mut last_error: Option<String> = None;
        let mut tools_executed = false;
        let mut consecutive_text_responses = 0u32;

        loop {
            iteration += 1;

            if iteration > self.config.max_iterations {
                return Ok(self.fail_max_iterations(
                    build_id,
                    requirement,
                    project_dir,
                    started_at,
                    iteration,
                    last_error.clone(),
                    &mut logs,
                ));
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
                    reason_ctx.messages.push(ChatMessage::assistant(&response));

                    if !tools_executed {
                        consecutive_text_responses += 1;

                        if consecutive_text_responses >= PLANNING_TEXT_LIMIT {
                            return Ok(self.fail_planning_stuck(
                                build_id,
                                requirement,
                                project_dir,
                                started_at,
                                iteration,
                                &mut logs,
                                consecutive_text_responses,
                            ));
                        }

                        Self::push_force_tool_use(&mut reason_ctx, consecutive_text_responses);
                        continue;
                    }

                    consecutive_text_responses = 0;

                    let response_lower = response.to_lowercase();
                    if is_completion_signal(&response_lower) {
                        return Ok(self
                            .build_success_result(
                                build_id,
                                requirement,
                                project_dir,
                                &mut logs,
                                iteration,
                                response,
                            )
                            .await);
                    }

                    reason_ctx
                        .messages
                        .push(ChatMessage::user("Continue with the next step."));
                }
                RespondResult::ToolCalls {
                    tool_calls,
                    content,
                } => {
                    tools_executed = true;
                    self.handle_tool_calls(
                        tool_calls,
                        content,
                        project_dir,
                        &mut reason_ctx,
                        &mut logs,
                        &mut current_phase,
                        &mut last_error,
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
        _project_dir: &Path,
    ) -> Result<ToolOutput, ToolError> {
        let tool =
            self.tools.get(tool_name).await.ok_or_else(|| {
                ToolError::ExecutionFailed(format!("Tool not found: {}", tool_name))
            })?;

        // Execute with a dummy context (build tools don't need job context)
        let ctx = JobContext::default();
        tool.execute(params.clone(), &ctx).await
    }

    /// Find the build artifact based on project type.
    async fn find_artifact(&self, requirement: &BuildRequirement, project_dir: &Path) -> PathBuf {
        match (&requirement.software_type, &requirement.language) {
            (SoftwareType::WasmTool, Language::Rust) => {
                // WASM output location
                crate::tools::wasm::wasm_artifact_path(
                    project_dir,
                    &requirement.name.replace('-', "_"),
                )
            }
            (SoftwareType::CliBinary, Language::Rust) => project_dir.join(format!(
                "target/release/{}",
                requirement.name.replace('-', "_")
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
