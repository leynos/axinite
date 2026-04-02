//! PostgreSQL-backed history store split by persistence domain.

#[cfg(feature = "postgres")]
use deadpool_postgres::{Config, Pool};

#[cfg(feature = "postgres")]
use crate::config::DatabaseConfig;
#[cfg(feature = "postgres")]
use crate::context::JobState;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

mod actions;
mod conversations;
mod estimation;
mod jobs;
mod llm_calls;
mod routines;
mod sandbox;
mod settings;
mod tools;

pub use conversations::{ConversationMessage, ConversationSummary};
pub use jobs::{AgentJobRecord, AgentJobSummary};
pub use llm_calls::LlmCallRecord;
pub use sandbox::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};
pub use settings::SettingRow;

/// Database store for the agent.
#[cfg(feature = "postgres")]
pub struct Store {
    pool: Pool,
}

#[cfg(feature = "postgres")]
impl Store {
    /// Wrap an existing pool (useful when the caller already has a connection).
    pub fn from_pool(pool: Pool) -> Self {
        Self { pool }
    }

    /// Create a new store and connect to the database.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let mut cfg = Config::new();
        cfg.url = Some(config.url().to_string());
        cfg.pool = Some(deadpool_postgres::PoolConfig {
            max_size: config.pool_size,
            ..Default::default()
        });

        let pool = crate::db::tls::create_pool(&cfg, config.ssl_mode)
            .map_err(|e| DatabaseError::Pool(e.to_string()))?;

        let _ = pool.get().await?;

        Ok(Self { pool })
    }

    /// Run database migrations (embedded via refinery).
    pub async fn run_migrations(&self) -> Result<(), DatabaseError> {
        use refinery::embed_migrations;
        embed_migrations!("migrations");

        let mut client = self.pool.get().await?;
        migrations::runner()
            .run_async(&mut **client)
            .await
            .map_err(|e| DatabaseError::Migration(e.to_string()))?;
        Ok(())
    }

    /// Get a connection from the pool.
    pub async fn conn(&self) -> Result<deadpool_postgres::Object, DatabaseError> {
        Ok(self.pool.get().await?)
    }

    /// Get a clone of the database pool.
    ///
    /// Useful for sharing the pool with other components like Workspace.
    pub fn pool(&self) -> Pool {
        self.pool.clone()
    }
}

#[cfg(feature = "postgres")]
fn parse_job_state(s: &str) -> Result<JobState, DatabaseError> {
    s.parse()
        .map_err(|()| DatabaseError::Serialization(format!("invalid persisted job state '{s}'")))
}
