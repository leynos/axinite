//! Helper methods for the default self-repair implementation.

use uuid::Uuid;

use crate::agent::self_repair::default::{BuilderAndDb, DefaultSelfRepair};
use crate::agent::self_repair::{BrokenTool, RepairResult};
use crate::db::Database;
use crate::error::RepairError;
use crate::tools::builder::{BuildResult, ProjectName};
use crate::tools::{BuildRequirement, Language, SoftwareBuilder, SoftwareType};

impl DefaultSelfRepair {
    /// Validates preconditions for tool repair: builder/store availability.
    /// Returns the builder and store references on success, or a terminal RepairResult on failure.
    pub(super) fn validate_repair_preconditions(
        &self,
        tool: &BrokenTool,
    ) -> Result<BuilderAndDb<'_>, RepairResult> {
        let Some(ref builder) = self.builder else {
            tracing::warn!(
                tool_name = %tool.name,
                "repair precondition failed: builder not available"
            );
            return Err(RepairResult::ManualRequired {
                message: format!("Builder not available for repairing tool '{}'", tool.name),
            });
        };

        let Some(ref store) = self.store else {
            tracing::warn!(
                tool_name = %tool.name,
                "repair precondition failed: store not available"
            );
            return Err(RepairResult::ManualRequired {
                message: "Store not available for tracking repair".to_string(),
            });
        };

        Ok((builder, store))
    }

    /// Creates a BuildRequirement from a BrokenTool, validating the tool name.
    pub(super) fn build_repair_requirement(
        tool: &BrokenTool,
    ) -> Result<BuildRequirement, RepairError> {
        let project_name = ProjectName::new(&tool.name).map_err(|error| RepairError::Failed {
            target_type: "tool".to_string(),
            target_id: Uuid::nil(),
            reason: format!(
                "invalid tool name '{}' for repair build: {error}",
                tool.name
            ),
        })?;

        Ok(BuildRequirement {
            name: project_name,
            description: format!(
                concat!(
                    "Repair broken WASM tool.\n\n",
                    "Tool name: {}\n",
                    "Previous error: {}\n",
                    "Failure count: {}\n\n",
                    "Analyze the error, fix the implementation, and rebuild."
                ),
                tool.name,
                tool.last_error.as_deref().unwrap_or("Unknown error"),
                tool.failure_count
            ),
            software_type: SoftwareType::WasmTool,
            language: Language::Rust,
            input_spec: None,
            output_spec: None,
            dependencies: vec![],
            capabilities: vec!["http".to_string(), "workspace".to_string()],
        })
    }

    /// Handles the build result, marking the tool as repaired if successful.
    pub(super) async fn handle_build_result(
        result: BuildResult,
        tool: &BrokenTool,
        store: &dyn Database,
    ) -> Result<RepairResult, RepairError> {
        if result.success {
            tracing::info!(
                "Successfully rebuilt tool '{}' after {} iterations",
                tool.name,
                result.iterations
            );

            // Mark as repaired in database
            match store.mark_tool_repaired(&tool.name).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!(
                        tool_name = %tool.name,
                        error = %e,
                        "failed to mark tool as repaired in database after successful build"
                    );
                    return Err(RepairError::Failed {
                        target_type: "tool".to_string(),
                        target_id: Uuid::nil(),
                        reason: format!("failed to mark {} as repaired: {}", tool.name, e),
                    });
                }
            }

            if result.registered {
                tracing::info!("Repaired tool '{}' auto-registered", tool.name);
            }

            Ok(RepairResult::Success {
                message: format!(
                    "Tool '{}' repaired successfully after {} {}",
                    tool.name,
                    result.iterations,
                    Self::iteration_word(result.iterations)
                ),
            })
        } else {
            // Build completed but failed
            tracing::warn!(
                "Repair build for '{}' completed but failed: {:?}",
                tool.name,
                result.error
            );
            Ok(RepairResult::Retry {
                message: format!(
                    "Repair attempt {} for '{}' failed: {}",
                    tool.repair_attempts + 1,
                    tool.name,
                    result.error.unwrap_or_else(|| "Unknown error".to_string())
                ),
            })
        }
    }

    pub(super) async fn attempt_repair_build(
        tool: &BrokenTool,
        store: &dyn Database,
        builder: &dyn SoftwareBuilder,
        requirement: &BuildRequirement,
    ) -> Result<RepairResult, RepairError> {
        match builder.build(requirement).await {
            Ok(result) => Self::handle_build_result(result, tool, store).await,
            Err(e) => {
                tracing::error!("Repair build for '{}' errored: {}", tool.name, e);
                Ok(RepairResult::Retry {
                    message: format!("Repair build error: {}", e),
                })
            }
        }
    }

    fn iteration_word(iterations: u32) -> &'static str {
        if iterations == 1 {
            "iteration"
        } else {
            "iterations"
        }
    }

    async fn load_persisted_broken_tool(
        store: &dyn Database,
        tool: &BrokenTool,
    ) -> Result<Option<BrokenTool>, RepairError> {
        store
            .get_broken_tool_by_name(&tool.name)
            .await
            .map_err(|e| RepairError::Failed {
                target_type: "tool".to_string(),
                target_id: Uuid::nil(),
                reason: format!("failed to reload repair attempts for {}: {}", tool.name, e),
            })
    }

    /// Loads the persisted tool state, enforces the attempt limit, then executes
    /// the repair build.
    ///
    /// Called from `repair_broken_tool` after preconditions are validated and the
    /// repair claim is acquired.
    pub(super) async fn execute_repair(
        &self,
        tool: &BrokenTool,
        builder: &dyn SoftwareBuilder,
        store: &dyn Database,
    ) -> Result<RepairResult, RepairError> {
        let persisted_tool = Self::load_persisted_broken_tool(store, tool).await?;
        let tool_for_repair = persisted_tool.as_ref().unwrap_or(tool);

        if let Some(p) = persisted_tool.as_ref() {
            tracing::debug!(
                tool_name = %p.name,
                source = "persisted",
                "using persisted tool state for repair"
            );
        } else {
            tracing::debug!(
                tool_name = %tool.name,
                source = "input",
                "using input tool state for repair"
            );
        }

        if tool_for_repair.repair_attempts >= self.max_repair_attempts {
            tracing::warn!(
                tool_name = %tool_for_repair.name,
                repair_attempts = tool_for_repair.repair_attempts,
                max_repair_attempts = self.max_repair_attempts,
                "repair precondition failed: max repair attempts exceeded"
            );
            return Ok(RepairResult::ManualRequired {
                message: format!(
                    "Tool '{}' exceeded max repair attempts ({})",
                    tool_for_repair.name, self.max_repair_attempts
                ),
            });
        }

        let requirement = Self::build_repair_requirement(tool_for_repair)?;
        tracing::info!(
            "Attempting to repair tool '{}' (attempt {})",
            tool_for_repair.name,
            tool_for_repair.repair_attempts + 1
        );

        match store.increment_repair_attempts(&tool_for_repair.name).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    tool_name = %tool_for_repair.name,
                    error = %e,
                    "failed to increment repair attempts in database"
                );
                return Err(RepairError::Failed {
                    target_type: "tool".to_string(),
                    target_id: Uuid::nil(),
                    reason: format!(
                        "failed to increment repair attempts for {}: {}",
                        tool_for_repair.name, e
                    ),
                });
            }
        }

        Self::attempt_repair_build(tool_for_repair, store, builder, &requirement).await
    }
}
