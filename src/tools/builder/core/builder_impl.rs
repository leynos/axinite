use super::*;

#[async_trait]
impl SoftwareBuilder for LlmSoftwareBuilder {
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

        // Extract JSON from response
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
        let json_str = &response[json_start..json_end];

        serde_json::from_str(json_str).map_err(|e| {
            AgentToolError::BuilderFailed(format!("Failed to parse requirement: {}", e))
        })
    }

    async fn build(&self, requirement: &BuildRequirement) -> Result<BuildResult, AgentToolError> {
        // Create project directory
        let project_dir = self.config.build_dir.join(&requirement.name);
        if project_dir.exists() {
            std::fs::remove_dir_all(&project_dir).map_err(|e| {
                AgentToolError::BuilderFailed(format!("Failed to clean project dir: {}", e))
            })?;
        }
        std::fs::create_dir_all(&project_dir).map_err(|e| {
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
        self.build(&requirement).await
    }
}
