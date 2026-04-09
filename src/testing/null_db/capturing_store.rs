//! Capturing database wrapper for tests.
//!
//! Provides a [`CapturingStore`] that wraps [`NullDatabase`] and captures
//! specific method calls for test assertions.

use std::sync::Arc;

use delegate::delegate;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::{Routine, routine::RoutineRun};
use crate::context::JobState;
use crate::db::{
    EnsureConversationParams, EstimationActualsParams, EstimationSnapshotParams,
    HybridSearchParams, InsertChunkParams, SandboxEventType, SandboxJobStatusUpdate, SandboxMode,
    SettingKey, UserId,
};
use crate::error::{DatabaseError, WorkspaceError};
use crate::history::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary, SettingRow,
};
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

use super::NullDatabase;

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
    calls: Arc<Calls>,
}

impl CapturingStore {
    /// Create a new capturing store with an inner NullDatabase.
    pub fn new() -> Self {
        Self {
            inner: NullDatabase::new(),
            calls: Arc::new(Calls::new()),
        }
    }

    /// Access the captured calls for assertions.
    pub fn calls(&self) -> &Arc<Calls> {
        &self.calls
    }
}

impl Default for CapturingStore {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::db::NativeDatabase for CapturingStore {
    delegate! {
        to self.inner {
            async fn run_migrations(&self) -> Result<(), DatabaseError>;
        }
    }
}

impl crate::db::NativeJobStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn save_job(&self, ctx: &crate::context::JobContext) -> Result<(), DatabaseError>;
            async fn get_job(
                &self,
                id: Uuid
            ) -> Result<Option<crate::context::JobContext>, DatabaseError>;
            async fn mark_job_stuck(&self, id: Uuid) -> Result<(), DatabaseError>;
            async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError>;
            async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError>;
            async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError>;
            async fn get_agent_job_failure_reason(
                &self,
                id: Uuid
            ) -> Result<Option<String>, DatabaseError>;
            async fn save_action(
                &self,
                job_id: Uuid,
                action: &crate::context::ActionRecord
            ) -> Result<(), DatabaseError>;
            async fn get_job_actions(
                &self,
                job_id: Uuid
            ) -> Result<Vec<crate::context::ActionRecord>, DatabaseError>;
            async fn record_llm_call(
                &self,
                record: &LlmCallRecord<'_>
            ) -> Result<Uuid, DatabaseError>;
            async fn save_estimation_snapshot(
                &self,
                params: EstimationSnapshotParams<'_>
            ) -> Result<Uuid, DatabaseError>;
            async fn update_estimation_actuals(
                &self,
                params: EstimationActualsParams
            ) -> Result<(), DatabaseError>;
        }
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
}

impl crate::db::NativeSandboxStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError>;
            async fn get_sandbox_job(
                &self,
                id: Uuid
            ) -> Result<Option<SandboxJobRecord>, DatabaseError>;
            async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
            async fn update_sandbox_job_status(
                &self,
                params: SandboxJobStatusUpdate<'_>
            ) -> Result<(), DatabaseError>;
            async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError>;
            async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError>;
            async fn list_sandbox_jobs_for_user(
                &self,
                user_id: UserId
            ) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
            async fn sandbox_job_summary_for_user(
                &self,
                user_id: UserId
            ) -> Result<SandboxJobSummary, DatabaseError>;
            async fn sandbox_job_belongs_to_user(
                &self,
                job_id: Uuid,
                user_id: UserId
            ) -> Result<bool, DatabaseError>;
            async fn update_sandbox_job_mode(
                &self,
                id: Uuid,
                mode: SandboxMode
            ) -> Result<(), DatabaseError>;
            async fn get_sandbox_job_mode(
                &self,
                id: Uuid
            ) -> Result<Option<SandboxMode>, DatabaseError>;
            async fn list_job_events(
                &self,
                job_id: Uuid,
                before_id: Option<i64>,
                limit: Option<i64>
            ) -> Result<Vec<JobEventRecord>, DatabaseError>;
        }
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
}

// Delegate all other traits to inner NullDatabase
impl crate::db::NativeConversationStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn create_conversation(
                &self,
                channel: &str,
                user_id: &str,
                thread_id: Option<&str>
            ) -> Result<Uuid, DatabaseError>;
            async fn touch_conversation(&self, id: Uuid) -> Result<(), DatabaseError>;
            async fn add_conversation_message(
                &self,
                conversation_id: Uuid,
                role: &str,
                content: &str
            ) -> Result<Uuid, DatabaseError>;
            async fn ensure_conversation(
                &self,
                params: EnsureConversationParams<'_>
            ) -> Result<(), DatabaseError>;
            async fn list_conversations_with_preview(
                &self,
                user_id: &str,
                channel: &str,
                limit: usize
            ) -> Result<Vec<ConversationSummary>, DatabaseError>;
            async fn list_conversations_all_channels(
                &self,
                user_id: &str,
                limit: usize
            ) -> Result<Vec<ConversationSummary>, DatabaseError>;
            async fn get_or_create_routine_conversation(
                &self,
                routine_id: Uuid,
                routine_name: &str,
                user_id: &str
            ) -> Result<Uuid, DatabaseError>;
            async fn get_or_create_heartbeat_conversation(
                &self,
                user_id: &str
            ) -> Result<Uuid, DatabaseError>;
            async fn get_or_create_assistant_conversation(
                &self,
                user_id: &str,
                channel: &str
            ) -> Result<Uuid, DatabaseError>;
            async fn create_conversation_with_metadata(
                &self,
                channel: &str,
                user_id: &str,
                metadata: &serde_json::Value
            ) -> Result<Uuid, DatabaseError>;
            async fn update_conversation_metadata_field(
                &self,
                id: Uuid,
                key: &str,
                value: &serde_json::Value
            ) -> Result<(), DatabaseError>;
            async fn get_conversation_metadata(
                &self,
                id: Uuid
            ) -> Result<Option<serde_json::Value>, DatabaseError>;
            async fn list_conversation_messages(
                &self,
                conversation_id: Uuid
            ) -> Result<Vec<ConversationMessage>, DatabaseError>;
            async fn list_conversation_messages_paginated(
                &self,
                conversation_id: Uuid,
                before: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
                limit: usize
            ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError>;
            async fn conversation_belongs_to_user(
                &self,
                conversation_id: Uuid,
                user_id: &str
            ) -> Result<bool, DatabaseError>;
        }
    }
}

