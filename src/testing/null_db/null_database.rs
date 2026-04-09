//! Null database implementation for tests.
//!
//! All methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.).
//! Use this as a baseline for test doubles that need to override only
//! specific methods while delegating the rest to null behavior.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::agent::BrokenTool;
use crate::agent::{Routine, routine::RoutineRun};
use crate::context::{ActionRecord, JobContext};
use crate::db::{
    EnsureConversationParams, EstimationActualsParams, EstimationSnapshotParams,
    HybridSearchParams, InsertChunkParams, RoutineRuntimeUpdate, SandboxEventType,
    SandboxJobStatusUpdate, SandboxMode, SettingKey, UserId,
};
use crate::error::{DatabaseError, WorkspaceError};
use crate::history::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary, SettingRow,
};
use crate::workspace::{
    MemoryChunk as WorkspaceMemoryChunk, MemoryDocument as WorkspaceMemoryDocument,
    SearchResult as WorkspaceSearchResult, WorkspaceEntry as WorkspaceWorkspaceEntry,
};

/// A no-op database implementation for testing.
///
/// All methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.).
/// Use this as a baseline for test doubles that need to override only
/// specific methods while delegating the rest to null behavior.
#[derive(Debug, Default)]
pub struct NullDatabase;

impl NullDatabase {
    /// Create a new null database instance.
    pub fn new() -> Self {
        Self
    }

    /// Helper for document-not-found errors in workspace operations.
    pub(super) fn doc_not_found(doc_type: &str) -> WorkspaceError {
        WorkspaceError::DocumentNotFound {
            doc_type: doc_type.to_string(),
            user_id: "test".to_string(),
        }
    }
}

// -----------------------------------------------------------------------------
// NativeDatabase
// -----------------------------------------------------------------------------

impl crate::db::NativeDatabase for NullDatabase {
    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// NativeJobStore
// -----------------------------------------------------------------------------

impl crate::db::NativeJobStore for NullDatabase {
    async fn save_job(&self, _ctx: &JobContext) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job(&self, _id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        Ok(None)
    }

    async fn update_job_status(
        &self,
        _id: Uuid,
        _status: crate::context::JobState,
        _failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn mark_job_stuck(&self, _id: Uuid) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        Ok(AgentJobSummary::default())
    }

    async fn get_agent_job_failure_reason(
        &self,
        _id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }

