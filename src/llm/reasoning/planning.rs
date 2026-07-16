//! Planning, tool selection, and success evaluation flows for the
//! `Reasoning` engine, including JSON extraction from LLM responses.

use crate::llm::error::LlmError;
use crate::llm::{ChatMessage, CompletionRequest, Role, ToolCompletionRequest};

use super::{
    ActionPlan, Reasoning, ReasoningContext, SuccessEvaluation, ToolSelection, clean_response,
    merge_system_messages, truncate_at_tool_tags,
};

impl Reasoning {
    /// Generate a plan for completing a goal.
    pub async fn plan(&self, context: &ReasoningContext) -> Result<ActionPlan, LlmError> {
        let system_prompt = self.build_planning_prompt(context);

        let system_prompt = merge_system_messages(system_prompt, &context.messages);
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(
            context
                .messages
                .iter()
                .filter(|m| m.role != Role::System)
                .cloned(),
        );

        if let Some(ref job) = context.job_description {
            messages.push(ChatMessage::user(format!(
                "Please create a plan to complete this job:\n\n{}",
                job
            )));
        }

        let request = CompletionRequest::new(messages)
            .with_max_tokens(2048)
            .with_temperature(0.3);

        let response = self.llm.complete(request).await?;

        // Clean reasoning model artefacts before parsing JSON.
        // Pre-truncate at tool tags to avoid strip_xml_tag discarding
        // content after unclosed tags (issue #789).
        let pre_truncated = truncate_at_tool_tags(&response.content);
        let cleaned = clean_response(&pre_truncated);
        self.parse_plan(&cleaned)
    }

    /// Select the best tool for the current situation.
    pub async fn select_tool(
        &self,
        context: &ReasoningContext,
    ) -> Result<Option<ToolSelection>, LlmError> {
        let tools = self.select_tools(context).await?;
        Ok(tools.into_iter().next())
    }

    /// Select tools to execute (may return multiple for parallel execution).
    ///
    /// The LLM may return multiple tool calls if it determines they can be
    /// executed in parallel. This enables more efficient job completion.
    pub async fn select_tools(
        &self,
        context: &ReasoningContext,
    ) -> Result<Vec<ToolSelection>, LlmError> {
        if context.available_tools.is_empty() {
            return Ok(vec![]);
        }

        let mut request =
            ToolCompletionRequest::new(context.messages.clone(), context.available_tools.clone())
                .with_max_tokens(1024)
                .with_tool_choice("auto");
        request.metadata = context.metadata.clone();

        let response = self.llm.complete_with_tools(request).await?;

        let reasoning = response.content.unwrap_or_default();

        let selections: Vec<ToolSelection> = response
            .tool_calls
            .into_iter()
            .map(|tool_call| ToolSelection {
                tool_name: tool_call.name,
                parameters: tool_call.arguments,
                reasoning: reasoning.clone(),
                alternatives: vec![],
                tool_call_id: tool_call.id,
            })
            .collect();

        Ok(selections)
    }

    /// Evaluate whether a task was completed successfully.
    pub async fn evaluate_success(
        &self,
        context: &ReasoningContext,
        result: &str,
    ) -> Result<SuccessEvaluation, LlmError> {
        let system_prompt = r#"You are an evaluation assistant. Your job is to determine if a task was completed successfully.

Analyse the task description and the result, then provide:
1. Whether the task was successful (true/false)
2. A confidence score (0-1)
3. Detailed reasoning
4. Any issues found
5. Suggestions for improvement

Respond in JSON format:
{
    "success": true/false,
    "confidence": 0.0-1.0,
    "reasoning": "...",
    "issues": ["..."],
    "suggestions": ["..."]
}"#;

        let mut messages = vec![ChatMessage::system(system_prompt)];

        if let Some(ref job) = context.job_description {
            messages.push(ChatMessage::user(format!(
                "Task description:\n{}\n\nResult:\n{}",
                job, result
            )));
        } else {
            messages.push(ChatMessage::user(format!(
                "Result to evaluate:\n{}",
                result
            )));
        }

        let request = CompletionRequest::new(messages)
            .with_max_tokens(1024)
            .with_temperature(0.1);

        let response = self.llm.complete(request).await?;

        // Clean reasoning model artefacts before parsing JSON.
        // Pre-truncate at tool tags to avoid strip_xml_tag discarding
        // content after unclosed tags (issue #789).
        let pre_truncated = truncate_at_tool_tags(&response.content);
        let cleaned = clean_response(&pre_truncated);
        self.parse_evaluation(&cleaned)
    }

    fn build_planning_prompt(&self, context: &ReasoningContext) -> String {
        let tools_desc = if context.available_tools.is_empty() {
            "No tools available.".to_string()
        } else {
            context
                .available_tools
                .iter()
                .map(|t| format!("- {}: {}", t.name, t.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            r#"You are a planning assistant for an autonomous agent. Your job is to create detailed, actionable plans.

Available tools:
{tools_desc}

When creating a plan:
1. Break down the goal into specific, achievable steps
2. Select the most appropriate tool for each step
3. Consider dependencies between steps
4. Estimate costs and time realistically
5. Identify potential failure points

Respond with a JSON plan in this format:
{{
    "goal": "Clear statement of the goal",
    "actions": [
        {{
            "tool_name": "tool_to_use",
            "parameters": {{}},
            "reasoning": "Why this action",
            "expected_outcome": "What should happen"
        }}
    ],
    "estimated_cost": 0.0,
    "estimated_time_secs": 0,
    "confidence": 0.0-1.0
}}"#
        )
    }

    fn parse_plan(&self, content: &str) -> Result<ActionPlan, LlmError> {
        // Try to extract JSON from the response
        let json_str = extract_json(content).unwrap_or(content);

        serde_json::from_str(json_str).map_err(|e| LlmError::InvalidResponse {
            provider: self.llm.model_name().to_string(),
            reason: format!("Failed to parse plan: {}", e),
        })
    }

    fn parse_evaluation(&self, content: &str) -> Result<SuccessEvaluation, LlmError> {
        let json_str = extract_json(content).unwrap_or(content);

        serde_json::from_str(json_str).map_err(|e| LlmError::InvalidResponse {
            provider: self.llm.model_name().to_string(),
            reason: format!("Failed to parse evaluation: {}", e),
        })
    }
}

/// Extract JSON from text that might contain other content.
pub(super) fn extract_json(text: &str) -> Option<&str> {
    // Find the first { and last } to extract JSON
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if start < end {
        Some(&text[start..=end])
    } else {
        None
    }
}
