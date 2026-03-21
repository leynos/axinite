//! Software builder for creating programs and tools using LLM-driven code generation.
//!
//! This module provides a general-purpose software building capability that:
//! - Uses an agent loop similar to Codex for iterative development
//! - Can build any software (binaries, libraries, scripts)
//! - Has special context injection when building WASM tools
//! - Integrates with existing tool loading infrastructure
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                          Software Build Loop                                 │
//! │                                                                              │
//! │  1. Analyze requirement ─▶ Determine project type, language, structure      │
//! │  2. Generate scaffold   ─▶ Create initial project files                     │
//! │  3. Implement code      ─▶ Write the actual implementation                  │
//! │  4. Build/compile       ─▶ Run build commands (cargo, npm, etc.)            │
//! │  5. Fix errors          ─▶ Parse errors, modify code, retry                 │
//! │  6. Test                ─▶ Run tests, fix failures                          │
//! │  7. Package             ─▶ Produce final artifact                           │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! For WASM tools specifically:
//! - Injects Tool trait interface documentation
//! - Injects WASM host function documentation
//! - Compiles to wasm32-wasip2 target
//! - Validates against tool interface
//! - Registers with ToolRegistry

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::JobContext;
use crate::error::ToolError as AgentToolError;
use crate::llm::{
    ChatMessage, LlmProvider, Reasoning, ReasoningContext, RespondResult, ToolDefinition,
};
use crate::tools::ToolRegistry;
use crate::tools::tool::{ApprovalRequirement, HostedToolEligibility, Tool, ToolError, ToolOutput};

mod build_loop;
mod builder_impl;
mod domain;
mod setup;
mod wrapper;

pub use domain::{
    BuildLog, BuildPhase, BuildRequirement, BuildResult, BuilderConfig, ExecutionCommand, Language,
    ProjectName, SoftwareBuilder, SoftwareType,
};
pub use setup::LlmSoftwareBuilder;
pub use wrapper::BuildSoftwareTool;

#[cfg(test)]
mod tests;
