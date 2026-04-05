//! Database abstraction layer.
//!
//! Provides a backend-agnostic `Database` trait that unifies all persistence
//! operations.  Two implementations exist behind feature flags:
//!
//! - `postgres` (default): Uses `deadpool-postgres` + `tokio-postgres`
//! - `libsql`: Uses libSQL (Turso's SQLite fork) for embedded/edge deployment
//!
//! Trait definitions live in [`traits`], parameter objects and the
//! boxed-future alias in [`params`], and the blanket dyn↔native adapters
//! in the private `forwarders` module.  This file re-exports the public API
//! so that external code continues to use `crate::db::{…}`.

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "postgres")]
pub mod tls;

#[cfg(feature = "libsql")]
pub mod libsql;

#[cfg(feature = "libsql")]
pub mod libsql_migrations;

pub mod settings;

mod params;
pub use params::*;

mod traits;
pub use traits::{
    ConversationStore, Database, JobStore, NativeConversationStore, NativeDatabase, NativeJobStore,
    NativeRoutineStore, NativeSandboxStore, NativeSettingsStore, NativeToolFailureStore,
    NativeWorkspaceStore, RoutineStore, SandboxStore, SettingsStore, ToolFailureStore,
    WorkspaceStore,
};

mod types;
pub use types::UserId;

mod forwarders;

use std::sync::Arc;

use crate::error::DatabaseError;

/// Unified macro for delegating async trait methods to an inner field.
#[cfg(feature = "postgres")]
macro_rules! delegate_async {
    (
        to $field:ident;
        $(
            async fn $method:ident ( &self $(, $arg:ident : $ty:ty)* ) -> $ret:ty ;
        )*
    ) => {
        $(
            async fn $method(&self $(, $arg : $ty )*) -> $ret {
                self.$field.$method($( $arg ),*).await
            }
        )*
    };
}

#[cfg(feature = "postgres")]
pub(crate) use delegate_async;

/// Create a database backend from configuration, run migrations, and return it.
///
/// This is the shared helper for CLI commands and other call sites that need
/// a simple `Arc<dyn Database>` without retaining backend-specific handles
/// (e.g., `pg_pool` or `libsql_conn` for the secrets store).  The main agent
/// startup in `main.rs` uses its own initialisation block because it also
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
/// a `SecretsStore` without going through the full `AppBuilder`.  Mirrors the
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: `create_secrets_store` selects the correct backend at
    /// runtime based on `DatabaseConfig`, not at compile time.  Previously the
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
