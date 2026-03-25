//! Database abstraction layer.
//!
//! Provides a backend-agnostic `Database` trait that unifies all persistence
//! operations. Two implementations exist behind feature flags:
//!
//! - `postgres` (default): Uses `deadpool-postgres` + `tokio-postgres`
//! - `libsql`: Uses libSQL (Turso's SQLite fork) for embedded/edge deployment
//!
//! The existing `Store`, `Repository`, `SecretsStore`, and `WasmToolStore`
//! types become thin wrappers that delegate to `Arc<dyn Database>`.

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "postgres")]
pub mod tls;

#[cfg(feature = "libsql")]
pub mod libsql;

#[cfg(feature = "libsql")]
pub mod libsql_migrations;

pub mod settings;

use std::sync::Arc;
use std::{future::Future, pin::Pin};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::agent::BrokenTool;
use crate::agent::routine::{Routine, RoutineRun, RunStatus};
use crate::context::{ActionRecord, JobContext, JobState};
use crate::error::DatabaseError;
use crate::error::WorkspaceError;
use crate::history::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary,
};
use crate::workspace::{MemoryChunk, MemoryDocument, WorkspaceEntry};
use crate::workspace::{SearchConfig, SearchResult};

/// Boxed future used at dyn-backed database trait boundaries.
pub type DbFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// Re-export settings types for backward compatibility
pub use settings::{NativeSettingsStore, SettingKey, SettingsStore, UserId};

// ==================== Parameter Structs ====================
//
// These structs reduce argument counts for database methods with many parameters.

/// Parameters for `ensure_conversation`.
pub struct EnsureConversationParams<'a> {
    pub id: Uuid,
    pub channel: &'a str,
    pub user_id: &'a str,
    pub thread_id: Option<&'a str>,
}

/// Parameters for `save_estimation_snapshot`.
pub struct EstimationSnapshotParams<'a> {
    pub job_id: Uuid,
    pub category: &'a str,
    pub tool_names: &'a [String],
    pub estimated_cost: Decimal,
    pub estimated_time_secs: i32,
    pub estimated_value: Decimal,
}

/// Parameters for `update_estimation_actuals`.
pub struct EstimationActualsParams {
    pub id: Uuid,
    pub actual_cost: Decimal,
    pub actual_time_secs: i32,
    pub actual_value: Option<Decimal>,
}

/// Parameters for `update_sandbox_job_status`.
pub struct SandboxJobStatusUpdate<'a> {
    pub id: Uuid,
    pub status: &'a str,
    pub success: Option<bool>,
    pub message: Option<&'a str>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Parameters for `update_routine_runtime`.
pub struct RoutineRuntimeUpdate<'a> {
    pub id: Uuid,
    pub last_run_at: DateTime<Utc>,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub state: &'a serde_json::Value,
}

/// Parameters for `complete_routine_run`.
pub struct RoutineRunCompletion<'a> {
    pub id: Uuid,
    pub status: RunStatus,
    pub result_summary: Option<&'a str>,
    pub tokens_used: Option<i32>,
}

/// Parameters for `insert_chunk`.
pub struct InsertChunkParams<'a> {
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: &'a str,
    pub embedding: Option<&'a [f32]>,
}

/// Parameters for `hybrid_search`.
pub struct HybridSearchParams<'a> {
    pub user_id: &'a str,
    pub agent_id: Option<Uuid>,
    pub query: &'a str,
    pub embedding: Option<&'a [f32]>,
    pub config: &'a SearchConfig,
}

// ==================== Newtypes ====================
/// Create a database backend from configuration, run migrations, and return it.
///
/// This is the shared helper for CLI commands and other call sites that need
/// a simple `Arc<dyn Database>` without retaining backend-specific handles
/// (e.g., `pg_pool` or `libsql_conn` for the secrets store). The main agent
/// startup in `main.rs` uses its own initialization block because it also
/// captures those backend-specific handles.
pub async fn connect_from_config(
    config: &crate::config::DatabaseConfig,
) -> Result<Arc<dyn Database>, DatabaseError> {
    let (db, _handles) = connect_with_handles(config).await?;
    Ok(db)
}

/// Backend-specific handles retained after database connection.
///
/// These are needed by satellite stores (e.g., `SecretsStore`) that require
/// a backend-specific handle rather than the generic `Arc<dyn Database>`.
#[derive(Default)]
pub struct DatabaseHandles {
    #[cfg(feature = "postgres")]
    pub pg_pool: Option<deadpool_postgres::Pool>,
    #[cfg(feature = "libsql")]
    pub libsql_db: Option<Arc<::libsql::Database>>,
}

