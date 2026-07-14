//! Response handling for the software builder's build loop: text and
//! tool-call handling, build tool execution, and artifact discovery.

use super::*;
use crate::llm::ToolCall;

impl LlmSoftwareBuilder {
    pub(super) async fn handle_text_response(
        &self,
        response: String,
        inputs: &BuildLoopParams,
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

    pub(super) async fn handle_tool_calls(
        &self,
        inputs: &BuildLoopParams,
        bundle: &mut ReasoningBundle,
        state: &mut BuildLoopState,
        tool_calls: Vec<ToolCall>,
        content: Option<String>,
    ) {
        bundle
            .ctx
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

                    bundle.ctx.messages.push(ChatMessage::tool_result(
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

                    bundle.ctx.messages.push(ChatMessage::tool_result(
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

    /// Execute a build tool.
    pub(super) async fn execute_build_tool(
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
                crate::tools::NativeTool::execute(&tool, params.clone(), &ctx).await
            }
            "write_file" => {
                let tool = crate::tools::builtin::WriteFileTool::new().with_base_dir(project_root);
                crate::tools::NativeTool::execute(&tool, params.clone(), &ctx).await
            }
            "list_dir" => {
                let tool = crate::tools::builtin::ListDirTool::new().with_base_dir(project_root);
                crate::tools::NativeTool::execute(&tool, params.clone(), &ctx).await
            }
            "apply_patch" => {
                let tool = crate::tools::builtin::ApplyPatchTool::new().with_base_dir(project_root);
                crate::tools::NativeTool::execute(&tool, params.clone(), &ctx).await
            }
            "shell" => {
                let tool = crate::tools::builtin::ShellTool::new().with_working_dir(project_root);
                crate::tools::NativeTool::execute(&tool, params.clone(), &ctx).await
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
    pub(super) async fn find_artifact(
        &self,
        requirement: &BuildRequirement,
        project_dir: &Path,
    ) -> PathBuf {
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
