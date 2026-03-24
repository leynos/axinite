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
        fn list_conversation_messages_paginated(conversation_id: Uuid, before: Option<DateTime<Utc>>, limit: i64) -> Result<(Vec<ConversationMessage>, bool), DatabaseError>;
        fn update_conversation_metadata_field(id: Uuid, key: &'a str, value: &'a serde_json::Value) -> Result<(), DatabaseError>;
        fn get_conversation_metadata(id: Uuid) -> Result<Option<serde_json::Value>, DatabaseError>;
        fn list_conversation_messages(conversation_id: Uuid) -> Result<Vec<ConversationMessage>, DatabaseError>;
        fn conversation_belongs_to_user(conversation_id: Uuid, user_id: &'a str) -> Result<bool, DatabaseError>;
    }
}

// ---- JobStore ----

impl<T: NativeJobStore> JobStore for T {
    fn save_job<'a>(&'a self, ctx: &'a JobContext) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeJobStore::save_job(self, ctx))
    }

    fn get_job<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<JobContext>, DatabaseError>> {
        Box::pin(NativeJobStore::get_job(self, id))
    }

    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeJobStore::update_job_status(
            self,
            id,
            status,
            failure_reason,
        ))
    }

    fn mark_job_stuck<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeJobStore::mark_job_stuck(self, id))
    }

    fn get_stuck_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<Uuid>, DatabaseError>> {
        Box::pin(NativeJobStore::get_stuck_jobs(self))
    }

    fn list_agent_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<AgentJobRecord>, DatabaseError>> {
        Box::pin(NativeJobStore::list_agent_jobs(self))
    }

    fn agent_job_summary<'a>(&'a self) -> DbFuture<'a, Result<AgentJobSummary, DatabaseError>> {
        Box::pin(NativeJobStore::agent_job_summary(self))
    }

    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>> {
        Box::pin(NativeJobStore::get_agent_job_failure_reason(self, id))
    }

    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeJobStore::save_action(self, job_id, action))
    }

    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ActionRecord>, DatabaseError>> {
        Box::pin(NativeJobStore::get_job_actions(self, job_id))
    }

    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>> {
        Box::pin(NativeJobStore::record_llm_call(self, record))
    }

    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>> {
        Box::pin(NativeJobStore::save_estimation_snapshot(self, params))
    }

    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeJobStore::update_estimation_actuals(self, params))
    }
}

// ---- SandboxStore ----

impl<T: NativeSandboxStore> SandboxStore for T {
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeSandboxStore::save_sandbox_job(self, job))
    }

    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<SandboxJobRecord>, DatabaseError>> {
        Box::pin(NativeSandboxStore::get_sandbox_job(self, id))
    }

    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>> {
        Box::pin(NativeSandboxStore::list_sandbox_jobs(self))
    }

    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeSandboxStore::update_sandbox_job_status(self, params))
    }

    fn cleanup_stale_sandbox_jobs<'a>(&'a self) -> DbFuture<'a, Result<u64, DatabaseError>> {
        Box::pin(NativeSandboxStore::cleanup_stale_sandbox_jobs(self))
    }

    fn sandbox_job_summary<'a>(&'a self) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>> {
        Box::pin(NativeSandboxStore::sandbox_job_summary(self))
    }

    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>> {
        Box::pin(NativeSandboxStore::list_sandbox_jobs_for_user(
            self, user_id,
        ))
    }

    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>> {
        Box::pin(NativeSandboxStore::sandbox_job_summary_for_user(
            self, user_id,
        ))
    }

    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(NativeSandboxStore::sandbox_job_belongs_to_user(
            self, job_id, user_id,
        ))
    }

    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeSandboxStore::update_sandbox_job_mode(self, id, mode))
    }

    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>> {
        Box::pin(NativeSandboxStore::get_sandbox_job_mode(self, id))
    }

    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: &'a str,
        data: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeSandboxStore::save_job_event(
            self, job_id, event_type, data,
        ))
    }

    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> DbFuture<'a, Result<Vec<JobEventRecord>, DatabaseError>> {
        Box::pin(NativeSandboxStore::list_job_events(
            self, job_id, before_id, limit,
        ))
    }
}

// ---- RoutineStore ----

