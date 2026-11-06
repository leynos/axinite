//! Database-related error types.

/// Database-related errors.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    /// Obtaining or using a connection from the pool failed, or a backend could not be opened.
    #[error("Connection pool error: {0}")]
    Pool(String),

    /// A query could not be executed or its result could not be read.
    #[error("Query failed: {0}")]
    Query(String),

    /// The requested row does not exist.
    #[error("Entity not found: {entity} with id {id}")]
    NotFound {
        /// The kind of entity that was looked up, such as `agent_job` or `conversation`.
        entity: String,
        /// The identifier that was looked up, as text.
        id: String,
    },

    /// A uniqueness, foreign-key or other constraint rejected the write.
    #[error("Constraint violation: {0}")]
    Constraint(String),

    /// Applying schema migrations failed.
    #[error("Migration failed: {0}")]
    Migration(String),

    /// A value could not be encoded for storage or decoded from it.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// A caller-supplied value was rejected, such as an out-of-range pagination limit.
    #[error("Validation error: {0}")]
    Validation(String),

    /// An error from the tokio-postgres driver.
    #[cfg(feature = "postgres")]
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    /// The deadpool-postgres pool could not be built from its configuration.
    #[cfg(feature = "postgres")]
    #[error("Pool build error: {0}")]
    PoolBuild(#[from] deadpool_postgres::BuildError),

    /// A deadpool-postgres pool operation failed at runtime.
    #[cfg(feature = "postgres")]
    #[error("Pool runtime error: {0}")]
    PoolRuntime(#[from] deadpool_postgres::PoolError),

    /// An error from the libSQL driver.
    #[cfg(feature = "libsql")]
    #[error("LibSQL error: {0}")]
    LibSql(#[from] libsql::Error),
}
