//! Blanket adapters that bridge `Native*Store` implementations to their
//! dyn-safe counterparts via `Box::pin`.
//!
//! The private [`impl_db_forwarders!`] macro eliminates boilerplate for the
//! ADR-006 dual-trait boundary pattern.  Each invocation generates a blanket
//! `impl DynTrait for T where T: NativeTrait` that wraps every method's
//! return value in a boxed future.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::params::*;
use crate::db::traits::conversation::*;
use crate::db::traits::database::*;
use crate::db::traits::job::*;
use crate::db::traits::routine::*;
use crate::db::traits::sandbox::*;
use crate::db::traits::settings::*;
use crate::db::traits::tool_failure::*;
use crate::db::traits::workspace::*;

use crate::agent::BrokenTool;
use crate::agent::routine::{Routine, RoutineRun};
use crate::context::{ActionRecord, JobContext, JobState};
use crate::error::{DatabaseError, WorkspaceError};
use crate::history::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary, SettingRow,
};
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

/// Generate blanket adapter implementations that forward dyn-safe trait methods
/// to native async trait methods via `Box::pin`.
///
/// This macro eliminates boilerplate for the ADR-006 dyn/native boundary
/// pattern, where each object-safe `*Store` trait has a companion
/// `Native*Store` trait with native async fn methods (RPITIT), and a blanket
/// impl bridges the two.
macro_rules! impl_db_forwarders {
    (
        dyn = $dyn_trait:path,
        native = $native_trait:path,
        methods = {
            $(
                fn $name:ident ( $($arg:ident : $argty:ty),* $(,)? ) -> $ret:ty ;
            )*
        }
    ) => {
        impl<T> $dyn_trait for T
        where
            T: $native_trait + Send + Sync,
        {
            $(
                fn $name<'a>(&'a self, $($arg: $argty),*) -> DbFuture<'a, $ret> {
                    Box::pin(<T as $native_trait>::$name(self, $($arg),*))
                }
            )*
        }
    };
}

// ---- ConversationStore ----