    async fn save_action(
        &self,
        _job_id: Uuid,
        _action: &ActionRecord,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job_actions(&self, _job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn record_llm_call(&self, _record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn save_estimation_snapshot(
        &self,
        _params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn update_estimation_actuals(
        &self,
        _params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// NativeSandboxStore
// -----------------------------------------------------------------------------

impl crate::db::NativeSandboxStore for NullDatabase {
    async fn save_sandbox_job(&self, _job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_sandbox_job(&self, _id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        Ok(None)
    }

    async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn update_sandbox_job_status(
        &self,
        _params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        Ok(0)
    }

    async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        Ok(SandboxJobSummary::default())
    }

    async fn list_sandbox_jobs_for_user(
        &self,
        _user_id: UserId,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn sandbox_job_summary_for_user(
        &self,
        _user_id: UserId,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        Ok(SandboxJobSummary::default())
    }

    async fn sandbox_job_belongs_to_user(
        &self,
        _job_id: Uuid,
        _user_id: UserId,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn update_sandbox_job_mode(
        &self,
        _id: Uuid,
        _mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_sandbox_job_mode(&self, _id: Uuid) -> Result<Option<SandboxMode>, DatabaseError> {
        Ok(None)
    }

    async fn save_job_event(
        &self,
        _job_id: Uuid,
        _event_type: SandboxEventType,
        _data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_job_events(
        &self,
        _job_id: Uuid,
        _before_id: Option<i64>,
        _limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        Ok(vec![])
    }
}

// -----------------------------------------------------------------------------
// NativeConversationStore
// -----------------------------------------------------------------------------

impl crate::db::NativeConversationStore for NullDatabase {
    async fn create_conversation(
        &self,
        _channel: &str,
        _user_id: &str,
        _thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn touch_conversation(&self, _id: Uuid) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn add_conversation_message(
        &self,
        _conversation_id: Uuid,
        _role: &str,
        _content: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn ensure_conversation(
        &self,
        _params: EnsureConversationParams<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_conversations_with_preview(
        &self,
        _user_id: &str,
        _channel: &str,
        _limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_conversations_all_channels(
        &self,
        _user_id: &str,
        _limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        Ok(vec![])
    }

    async fn get_or_create_routine_conversation(
        &self,
        _routine_id: Uuid,
        _routine_name: &str,
        _user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn get_or_create_heartbeat_conversation(
        &self,
        _user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn get_or_create_assistant_conversation(
        &self,
        _user_id: &str,
        _channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn create_conversation_with_metadata(
        &self,
        _channel: &str,
        _user_id: &str,
        _metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn update_conversation_metadata_field(
        &self,
        _id: Uuid,
        _key: &str,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_conversation_metadata(
        &self,
        _id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        Ok(None)
    }

    async fn list_conversation_messages(
        &self,
        _conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_conversation_messages_paginated(
        &self,
        _conversation_id: Uuid,
        _before: Option<(DateTime<Utc>, Uuid)>,
        _limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        Ok((vec![], false))
    }

    async fn conversation_belongs_to_user(
        &self,
        _conversation_id: Uuid,
        _user_id: &str,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }
}

// -----------------------------------------------------------------------------
// NativeRoutineStore
// -----------------------------------------------------------------------------

impl crate::db::NativeRoutineStore for NullDatabase {
    async fn create_routine(&self, _routine: &Routine) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_routine(&self, _id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        Ok(None)
    }

    async fn get_routine_by_name(
        &self,
        _user_id: &str,
        _name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        Ok(None)
    }

    async fn list_routines(&self, _user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn update_routine(&self, _routine: &Routine) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn delete_routine(&self, _id: Uuid) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn update_routine_runtime(
        &self,
        _update: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn create_routine_run(&self, _run: &RoutineRun) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_routine_runs(
        &self,
        _routine_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        Ok(vec![])
    }

    async fn complete_routine_run(
        &self,
        _completion: crate::db::RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn count_running_routine_runs(&self, _routine_id: Uuid) -> Result<i64, DatabaseError> {
        Ok(0)
    }

    async fn link_routine_run_to_job(
        &self,
        _run_id: Uuid,
        _job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// NativeToolFailureStore
// -----------------------------------------------------------------------------

impl crate::db::NativeToolFailureStore for NullDatabase {
    async fn record_tool_failure(
        &self,
        _tool_name: &str,
        _error: &str,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_broken_tools(&self, _threshold: i32) -> Result<Vec<BrokenTool>, DatabaseError> {
        Ok(vec![])
    }

    async fn mark_tool_repaired(&self, _tool_name: &str) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn increment_repair_attempts(&self, _tool_name: &str) -> Result<(), DatabaseError> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// NativeSettingsStore
// -----------------------------------------------------------------------------

impl crate::db::NativeSettingsStore for NullDatabase {
    async fn get_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        Ok(None)
    }

    async fn get_setting_full(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        Ok(None)
    }

    async fn delete_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn list_settings(&self, _user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
        Ok(vec![])
    }

    async fn set_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_all_settings(
        &self,
        _user_id: UserId,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        Ok(HashMap::new())
    }

    async fn set_all_settings(
        &self,
        _user_id: UserId,
        _settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn has_settings(&self, _user_id: UserId) -> Result<bool, DatabaseError> {
        Ok(false)
    }
}

// -----------------------------------------------------------------------------
// NativeWorkspaceStore
// -----------------------------------------------------------------------------

impl crate::db::NativeWorkspaceStore for NullDatabase {
    async fn get_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(Self::doc_not_found("file"))
    }

    async fn get_document_by_id(
        &self,
        _id: Uuid,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(Self::doc_not_found("id"))
    }

    async fn get_or_create_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(Self::doc_not_found("file"))
    }

    async fn update_document(&self, _id: Uuid, _content: &str) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn delete_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn list_directory(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _directory: &str,
    ) -> Result<Vec<WorkspaceWorkspaceEntry>, WorkspaceError> {
        Ok(vec![])
    }

    async fn list_all_paths(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        Ok(vec![])
    }

    async fn list_documents(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
    ) -> Result<Vec<WorkspaceMemoryDocument>, WorkspaceError> {
        Ok(vec![])
    }

    async fn delete_chunks(&self, _document_id: Uuid) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn insert_chunk(&self, _params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        Ok(Uuid::new_v4())
    }

    async fn update_chunk_embedding(
        &self,
        _chunk_id: Uuid,
        _embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn get_chunks_without_embeddings(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _limit: usize,
    ) -> Result<Vec<WorkspaceMemoryChunk>, WorkspaceError> {
        Ok(vec![])
    }

    async fn hybrid_search(
        &self,
        _params: HybridSearchParams<'_>,
    ) -> Result<Vec<WorkspaceSearchResult>, WorkspaceError> {
        Ok(vec![])
    }
}