/// Connect to the database, run migrations, and return both the generic
/// `Database` trait object and the backend-specific handles.
pub async fn connect_with_handles(
    config: &crate::config::DatabaseConfig,
) -> Result<(Arc<dyn Database>, DatabaseHandles), DatabaseError> {
    match config.backend {
        #[cfg(feature = "libsql")]
        crate::config::DatabaseBackend::LibSql => {
            use secrecy::ExposeSecret as _;

            let mut handles = DatabaseHandles::default();
            let default_path = crate::config::default_libsql_path();
            let db_path = config.libsql_path.as_deref().unwrap_or(&default_path);

            let backend = if let Some(ref url) = config.libsql_url {
                let token = config.libsql_auth_token.as_ref().ok_or_else(|| {
                    DatabaseError::Pool(
                        "LIBSQL_AUTH_TOKEN required when LIBSQL_URL is set".to_string(),
                    )
                })?;
                libsql::LibSqlBackend::new_remote_replica(db_path, url, token.expose_secret())
                    .await
                    .map_err(|e| DatabaseError::Pool(e.to_string()))?
            } else {
                libsql::LibSqlBackend::new_local(db_path)
                    .await
                    .map_err(|e| DatabaseError::Pool(e.to_string()))?
            };
            NativeDatabase::run_migrations(&backend).await?;
            tracing::info!("libSQL database connected and migrations applied");

            handles.libsql_db = Some(backend.shared_db());

            Ok((Arc::new(backend) as Arc<dyn Database>, handles))
        }
        #[cfg(feature = "postgres")]
        crate::config::DatabaseBackend::Postgres => {
            let mut handles = DatabaseHandles::default();
            let pg = postgres::PgBackend::new(config)
                .await
                .map_err(|e| DatabaseError::Pool(e.to_string()))?;
            NativeDatabase::run_migrations(&pg).await?;
            tracing::info!("PostgreSQL database connected and migrations applied");

            handles.pg_pool = Some(pg.pool());

            Ok((Arc::new(pg) as Arc<dyn Database>, handles))
        }
        #[cfg(not(feature = "postgres"))]
        crate::config::DatabaseBackend::Postgres => Err(DatabaseError::Pool(
            "postgres feature not enabled".to_string(),
        )),
        #[cfg(not(feature = "libsql"))]
        crate::config::DatabaseBackend::LibSql => Err(DatabaseError::Pool(
            "libsql feature not enabled".to_string(),
        )),
    }
}

/// Create a secrets store from database and secrets configuration.
///
/// This is the shared factory for CLI commands and other call sites that need
/// a `SecretsStore` without going through the full `AppBuilder`. Mirrors the
/// pattern of [`connect_from_config`] but returns a secrets-specific store.
pub async fn create_secrets_store(
    config: &crate::config::DatabaseConfig,
    crypto: Arc<crate::secrets::SecretsCrypto>,
) -> Result<Arc<dyn crate::secrets::SecretsStore + Send + Sync>, DatabaseError> {
    #[cfg(not(any(feature = "libsql", feature = "postgres")))]
    let _ = &crypto;

    let (_db, handles) = connect_with_handles(config).await?;

    #[cfg(not(any(feature = "libsql", feature = "postgres")))]
    let _ = &handles;

    match config.backend {
        #[cfg(feature = "libsql")]
        crate::config::DatabaseBackend::LibSql => {
            let libsql_db = handles.libsql_db.ok_or_else(|| {
                DatabaseError::Pool("libSQL handle missing after connect_with_handles".to_string())
            })?;

            Ok(Arc::new(crate::secrets::LibSqlSecretsStore::new(
                libsql_db, crypto,
            )))
        }
        #[cfg(feature = "postgres")]
        crate::config::DatabaseBackend::Postgres => {
            let pg_pool = handles.pg_pool.ok_or_else(|| {
                DatabaseError::Pool(
                    "PostgreSQL handle missing after connect_with_handles".to_string(),
                )
            })?;

            Ok(Arc::new(crate::secrets::PostgresSecretsStore::new(
                pg_pool, crypto,
            )))
        }
        #[cfg(not(feature = "postgres"))]
        crate::config::DatabaseBackend::Postgres => Err(DatabaseError::Pool(
            "postgres feature not enabled".to_string(),
        )),
        #[cfg(not(feature = "libsql"))]
        crate::config::DatabaseBackend::LibSql => Err(DatabaseError::Pool(
            "libsql feature not enabled".to_string(),
        )),
    }
}

// ==================== Sub-traits ====================
//
// Each sub-trait groups related persistence methods. The `Database` supertrait
// combines them all, so existing `Arc<dyn Database>` consumers keep working.
// Leaf consumers can depend on a specific sub-trait instead.

// ---- ConversationStore ----