impl_db_forwarders! {
    dyn = ConversationStore,
    native = NativeConversationStore,
    methods = {
        fn create_conversation(channel: &'a str, user_id: &'a str, thread_id: Option<&'a str>) -> Result<Uuid, DatabaseError>;
        fn touch_conversation(id: Uuid) -> Result<(), DatabaseError>;
        fn add_conversation_message(conversation_id: Uuid, role: &'a str, content: &'a str) -> Result<Uuid, DatabaseError>;
        fn ensure_conversation(params: EnsureConversationParams<'a>) -> Result<(), DatabaseError>;
        fn list_conversations_with_preview(user_id: &'a str, channel: &'a str, limit: i64) -> Result<Vec<ConversationSummary>, DatabaseError>;
        fn list_conversations_all_channels(user_id: &'a str, limit: i64) -> Result<Vec<ConversationSummary>, DatabaseError>;
        fn get_or_create_routine_conversation(routine_id: Uuid, routine_name: &'a str, user_id: &'a str) -> Result<Uuid, DatabaseError>;
        fn get_or_create_heartbeat_conversation(user_id: &'a str) -> Result<Uuid, DatabaseError>;
        fn get_or_create_assistant_conversation(user_id: &'a str, channel: &'a str) -> Result<Uuid, DatabaseError>;
        fn create_conversation_with_metadata(channel: &'a str, user_id: &'a str, metadata: &'a serde_json::Value) -> Result<Uuid, DatabaseError>;
        fn list_conversation_messages_paginated(conversation_id: Uuid, before: Option<(DateTime<Utc>, Uuid)>, limit: i64) -> Result<(Vec<ConversationMessage>, bool), DatabaseError>;
        fn update_conversation_metadata_field(id: Uuid, key: &'a str, value: &'a serde_json::Value) -> Result<(), DatabaseError>;
        fn get_conversation_metadata(id: Uuid) -> Result<Option<serde_json::Value>, DatabaseError>;
        fn list_conversation_messages(conversation_id: Uuid) -> Result<Vec<ConversationMessage>, DatabaseError>;
        fn conversation_belongs_to_user(conversation_id: Uuid, user_id: &'a str) -> Result<bool, DatabaseError>;
    }
}

// ---- JobStore ----

impl_db_forwarders! {
    dyn = JobStore,
    native = NativeJobStore,
    methods = {
        fn save_job(ctx: &'a JobContext) -> Result<(), DatabaseError>;
        fn get_job(id: Uuid) -> Result<Option<JobContext>, DatabaseError>;
        fn update_job_status(id: Uuid, status: JobState, failure_reason: Option<&'a str>) -> Result<(), DatabaseError>;
        fn mark_job_stuck(id: Uuid) -> Result<(), DatabaseError>;
        fn get_stuck_jobs() -> Result<Vec<Uuid>, DatabaseError>;
        fn list_agent_jobs() -> Result<Vec<AgentJobRecord>, DatabaseError>;
        fn agent_job_summary() -> Result<AgentJobSummary, DatabaseError>;
        fn get_agent_job_failure_reason(id: Uuid) -> Result<Option<String>, DatabaseError>;
        fn save_action(job_id: Uuid, action: &'a ActionRecord) -> Result<(), DatabaseError>;
        fn get_job_actions(job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError>;
        fn record_llm_call(record: &'a LlmCallRecord<'a>) -> Result<Uuid, DatabaseError>;
        fn save_estimation_snapshot(params: EstimationSnapshotParams<'a>) -> Result<Uuid, DatabaseError>;
        fn update_estimation_actuals(params: EstimationActualsParams) -> Result<(), DatabaseError>;
    }
}

// ---- SandboxStore ----

impl_db_forwarders! {
    dyn = SandboxStore,
    native = NativeSandboxStore,
    methods = {
        fn save_sandbox_job(job: &'a SandboxJobRecord) -> Result<(), DatabaseError>;
        fn get_sandbox_job(id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError>;
        fn list_sandbox_jobs() -> Result<Vec<SandboxJobRecord>, DatabaseError>;
        fn update_sandbox_job_status(params: SandboxJobStatusUpdate<'a>) -> Result<(), DatabaseError>;
        fn cleanup_stale_sandbox_jobs() -> Result<u64, DatabaseError>;
        fn sandbox_job_summary() -> Result<SandboxJobSummary, DatabaseError>;
        fn list_sandbox_jobs_for_user(user_id: UserId) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
        fn sandbox_job_summary_for_user(user_id: UserId) -> Result<SandboxJobSummary, DatabaseError>;
        fn sandbox_job_belongs_to_user(job_id: Uuid, user_id: UserId) -> Result<bool, DatabaseError>;
        fn update_sandbox_job_mode(id: Uuid, mode: SandboxMode) -> Result<(), DatabaseError>;
        fn get_sandbox_job_mode(id: Uuid) -> Result<Option<SandboxMode>, DatabaseError>;
        fn save_job_event(job_id: Uuid, event_type: SandboxEventType, data: &'a serde_json::Value) -> Result<(), DatabaseError>;
        fn list_job_events(job_id: Uuid, before_id: Option<i64>, limit: Option<i64>) -> Result<Vec<JobEventRecord>, DatabaseError>;
    }
}

// ---- RoutineStore ----

impl_db_forwarders! {
    dyn = RoutineStore,
    native = NativeRoutineStore,
    methods = {
        fn create_routine(routine: &'a Routine) -> Result<(), DatabaseError>;
        fn get_routine(id: Uuid) -> Result<Option<Routine>, DatabaseError>;
        fn get_routine_by_name(user_id: &'a str, name: &'a str) -> Result<Option<Routine>, DatabaseError>;
        fn list_routines(user_id: &'a str) -> Result<Vec<Routine>, DatabaseError>;
        fn list_all_routines() -> Result<Vec<Routine>, DatabaseError>;
        fn list_event_routines() -> Result<Vec<Routine>, DatabaseError>;
        fn list_due_cron_routines() -> Result<Vec<Routine>, DatabaseError>;
        fn update_routine(routine: &'a Routine) -> Result<(), DatabaseError>;
        fn update_routine_runtime(params: RoutineRuntimeUpdate<'a>) -> Result<(), DatabaseError>;
        fn delete_routine(id: Uuid) -> Result<bool, DatabaseError>;
        fn create_routine_run(run: &'a RoutineRun) -> Result<(), DatabaseError>;
        fn complete_routine_run(params: RoutineRunCompletion<'a>) -> Result<(), DatabaseError>;
        fn list_routine_runs(routine_id: Uuid, limit: i64) -> Result<Vec<RoutineRun>, DatabaseError>;
        fn count_running_routine_runs(routine_id: Uuid) -> Result<i64, DatabaseError>;
        fn link_routine_run_to_job(run_id: Uuid, job_id: Uuid) -> Result<(), DatabaseError>;
    }
}

// ---- ToolFailureStore ----

impl_db_forwarders! {
    dyn = ToolFailureStore,
    native = NativeToolFailureStore,
    methods = {
        fn record_tool_failure(tool_name: &'a str, error_message: &'a str) -> Result<(), DatabaseError>;
        fn get_broken_tools(threshold: i32) -> Result<Vec<BrokenTool>, DatabaseError>;
        fn mark_tool_repaired(tool_name: &'a str) -> Result<(), DatabaseError>;
        fn increment_repair_attempts(tool_name: &'a str) -> Result<(), DatabaseError>;
    }
}

// ---- SettingsStore ----

impl_db_forwarders! {
    dyn = SettingsStore,
    native = NativeSettingsStore,
    methods = {
        fn get_setting(user_id: UserId, key: SettingKey) -> Result<Option<serde_json::Value>, DatabaseError>;
        fn get_setting_full(user_id: UserId, key: SettingKey) -> Result<Option<SettingRow>, DatabaseError>;
        fn set_setting(user_id: UserId, key: SettingKey, value: &'a serde_json::Value) -> Result<(), DatabaseError>;
        fn delete_setting(user_id: UserId, key: SettingKey) -> Result<bool, DatabaseError>;
        fn list_settings(user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError>;
        fn get_all_settings(user_id: UserId) -> Result<HashMap<String, serde_json::Value>, DatabaseError>;
        fn set_all_settings(user_id: UserId, settings: &'a HashMap<String, serde_json::Value>) -> Result<(), DatabaseError>;
        fn has_settings(user_id: UserId) -> Result<bool, DatabaseError>;
    }
}

// ---- WorkspaceStore ----

impl_db_forwarders! {
    dyn = WorkspaceStore,
    native = NativeWorkspaceStore,
    methods = {
        fn get_document_by_path(user_id: &'a str, agent_id: Option<Uuid>, path: &'a str) -> Result<MemoryDocument, WorkspaceError>;
        fn get_document_by_id(id: Uuid) -> Result<MemoryDocument, WorkspaceError>;
        fn get_or_create_document_by_path(user_id: &'a str, agent_id: Option<Uuid>, path: &'a str) -> Result<MemoryDocument, WorkspaceError>;
        fn update_document(id: Uuid, content: &'a str) -> Result<(), WorkspaceError>;
        fn delete_document_by_path(user_id: &'a str, agent_id: Option<Uuid>, path: &'a str) -> Result<(), WorkspaceError>;
        fn list_directory(user_id: &'a str, agent_id: Option<Uuid>, directory: &'a str) -> Result<Vec<WorkspaceEntry>, WorkspaceError>;
        fn list_all_paths(user_id: &'a str, agent_id: Option<Uuid>) -> Result<Vec<String>, WorkspaceError>;
        fn list_documents(user_id: &'a str, agent_id: Option<Uuid>) -> Result<Vec<MemoryDocument>, WorkspaceError>;
        fn delete_chunks(document_id: Uuid) -> Result<(), WorkspaceError>;
        fn insert_chunk(params: InsertChunkParams<'a>) -> Result<Uuid, WorkspaceError>;
        fn update_chunk_embedding(chunk_id: Uuid, embedding: &'a [f32]) -> Result<(), WorkspaceError>;
        fn get_chunks_without_embeddings(user_id: &'a str, agent_id: Option<Uuid>, limit: usize) -> Result<Vec<MemoryChunk>, WorkspaceError>;
        fn hybrid_search(params: HybridSearchParams<'a>) -> Result<Vec<SearchResult>, WorkspaceError>;
    }
}

// ---- Database ----

impl_db_forwarders! {
    dyn = Database,
    native = NativeDatabase,
    methods = {
        fn run_migrations() -> Result<(), DatabaseError>;
    }
}
