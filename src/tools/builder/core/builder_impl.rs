//! Builder implementation for requirement analysis, build execution, and repair.
//!
//! This module wires [`LlmSoftwareBuilder`] into the native builder trait.
//! It owns the LLM-backed requirement analysis path, project-directory lifecycle,
//! and timeout-wrapped build execution.

use super::*;

fn extract_first_json_object(response: &str) -> Option<&str> {
    for (start, ch) in response.char_indices() {
        if ch != '{' {
            continue;
        }

        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (offset, current) in response[start..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                match current {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
                continue;
            }

            match current {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let candidate = &response[start..start + offset + current.len_utf8()];
                        if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                            return Some(candidate);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    None
}

impl NativeSoftwareBuilder for LlmSoftwareBuilder {
    async fn analyze(&self, description: &str) -> Result<BuildRequirement, AgentToolError> {
        // Use LLM to parse the description
        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());

        let prompt = format!(
            r#"Analyze this software requirement and extract structured information.

Description: {}

IMPORTANT: If this is a "tool" that the agent will use (e.g., "calendar tool", "email tool",
"API client tool"), you MUST use:
- software_type: "wasm_tool"
- language: "rust"

Only use cli_binary/script/library for software meant for human end-users, not agent tools.

Respond with a JSON object containing:
- name: A short identifier (snake_case)
- description: What the software should do
- software_type: One of "wasm_tool", "cli_binary", "library", "script", "web_service"
  (PREFER "wasm_tool" for agent-usable tools)
- language: One of "rust", "python", "typescript", "javascript", "go", "bash"
  (PREFER "rust" for wasm_tool)
- input_spec: Expected input format (optional)
- output_spec: Expected output format (optional)
- dependencies: List of external dependencies needed
- capabilities: For WASM tools, list needed capabilities (http, workspace, secrets)

JSON:"#,
            description
        );

        let ctx = ReasoningContext::new().with_message(ChatMessage::user(&prompt));

        let response = reasoning
            .respond(&ctx)
            .await
            .map_err(|e| AgentToolError::BuilderFailed(format!("Analysis failed: {}", e)))?;

        let json_str = extract_first_json_object(&response).unwrap_or(&response);

        serde_json::from_str(json_str).map_err(|e| {
            AgentToolError::BuilderFailed(format!("Failed to parse requirement: {}", e))
        })
    }

    async fn build(&self, requirement: &BuildRequirement) -> Result<BuildResult, AgentToolError> {
        // Create project directory
        let project_dir = self.config.build_dir.join(requirement.name.as_str());
        if project_dir.exists() {
            tokio::fs::remove_dir_all(&project_dir).await.map_err(|e| {
                AgentToolError::BuilderFailed(format!("Failed to clean project dir: {}", e))
            })?;
        }
        tokio::fs::create_dir_all(&project_dir).await.map_err(|e| {
            AgentToolError::BuilderFailed(format!("Failed to create project dir: {}", e))
        })?;

        // Run the build loop with timeout
        let result = tokio::time::timeout(
            self.config.timeout,
            self.execute_build_loop(requirement, &project_dir),
        )
        .await;

        match result {
            Ok(Ok(build_result)) => Ok(build_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AgentToolError::BuilderFailed("Build timed out".into())),
        }
    }

    async fn repair(
        &self,
        result: &BuildResult,
        error: &str,
    ) -> Result<BuildResult, AgentToolError> {
        // Create a new requirement with repair context
        let mut requirement = result.requirement.clone();
        requirement.description = format!(
            "{}\n\nPrevious build failed with error:\n{}\n\nFix the issues and rebuild.",
            requirement.description, error
        );

        // Rebuild (preserving project directory if it exists)
        NativeSoftwareBuilder::build(self, &requirement).await
    }
}