/// Object-safe persistence surface for conversation history and metadata.
///
/// This trait provides the dyn-safe boundary for conversation storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn ConversationStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeConversationStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeConversationStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait ConversationStore: Send + Sync {
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn touch_conversation<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> DbFuture<'a, Result<(Vec<ConversationMessage>, bool), DatabaseError>>;
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ConversationMessage>, DatabaseError>>;
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete conversation-store implementations.
pub trait NativeConversationStore: Send + Sync {
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn touch_conversation<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> impl Future<Output = Result<(Vec<ConversationMessage>, bool), DatabaseError>> + Send + 'a;
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConversationMessage>, DatabaseError>> + Send + 'a;
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}

/// Generate blanket adapter implementations that forward dyn-safe trait methods
/// to native async trait methods via `Box::pin`.
///
/// This macro eliminates boilerplate for the ADR-006 dyn/native boundary pattern,
/// where each object-safe `*Store` trait has a companion `Native*Store` trait
/// with native async fn methods (RPITIT), and a blanket impl bridges the two.
///
/// ## Why this macro is only used for ConversationStore
///
/// The other store traits (JobStore, SandboxStore, RoutineStore, ToolFailureStore,
/// WorkspaceStore) require manual blanket impls because:
///
/// Note: SettingsStore now uses the `settings_delegate!` macro for its blanket impl.
///
/// 1. **Generic type parameters with lifetimes**: Some methods accept types like
///    `LlmCallRecord<'a>` that carry their own lifetime parameters, which the
///    current macro cannot handle (it only supports `&'a T` references).
///
/// 2. **Struct parameter patterns**: Methods that destructure struct parameters
///    (e.g., `SandboxJobStatusUpdate { id, status, ... }`) would require macro
///    support for struct destructuring in the forwarding call.
///
/// 3. **Complexity vs. benefit**: Extending the macro to handle all edge cases
///    (nested lifetimes, generic parameters, pattern destructuring) would make
///    it significantly more complex than the manual impls it replaces.
///
/// The manual impls are straightforward and follow the same Box::pin pattern,
/// making them easy to verify and maintain without macro indirection.
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

impl_db_forwarders! {
    dyn = crate::db::ConversationStore,
    native = crate::db::NativeConversationStore,
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

/// Object-safe persistence surface for agent jobs, LLM calls, and estimation snapshots.
///
/// This trait provides the dyn-safe boundary for job storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn JobStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeJobStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeJobStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait JobStore: Send + Sync {
    fn save_job<'a>(&'a self, ctx: &'a JobContext) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_job<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<JobContext>, DatabaseError>>;
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn mark_job_stuck<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_stuck_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<Uuid>, DatabaseError>>;
    fn list_agent_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<AgentJobRecord>, DatabaseError>>;
    fn agent_job_summary<'a>(&'a self) -> DbFuture<'a, Result<AgentJobSummary, DatabaseError>>;
    /// Get the failure reason for a single agent job (O(1) lookup).
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>>;
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ActionRecord>, DatabaseError>>;
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete job-store implementations.
pub trait NativeJobStore: Send + Sync {
    fn save_job<'a>(
        &'a self,
        ctx: &'a JobContext,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<JobContext>, DatabaseError>> + Send + 'a;
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn mark_job_stuck<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_stuck_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Uuid>, DatabaseError>> + Send + 'a;
    fn list_agent_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<AgentJobRecord>, DatabaseError>> + Send + 'a;
    fn agent_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<AgentJobSummary, DatabaseError>> + Send + 'a;
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, DatabaseError>> + Send + 'a;
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ActionRecord>, DatabaseError>> + Send + 'a;
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

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

/// Object-safe persistence surface for sandbox job lifecycle and events.
///
/// This trait provides the dyn-safe boundary for sandbox job storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn SandboxStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeSandboxStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeSandboxStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait SandboxStore: Send + Sync {
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<SandboxJobRecord>, DatabaseError>>;
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn cleanup_stale_sandbox_jobs<'a>(&'a self) -> DbFuture<'a, Result<u64, DatabaseError>>;
    fn sandbox_job_summary<'a>(&'a self) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>>;
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: &'a str,
        data: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load job events ordered by ascending id.
    ///
    /// When `before_id` is set, only events with ids strictly smaller than the
    /// cursor are returned. When `limit` is set, at most that many events are
    /// returned.
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> DbFuture<'a, Result<Vec<JobEventRecord>, DatabaseError>>;
}

/// Native async sibling trait for concrete sandbox-store implementations.
pub trait NativeSandboxStore: Send + Sync {
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn cleanup_stale_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<u64, DatabaseError>> + Send + 'a;
    fn sandbox_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, DatabaseError>> + Send + 'a;
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: &'a str,
        data: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> impl Future<Output = Result<Vec<JobEventRecord>, DatabaseError>> + Send + 'a;
}

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