impl<T: NativeRoutineStore> RoutineStore for T {
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::create_routine(self, routine))
    }

    fn get_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::get_routine(self, id))
    }

    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::get_routine_by_name(self, user_id, name))
    }

    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::list_routines(self, user_id))
    }

    fn list_all_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::list_all_routines(self))
    }

    fn list_event_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::list_event_routines(self))
    }

    fn list_due_cron_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>> {
        Box::pin(NativeRoutineStore::list_due_cron_routines(self))
    }

    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::update_routine(self, routine))
    }

    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::update_routine_runtime(self, params))
    }

    fn delete_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(NativeRoutineStore::delete_routine(self, id))
    }

    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::create_routine_run(self, run))
    }

    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::complete_routine_run(self, params))
    }

    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<RoutineRun>, DatabaseError>> {
        Box::pin(NativeRoutineStore::list_routine_runs(
            self, routine_id, limit,
        ))
    }

    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> DbFuture<'a, Result<i64, DatabaseError>> {
        Box::pin(NativeRoutineStore::count_running_routine_runs(
            self, routine_id,
        ))
    }

    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeRoutineStore::link_routine_run_to_job(
            self, run_id, job_id,
        ))
    }
}

// ---- ToolFailureStore ----

impl<T: NativeToolFailureStore> ToolFailureStore for T {
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeToolFailureStore::record_tool_failure(
            self,
            tool_name,
            error_message,
        ))
    }

    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> DbFuture<'a, Result<Vec<BrokenTool>, DatabaseError>> {
        Box::pin(NativeToolFailureStore::get_broken_tools(self, threshold))
    }

    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeToolFailureStore::mark_tool_repaired(self, tool_name))
    }

    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeToolFailureStore::increment_repair_attempts(
            self, tool_name,
        ))
    }
}

// ---- SettingsStore ----

impl<T> SettingsStore for T
where
    T: NativeSettingsStore + Send + Sync,
{
    fn get_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::get_setting(self, user_id, key).await })
    }

    fn get_setting_full<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<Option<SettingRow>, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::get_setting_full(self, user_id, key).await })
    }

    fn set_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::set_setting(self, user_id, key, value).await })
    }

    fn delete_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::delete_setting(self, user_id, key).await })
    }

    fn list_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<SettingRow>, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::list_settings(self, user_id).await })
    }

    fn get_all_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<HashMap<String, serde_json::Value>, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::get_all_settings(self, user_id).await })
    }

    fn set_all_settings<'a>(
        &'a self,
        user_id: &'a str,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(
            async move { NativeSettingsStore::set_all_settings(self, user_id, settings).await },
        )
    }

    fn has_settings<'a>(&'a self, user_id: &'a str) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(async move { NativeSettingsStore::has_settings(self, user_id).await })
    }
}

// ---- WorkspaceStore ----

impl<T: NativeWorkspaceStore> WorkspaceStore for T {
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::get_document_by_path(
            self, user_id, agent_id, path,
        ))
    }

    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::get_document_by_id(self, id))
    }

    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::get_or_create_document_by_path(
            self, user_id, agent_id, path,
        ))
    }

    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::update_document(self, id, content))
    }

    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::delete_document_by_path(
            self, user_id, agent_id, path,
        ))
    }

    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> DbFuture<'a, Result<Vec<WorkspaceEntry>, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::list_directory(
            self, user_id, agent_id, directory,
        ))
    }

    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<String>, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::list_all_paths(
            self, user_id, agent_id,
        ))
    }

    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<MemoryDocument>, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::list_documents(
            self, user_id, agent_id,
        ))
    }

    fn delete_chunks<'a>(&'a self, document_id: Uuid) -> DbFuture<'a, Result<(), WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::delete_chunks(self, document_id))
    }

    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::insert_chunk(self, params))
    }

    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> DbFuture<'a, Result<(), WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::update_chunk_embedding(
            self, chunk_id, embedding,
        ))
    }

    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<MemoryChunk>, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::get_chunks_without_embeddings(
            self, user_id, agent_id, limit,
        ))
    }

    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> DbFuture<'a, Result<Vec<SearchResult>, WorkspaceError>> {
        Box::pin(NativeWorkspaceStore::hybrid_search(self, params))
    }
}

// ---- Database ----

impl<T: NativeDatabase> Database for T {
    fn run_migrations<'a>(&'a self) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeDatabase::run_migrations(self))
    }
}
