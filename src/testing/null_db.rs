//! Null database helper for tests.
//!
//! Provides a [`NullDatabase`] struct that implements all `Native*Store` traits
//! with no-op methods returning default values. Useful as a baseline for
//! test doubles that need to override only specific methods.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::BrokenTool;
use crate::agent::routine::{Routine, RoutineRun};
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
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

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
    fn doc_not_found(doc_type: &str) -> WorkspaceError {
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

    async fn list_conversation_messages_paginated(
        &self,
        _conversation_id: Uuid,
        _before: Option<(DateTime<Utc>, Uuid)>,
        _limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        Ok((vec![], false))
    }

    async fn list_conversation_messages(
        &self,
        _conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        Ok(vec![])
    }

    async fn conversation_belongs_to_user(
        &self,
        _conversation_id: Uuid,
        _user_id: &str,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
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

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn update_routine(&self, _routine: &Routine) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn update_routine_runtime(
        &self,
        _params: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn delete_routine(&self, _id: Uuid) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn create_routine_run(&self, _run: &RoutineRun) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn complete_routine_run(
        &self,
        _params: crate::db::RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_routine_runs(
        &self,
        _routine_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
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
        _error_message: &str,
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

    async fn set_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
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
    ) -> Result<MemoryDocument, WorkspaceError> {
        Err(Self::doc_not_found("file"))
    }

    async fn get_document_by_id(&self, _id: Uuid) -> Result<MemoryDocument, WorkspaceError> {
        Err(Self::doc_not_found("id"))
    }

    async fn get_or_create_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
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
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
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
    ) -> Result<Vec<MemoryDocument>, WorkspaceError> {
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
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        Ok(vec![])
    }

    async fn hybrid_search(
        &self,
        _params: HybridSearchParams<'_>,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        Ok(vec![])
    }
}

// -----------------------------------------------------------------------------
// CapturingStore - A wrapper around NullDatabase that captures specific calls
// -----------------------------------------------------------------------------

use crate::context::JobState;

/// Captured status update call.
#[derive(Debug, Clone)]
pub struct StatusCall {
    /// The job status that was recorded.
    pub status: JobState,
    /// Optional failure reason associated with the status.
    pub reason: Option<String>,
}

/// Captured job event call.
#[derive(Debug, Clone)]
pub struct EventCall {
    /// The event type string (e.g., "result").
    pub event_type: String,
    /// The JSON data payload associated with the event.
    pub data: serde_json::Value,
}

/// Thread-safe storage for captured calls.
#[derive(Debug, Default)]
pub struct Calls {
    /// The last status update call captured, if any.
    pub last_status: Mutex<Option<StatusCall>>,
    /// The last event call captured, if any.
    pub last_event: Mutex<Option<EventCall>>,
}

impl Calls {
    /// Create a new empty Calls container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a status update call.
    pub async fn record_status(&self, _id: Uuid, status: JobState, reason: Option<&str>) {
        *self.last_status.lock().await = Some(StatusCall {
            status,
            reason: reason.map(ToOwned::to_owned),
        });
    }

    /// Record an event call.
    pub async fn record_event(
        &self,
        _job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) {
        *self.last_event.lock().await = Some(EventCall {
            event_type: event_type.as_str().to_string(),
            data: data.clone(),
        });
    }
}

/// A database wrapper that captures calls to specific methods for testing.
///
/// Delegates all other methods to the inner [`NullDatabase`].
#[derive(Debug)]
pub struct CapturingStore {
    inner: NullDatabase,
    calls: std::sync::Arc<Calls>,
}

impl CapturingStore {
    /// Create a new capturing store with an inner NullDatabase.
    pub fn new() -> Self {
        Self {
            inner: NullDatabase::new(),
            calls: std::sync::Arc::new(Calls::new()),
        }
    }

    /// Access the captured calls for assertions.
    pub fn calls(&self) -> &std::sync::Arc<Calls> {
        &self.calls
    }
}

impl Default for CapturingStore {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::db::NativeDatabase for CapturingStore {
    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        self.inner.run_migrations().await
    }
}

impl crate::db::NativeJobStore for CapturingStore {
    async fn save_job(&self, ctx: &JobContext) -> Result<(), DatabaseError> {
        self.inner.save_job(ctx).await
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        self.inner.get_job(id).await
    }

    async fn update_job_status(
        &self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        self.calls.record_status(id, status, failure_reason).await;
        Ok(())
    }

    async fn mark_job_stuck(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.inner.mark_job_stuck(id).await
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        self.inner.get_stuck_jobs().await
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        self.inner.list_agent_jobs().await
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        self.inner.agent_job_summary().await
    }

    async fn get_agent_job_failure_reason(
        &self,
        id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        self.inner.get_agent_job_failure_reason(id).await
    }

    async fn save_action(&self, job_id: Uuid, action: &ActionRecord) -> Result<(), DatabaseError> {
        self.inner.save_action(job_id, action).await
    }

    async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        self.inner.get_job_actions(job_id).await
    }

    async fn record_llm_call(&self, record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        self.inner.record_llm_call(record).await
    }

    async fn save_estimation_snapshot(
        &self,
        params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        self.inner.save_estimation_snapshot(params).await
    }

    async fn update_estimation_actuals(
        &self,
        params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        self.inner.update_estimation_actuals(params).await
    }
}

impl crate::db::NativeSandboxStore for CapturingStore {
    async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        self.inner.save_sandbox_job(job).await
    }

    async fn get_sandbox_job(&self, id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        self.inner.get_sandbox_job(id).await
    }

    async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        self.inner.list_sandbox_jobs().await
    }

    async fn update_sandbox_job_status(
        &self,
        params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        self.inner.update_sandbox_job_status(params).await
    }

    async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        self.inner.cleanup_stale_sandbox_jobs().await
    }

    async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        self.inner.sandbox_job_summary().await
    }

    async fn list_sandbox_jobs_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        self.inner.list_sandbox_jobs_for_user(user_id).await
    }

    async fn sandbox_job_summary_for_user(
        &self,
        user_id: UserId,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        self.inner.sandbox_job_summary_for_user(user_id).await
    }

    async fn sandbox_job_belongs_to_user(
        &self,
        job_id: Uuid,
        user_id: UserId,
    ) -> Result<bool, DatabaseError> {
        self.inner
            .sandbox_job_belongs_to_user(job_id, user_id)
            .await
    }

    async fn update_sandbox_job_mode(
        &self,
        id: Uuid,
        mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        self.inner.update_sandbox_job_mode(id, mode).await
    }

    async fn get_sandbox_job_mode(&self, id: Uuid) -> Result<Option<SandboxMode>, DatabaseError> {
        self.inner.get_sandbox_job_mode(id).await
    }

    async fn save_job_event(
        &self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.calls.record_event(job_id, event_type, data).await;
        Ok(())
    }

    async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        self.inner.list_job_events(job_id, before_id, limit).await
    }
}