/// Object-safe persistence surface for scheduled routines and their execution history.
///
/// This trait provides the dyn-safe boundary for routine storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn RoutineStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeRoutineStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeRoutineStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait RoutineStore: Send + Sync {
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_all_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_event_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_due_cron_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn delete_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<RoutineRun>, DatabaseError>>;
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> DbFuture<'a, Result<i64, DatabaseError>>;
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete routine-store implementations.
pub trait NativeRoutineStore: Send + Sync {
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_all_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_event_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_due_cron_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn delete_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RoutineRun>, DatabaseError>> + Send + 'a;
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> impl Future<Output = Result<i64, DatabaseError>> + Send + 'a;
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

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

/// Object-safe persistence surface for tool failure tracking and analysis.
///
/// This trait provides the dyn-safe boundary for tool failure storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn ToolFailureStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeToolFailureStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeToolFailureStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait ToolFailureStore: Send + Sync {
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> DbFuture<'a, Result<Vec<BrokenTool>, DatabaseError>>;
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete tool-failure-store implementations.
pub trait NativeToolFailureStore: Send + Sync {
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> impl Future<Output = Result<Vec<BrokenTool>, DatabaseError>> + Send + 'a;
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

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

// ---- SettingsStore (already migrated to DbFuture pattern) ----
// Moved to src/db/settings.rs

// ---- WorkspaceStore ----

/// Object-safe persistence surface for workspace documents, chunks, and semantic search.
///
/// This trait provides the dyn-safe boundary for workspace storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn WorkspaceStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeWorkspaceStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeWorkspaceStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
pub trait WorkspaceStore: Send + Sync {
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> DbFuture<'a, Result<Vec<WorkspaceEntry>, WorkspaceError>>;
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<String>, WorkspaceError>>;
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<MemoryDocument>, WorkspaceError>>;
    fn delete_chunks<'a>(&'a self, document_id: Uuid) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, WorkspaceError>>;
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<MemoryChunk>, WorkspaceError>>;
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> DbFuture<'a, Result<Vec<SearchResult>, WorkspaceError>>;
}

/// Native async sibling trait for concrete workspace-store implementations.
pub trait NativeWorkspaceStore: Send + Sync {
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> impl Future<Output = Result<Vec<WorkspaceEntry>, WorkspaceError>> + Send + 'a;
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<String>, WorkspaceError>> + Send + 'a;
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<MemoryDocument>, WorkspaceError>> + Send + 'a;
    fn delete_chunks<'a>(
        &'a self,
        document_id: Uuid,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> impl Future<Output = Result<Uuid, WorkspaceError>> + Send + 'a;
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemoryChunk>, WorkspaceError>> + Send + 'a;
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> impl Future<Output = Result<Vec<SearchResult>, WorkspaceError>> + Send + 'a;
}

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

// ---- Database supertrait ----

/// Backend-agnostic database supertrait.
///
/// Combines all sub-traits into one. Existing `Arc<dyn Database>` consumers
/// continue to work; leaf consumers can depend on a specific sub-trait instead.
pub trait Database:
    ConversationStore
    + JobStore
    + SandboxStore
    + RoutineStore
    + ToolFailureStore
    + SettingsStore
    + WorkspaceStore
    + Send
    + Sync
{
    /// Run schema migrations for this backend.
    fn run_migrations<'a>(&'a self) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete database implementations.
pub trait NativeDatabase:
    NativeConversationStore
    + NativeJobStore
    + NativeSandboxStore
    + NativeRoutineStore
    + NativeToolFailureStore
    + NativeSettingsStore
    + NativeWorkspaceStore
    + Send
    + Sync
{
    fn run_migrations<'a>(&'a self) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

impl<T: NativeDatabase> Database for T {
    fn run_migrations<'a>(&'a self) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(NativeDatabase::run_migrations(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: `create_secrets_store` selects the correct backend at
    /// runtime based on `DatabaseConfig`, not at compile time. Previously the
    /// CLI duplicated this logic with compile-time `#[cfg]` gates that always
    /// chose postgres when both features were enabled (PR #209).
    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn test_create_secrets_store_libsql_backend() {
        use secrecy::SecretString;

        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");

        let config = crate::config::DatabaseConfig {
            backend: crate::config::DatabaseBackend::LibSql,
            libsql_path: Some(db_path),
            libsql_url: None,
            libsql_auth_token: None,
            url: SecretString::from("unused://libsql".to_string()),
            pool_size: 1,
            ssl_mode: crate::config::SslMode::default(),
        };

        let master_key = SecretString::from("a]".repeat(16));
        let crypto = Arc::new(crate::secrets::SecretsCrypto::new(master_key).unwrap());

        let store = create_secrets_store(&config, crypto).await;
        assert!(
            store.is_ok(),
            "create_secrets_store should succeed for libsql backend"
        );

        // Verify basic operation works
        let store = store.unwrap();
        let exists = store.exists("test_user", "nonexistent_secret").await;
        assert!(exists.is_ok());
        assert!(!exists.unwrap());
    }
}
