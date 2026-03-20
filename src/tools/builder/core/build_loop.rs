use super::*;
use crate::llm::ToolCall;

impl LlmSoftwareBuilder {
    #[inline]
    fn is_completion_signal(response: &str) -> bool {
        let response_lower = response.to_lowercase();
        response_lower.contains("build complete")
            || response_lower.contains("successfully built")
            || response_lower.contains("all tests pass")
            || response_lower.contains("complete")
    }

    #[inline]
    fn has_build_error(output: &str) -> bool {
        let output_lower = output.to_lowercase();
        output_lower.contains("error:")
            || output_lower.contains("error[")
            || output_lower.contains("failed")
    }

    #[inline]
    fn next_phase(current: BuildPhase, tool_name: &str, args: &serde_json::Value) -> BuildPhase {
        match tool_name {
            "write_file" => BuildPhase::Implementing,
            "shell" if args.to_string().contains("build") => BuildPhase::Building,
            "shell" if args.to_string().contains("test") => BuildPhase::Testing,
            _ => current,
        }
    }

    #[inline]
    fn force_tool_use_prompt(ctx: &mut ReasoningContext) {
        ctx.messages.push(ChatMessage::user(
            "STOP. Do NOT output text, JSON specs, or explanations. \
             Call the write_file tool RIGHT NOW to create Cargo.toml. \
             Just call the tool—no commentary.",
        ));
    }

    fn make_success_result(
        build_id: Uuid,
        requirement: &BuildRequirement,
        artifact_path: PathBuf,
        logs: Vec<BuildLog>,
        started_at: DateTime<Utc>,
        iterations: i32,
    ) -> BuildResult {
        BuildResult {
            build_id,
            requirement: requirement.clone(),
            artifact_path,
            logs,
            success: true,
            error: None,
            started_at,
            completed_at: Utc::now(),
            iterations: iterations as u32,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    fn make_failure_result(
        build_id: Uuid,
        requirement: &BuildRequirement,
        artifact_path: PathBuf,
        logs: Vec<BuildLog>,
        started_at: DateTime<Utc>,
        iterations: i32,
        reason: &str,
    ) -> BuildResult {
        BuildResult {
            build_id,
            requirement: requirement.clone(),
            artifact_path,
            logs,
            success: false,
            error: Some(reason.into()),
            started_at,
            completed_at: Utc::now(),
            iterations: iterations as u32,
            validation_warnings: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            registered: false,
        }
    }

    async fn handle_tool_call(
        &self,
        tc: ToolCall,
        phase: BuildPhase,
        reason_ctx: &mut ReasoningContext,
        logs: &mut Vec<BuildLog>,
    ) -> (BuildPhase, Option<String>) {
        logs.push(BuildLog {
            timestamp: Utc::now(),
            phase,
            message: format!("Executing: {}", tc.name),
            details: Some(format!("{:?}", tc.arguments)),
        });

        match self
            .execute_build_tool(&tc.name, &tc.arguments, Path::new("."))
            .await
        {
            Ok(output) => {
                let output_str = serde_json::to_string_pretty(&output.result).unwrap_or_default();

                reason_ctx.messages.push(ChatMessage::tool_result(
                    &tc.id,
                    &tc.name,
                    output_str.clone(),
                ));

                let mut next_phase = Self::next_phase(phase, &tc.name, &tc.arguments);
                if Self::has_build_error(&output_str) {
                    next_phase = BuildPhase::Fixing;
                    return (next_phase, Some(output_str));
                }

                (next_phase, None)
            }
            Err(e) => {
                let error_msg = format!("Tool error: {}", e);

                reason_ctx.messages.push(ChatMessage::tool_result(
                    &tc.id,
                    &tc.name,
                    format!("Error: {}", e),
                ));

                logs.push(BuildLog {
                    timestamp: Utc::now(),
                    phase: BuildPhase::Fixing,
                    message: "Tool execution failed".into(),
                    details: Some(error_msg.clone()),
                });

                (BuildPhase::Fixing, Some(error_msg))
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
        let started_at = Utc::now();
        let mut logs = Vec::new();
        let mut iteration = 0i32;

        // Create reasoning engine
        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());

        // Build initial context
        let tool_defs = self.get_build_tools().await;
        let mut reason_ctx = ReasoningContext::new().with_tools(tool_defs);

        // Add system prompt
        reason_ctx
            .messages
            .push(ChatMessage::system(self.build_system_prompt(requirement)));

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

        logs.push(BuildLog {
            timestamp: Utc::now(),
            phase: BuildPhase::Analyzing,
            message: "Starting build process".into(),
            details: None,
        });

        // Main build loop
        let mut current_phase = BuildPhase::Scaffolding;
        let mut last_error: Option<String> = None;
        let mut tools_executed = false;
        let mut consecutive_text_responses = 0;

        loop {
            iteration += 1;

            if iteration > self.config.max_iterations as i32 {
                logs.push(BuildLog {
                    timestamp: Utc::now(),
                    phase: BuildPhase::Failed,
                    message: "Maximum iterations exceeded".into(),
                    details: last_error.clone(),
                });

                return Ok(Self::make_failure_result(
                    build_id,
                    requirement,
                    project_dir.to_path_buf(),
                    logs,
                    started_at,
                    iteration,
                    "Maximum iterations exceeded",
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

                    // If tools haven't been executed yet, we're stuck in planning mode
                    if !tools_executed {
                        consecutive_text_responses += 1;

                        if consecutive_text_responses >= 2 {
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

                            return Ok(Self::make_failure_result(
                                build_id,
                                requirement,
                                project_dir.to_path_buf(),
                                logs,
                                started_at,
                                iteration,
                                "LLM not executing tools - stuck in planning mode",
                            ));
                        }

                        tracing::debug!(
                            "Builder: no tools executed (text response #{}/2), forcing tool use",
                            consecutive_text_responses
                        );
                        Self::force_tool_use_prompt(&mut reason_ctx);
                        continue;
                    }

                    consecutive_text_responses = 0;

                    if Self::is_completion_signal(&response) {
                        logs.push(BuildLog {
                            timestamp: Utc::now(),
                            phase: BuildPhase::Complete,
                            message: "Build completed successfully".into(),
                            details: Some(response),
                        });

                        let artifact_path = self.find_artifact(requirement, project_dir).await;

                        return Ok(Self::make_success_result(
                            build_id,
                            requirement,
                            artifact_path,
                            logs,
                            started_at,
                            iteration,
                        ));
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

                    // Add assistant message with tool_calls (OpenAI protocol)
                    reason_ctx
                        .messages
                        .push(ChatMessage::assistant_with_tool_calls(
                            content,
                            tool_calls.clone(),
                        ));

                    for tc in tool_calls {
                        let (phase, error) = self
                            .handle_tool_call(tc, current_phase, &mut reason_ctx, &mut logs)
                            .await;
                        current_phase = phase;
                        if error.is_some() {
                            last_error = error;
                        }
                    }
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