// Delegate all other traits to inner NullDatabase
impl crate::db::NativeConversationStore for CapturingStore {
    async fn create_conversation(
        &self,
        channel: &str,
        user_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .create_conversation(channel, user_id, thread_id)
            .await
    }

    async fn touch_conversation(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.inner.touch_conversation(id).await
    }

    async fn add_conversation_message(
        &self,
        conversation_id: Uuid,
        role: &str,
        content: &str,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .add_conversation_message(conversation_id, role, content)
            .await
    }

    async fn ensure_conversation(
        &self,
        params: EnsureConversationParams<'_>,
    ) -> Result<(), DatabaseError> {
        self.inner.ensure_conversation(params).await
    }

    async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        self.inner
            .list_conversations_with_preview(user_id, channel, limit)
            .await
    }

    async fn list_conversations_all_channels(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        self.inner
            .list_conversations_all_channels(user_id, limit)
            .await
    }

    async fn get_or_create_routine_conversation(
        &self,
        routine_id: Uuid,
        routine_name: &str,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .get_or_create_routine_conversation(routine_id, routine_name, user_id)
            .await
    }

    async fn get_or_create_heartbeat_conversation(
        &self,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .get_or_create_heartbeat_conversation(user_id)
            .await
    }

    async fn get_or_create_assistant_conversation(
        &self,
        user_id: &str,
        channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .get_or_create_assistant_conversation(user_id, channel)
            .await
    }

    async fn create_conversation_with_metadata(
        &self,
        channel: &str,
        user_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        self.inner
            .create_conversation_with_metadata(channel, user_id, metadata)
            .await
    }

    async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        self.inner
            .list_conversation_messages_paginated(conversation_id, before, limit)
            .await
    }

    async fn list_conversation_messages(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        self.inner.list_conversation_messages(conversation_id).await
    }

    async fn conversation_belongs_to_user(
        &self,
        conversation_id: Uuid,
        user_id: &str,
    ) -> Result<bool, DatabaseError> {
        self.inner
            .conversation_belongs_to_user(conversation_id, user_id)
            .await
    }

    async fn update_conversation_metadata_field(
        &self,
        id: Uuid,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.inner
            .update_conversation_metadata_field(id, key, value)
            .await
    }

    async fn get_conversation_metadata(
        &self,
        id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        self.inner.get_conversation_metadata(id).await
    }
}