impl crate::db::NativeRoutineStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn create_routine(&self, routine: &Routine) -> Result<(), DatabaseError>;
            async fn get_routine(&self, id: Uuid) -> Result<Option<Routine>, DatabaseError>;
            async fn get_routine_by_name(
                &self,
                user_id: &str,
                name: &str
            ) -> Result<Option<Routine>, DatabaseError>;
            async fn list_routines(&self, user_id: &str) -> Result<Vec<Routine>, DatabaseError>;
            async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError>;
            async fn update_routine(&self, routine: &Routine) -> Result<(), DatabaseError>;
            async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError>;
            async fn update_routine_runtime(
                &self,
                update: crate::db::RoutineRuntimeUpdate<'_>
            ) -> Result<(), DatabaseError>;
            async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError>;
            async fn list_routine_runs(
                &self,
                routine_id: Uuid,
                limit: i64
            ) -> Result<Vec<RoutineRun>, DatabaseError>;
            async fn complete_routine_run(
                &self,
                completion: crate::db::RoutineRunCompletion<'_>
            ) -> Result<(), DatabaseError>;
            async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError>;
            async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError>;
            async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError>;
            async fn link_routine_run_to_job(
                &self,
                run_id: Uuid,
                job_id: Uuid
            ) -> Result<(), DatabaseError>;
        }
    }
}

impl crate::db::NativeToolFailureStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn record_tool_failure(
                &self,
                tool_name: &str,
                error: &str
            ) -> Result<(), DatabaseError>;
            async fn get_broken_tools(
                &self,
                threshold: i32
            ) -> Result<Vec<crate::agent::BrokenTool>, DatabaseError>;
            async fn mark_tool_repaired(&self, tool_name: &str) -> Result<(), DatabaseError>;
            async fn increment_repair_attempts(&self, tool_name: &str) -> Result<(), DatabaseError>;
        }
    }
}

impl crate::db::NativeSettingsStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn get_setting(
                &self,
                user_id: UserId,
                key: SettingKey
            ) -> Result<Option<serde_json::Value>, DatabaseError>;
            async fn get_setting_full(
                &self,
                user_id: UserId,
                key: SettingKey
            ) -> Result<Option<SettingRow>, DatabaseError>;
            async fn delete_setting(
                &self,
                user_id: UserId,
                key: SettingKey
            ) -> Result<bool, DatabaseError>;
            async fn list_settings(
                &self,
                user_id: UserId
            ) -> Result<Vec<SettingRow>, DatabaseError>;
            async fn set_setting(
                &self,
                user_id: UserId,
                key: SettingKey,
                value: &serde_json::Value
            ) -> Result<(), DatabaseError>;
            async fn get_all_settings(
                &self,
                user_id: UserId
            ) -> Result<std::collections::HashMap<String, serde_json::Value>, DatabaseError>;
            async fn set_all_settings(
                &self,
                user_id: UserId,
                settings: &std::collections::HashMap<String, serde_json::Value>
            ) -> Result<(), DatabaseError>;
            async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError>;
        }
    }
}

impl crate::db::NativeWorkspaceStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn get_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<MemoryDocument, WorkspaceError>;
            async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError>;
            async fn get_or_create_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<MemoryDocument, WorkspaceError>;
            async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError>;
            async fn delete_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<(), WorkspaceError>;
            async fn list_directory(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                directory: &str
            ) -> Result<Vec<WorkspaceEntry>, WorkspaceError>;
            async fn list_all_paths(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>
            ) -> Result<Vec<String>, WorkspaceError>;
            async fn list_documents(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>
            ) -> Result<Vec<MemoryDocument>, WorkspaceError>;
            async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError>;
            async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError>;
            async fn update_chunk_embedding(
                &self,
                chunk_id: Uuid,
                embedding: &[f32]
            ) -> Result<(), WorkspaceError>;
            async fn get_chunks_without_embeddings(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                limit: usize
            ) -> Result<Vec<MemoryChunk>, WorkspaceError>;
            async fn hybrid_search(
                &self,
                params: HybridSearchParams<'_>
            ) -> Result<Vec<SearchResult>, WorkspaceError>;
        }
    }
}
