//! Database-related error types.

/// Database-related errors.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection pool error: {0}")]
    Pool(String),

    #[error("Query failed: {0}")]
    Query(String),

    #[error("Entity not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Migration failed: {0}")]
    Migration(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[cfg(feature = "postgres")]
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[cfg(feature = "postgres")]
    #[error("Pool build error: {0}")]
    PoolBuild(#[from] deadpool_postgres::BuildError),

    #[cfg(feature = "postgres")]
    #[error("Pool runtime error: {0}")]
    PoolRuntime(#[from] deadpool_postgres::PoolError),

    #[cfg(feature = "libsql")]
    #[error("LibSQL error: {0}")]
    LibSql(#[from] libsql::Error),
}