impl crate::db::NativeRoutineStore for CapturingStore {
    async fn create_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        self.inner.create_routine(routine).await
    }

    async fn get_routine(&self, id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        self.inner.get_routine(id).await
    }

    async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        self.inner.get_routine_by_name(user_id, name).await
    }

    async fn list_routines(&self, user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        self.inner.list_routines(user_id).await
    }

    async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.inner.list_all_routines().await
    }

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.inner.list_event_routines().await
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.inner.list_due_cron_routines().await
    }

    async fn update_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        self.inner.update_routine(routine).await
    }

    async fn update_routine_runtime(
        &self,
        params: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        self.inner.update_routine_runtime(params).await
    }

    async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError> {
        self.inner.delete_routine(id).await
    }

    async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        self.inner.create_routine_run(run).await
    }

    async fn complete_routine_run(
        &self,
        params: crate::db::RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        self.inner.complete_routine_run(params).await
    }

    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        self.inner.list_routine_runs(routine_id, limit).await
    }

    async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        self.inner.count_running_routine_runs(routine_id).await
    }

    async fn link_routine_run_to_job(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        self.inner.link_routine_run_to_job(run_id, job_id).await
    }
}

impl crate::db::NativeToolFailureStore for CapturingStore {
    async fn record_tool_failure(
        &self,
        tool_name: &str,
        error_message: &str,
    ) -> Result<(), DatabaseError> {
        self.inner
            .record_tool_failure(tool_name, error_message)
            .await
    }

    async fn get_broken_tools(&self, threshold: i32) -> Result<Vec<BrokenTool>, DatabaseError> {
        self.inner.get_broken_tools(threshold).await
    }

    async fn mark_tool_repaired(&self, tool_name: &str) -> Result<(), DatabaseError> {
        self.inner.mark_tool_repaired(tool_name).await
    }

    async fn increment_repair_attempts(&self, tool_name: &str) -> Result<(), DatabaseError> {
        self.inner.increment_repair_attempts(tool_name).await
    }
}

impl crate::db::NativeSettingsStore for CapturingStore {
    async fn get_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        self.inner.get_setting(user_id, key).await
    }

    async fn get_setting_full(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        self.inner.get_setting_full(user_id, key).await
    }

    async fn set_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.inner.set_setting(user_id, key, value).await
    }

    async fn delete_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        self.inner.delete_setting(user_id, key).await
    }

    async fn list_settings(&self, user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
        self.inner.list_settings(user_id).await
    }

    async fn get_all_settings(
        &self,
        user_id: UserId,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        self.inner.get_all_settings(user_id).await
    }

    async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        self.inner.set_all_settings(user_id, settings).await
    }

    async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        self.inner.has_settings(user_id).await
    }
}

impl crate::db::NativeWorkspaceStore for CapturingStore {
    async fn get_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        self.inner
            .get_document_by_path(user_id, agent_id, path)
            .await
    }

    async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError> {
        self.inner.get_document_by_id(id).await
    }

    async fn get_or_create_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        self.inner
            .get_or_create_document_by_path(user_id, agent_id, path)
            .await
    }

    async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError> {
        self.inner.update_document(id, content).await
    }

    async fn delete_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        self.inner
            .delete_document_by_path(user_id, agent_id, path)
            .await
    }

    async fn list_directory(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        directory: &str,
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        self.inner
            .list_directory(user_id, agent_id, directory)
            .await
    }

    async fn list_all_paths(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        self.inner.list_all_paths(user_id, agent_id).await
    }

    async fn list_documents(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<MemoryDocument>, WorkspaceError> {
        self.inner.list_documents(user_id, agent_id).await
    }

    async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        self.inner.delete_chunks(document_id).await
    }

    async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        self.inner.insert_chunk(params).await
    }

    async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        self.inner.update_chunk_embedding(chunk_id, embedding).await
    }

    async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        self.inner
            .get_chunks_without_embeddings(user_id, agent_id, limit)
            .await
    }

    async fn hybrid_search(
        &self,
        params: HybridSearchParams<'_>,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        self.inner.hybrid_search(params).await
    }
}
