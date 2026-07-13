//! Database connection testing, schema migrations, and non-interactive
//! (quick-mode) database setup.

use super::*;

pub(super) struct LibsqlConnParams<'a> {
    pub(super) path: &'a std::path::Path,
    pub(super) turso_url: Option<&'a str>,
    pub(super) turso_token: Option<&'a str>,
}

impl SetupWizard {
    /// Test PostgreSQL connection and store the pool.
    ///
    /// After connecting, validates:
    /// 1. PostgreSQL version >= 15 (required for pgvector compatibility)
    /// 2. pgvector extension is available (required for embeddings/vector search)
    #[cfg(feature = "postgres")]
    pub(super) async fn test_database_connection_postgres(
        &mut self,
        url: &str,
    ) -> Result<(), SetupError> {
        let mut cfg = PoolConfig::new();
        cfg.url = Some(url.to_string());
        cfg.pool = Some(deadpool_postgres::PoolConfig {
            max_size: 5,
            ..Default::default()
        });

        let pool = crate::db::tls::create_pool(&cfg, crate::config::SslMode::from_env())
            .map_err(|e| SetupError::Database(format!("Failed to create pool: {}", e)))?;

        let client = pool
            .get()
            .await
            .map_err(|e| SetupError::Database(format!("Failed to connect: {}", e)))?;

        // Check PostgreSQL server version (need 15+ for pgvector)
        let version_row = client
            .query_one("SHOW server_version", &[])
            .await
            .map_err(|e| SetupError::Database(format!("Failed to query server version: {}", e)))?;
        let version_str: &str = version_row.get(0);
        let major_version = version_str
            .split('.')
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);

        const MIN_PG_MAJOR_VERSION: u32 = 15;

        if major_version < MIN_PG_MAJOR_VERSION {
            return Err(SetupError::Database(format!(
                "PostgreSQL {} detected. IronClaw requires PostgreSQL {} or later for pgvector support.\n\
                 Upgrade: https://www.postgresql.org/download/",
                version_str, MIN_PG_MAJOR_VERSION
            )));
        }

        // Check if pgvector extension is available
        let pgvector_row = client
            .query_opt(
                "SELECT 1 FROM pg_available_extensions WHERE name = 'vector'",
                &[],
            )
            .await
            .map_err(|e| {
                SetupError::Database(format!("Failed to check pgvector availability: {}", e))
            })?;

        if pgvector_row.is_none() {
            return Err(SetupError::Database(format!(
                "pgvector extension not found on your PostgreSQL server.\n\n\
                 Install it:\n  \
                 macOS:   brew install pgvector\n  \
                 Ubuntu:  apt install postgresql-{0}-pgvector\n  \
                 Docker:  use the pgvector/pgvector:pg{0} image\n  \
                 Source:  https://github.com/pgvector/pgvector#installation\n\n\
                 Then restart PostgreSQL and re-run: ironclaw onboard",
                major_version
            )));
        }

        self.db_pool = Some(pool);
        #[cfg(feature = "libsql")]
        {
            self.db_backend = None; // Clear stale libSQL handle
        }
        Ok(())
    }

    /// Test libSQL connection and store the backend.
    #[cfg(feature = "libsql")]
    pub(super) async fn test_database_connection_libsql(
        &mut self,
        params: LibsqlConnParams<'_>,
    ) -> Result<(), SetupError> {
        use crate::db::libsql::LibSqlBackend;

        let LibsqlConnParams {
            path,
            turso_url,
            turso_token,
        } = params;
        let db_path = path;

        let backend = if let (Some(url), Some(token)) = (turso_url, turso_token) {
            LibSqlBackend::new_remote_replica(db_path, url, token)
                .await
                .map_err(|e| SetupError::Database(format!("Failed to connect: {}", e)))?
        } else {
            LibSqlBackend::new_local(db_path)
                .await
                .map_err(|e| SetupError::Database(format!("Failed to open database: {}", e)))?
        };

        self.db_backend = Some(Arc::new(backend));
        #[cfg(feature = "postgres")]
        {
            self.db_pool = None; // Clear stale PostgreSQL handle
        }
        Ok(())
    }

    /// Run PostgreSQL migrations.
    #[cfg(feature = "postgres")]
    pub(super) async fn run_migrations_postgres(&self) -> Result<(), SetupError> {
        if let Some(ref pool) = self.db_pool {
            if !self.config.quick {
                print_info("Running migrations...");
            }
            tracing::debug!("Running PostgreSQL migrations...");

            let mut client = pool
                .get()
                .await
                .map_err(|e| SetupError::Database(format!("Pool error: {}", e)))?;

            crate::history::run_postgres_migrations(&mut client)
                .await
                .map_err(|e| SetupError::Database(format!("Migration failed: {}", e)))?;

            if !self.config.quick {
                print_success("Migrations applied");
            }
            tracing::debug!("PostgreSQL migrations applied");
        }
        Ok(())
    }

    /// Run libSQL migrations.
    #[cfg(feature = "libsql")]
    pub(super) async fn run_migrations_libsql(&self) -> Result<(), SetupError> {
        if let Some(ref backend) = self.db_backend {
            use crate::db::Database;

            if !self.config.quick {
                print_info("Running migrations...");
            }
            tracing::debug!("Running libSQL migrations...");

            backend
                .run_migrations()
                .await
                .map_err(|e| SetupError::Database(format!("Migration failed: {}", e)))?;

            if !self.config.quick {
                print_success("Migrations applied");
            }
            tracing::debug!("libSQL migrations applied");
        }
        Ok(())
    }

    /// Auto-setup database with zero prompts (quick mode).
    ///
    /// Uses existing env vars if present, otherwise defaults to libsql at the
    /// standard path. Falls back to the interactive `step_database()` only when
    /// just the postgres feature is compiled (can't auto-default postgres).
    pub(super) async fn auto_setup_database(&mut self) -> Result<(), SetupError> {
        // If DATABASE_URL or LIBSQL_PATH already set, respect existing config
        #[cfg(feature = "postgres")]
        let env_backend = std::env::var("DATABASE_BACKEND").ok();

        #[cfg(feature = "postgres")]
        if let Some(ref backend) = env_backend
            && is_postgres_backend(backend)
        {
            if let Ok(url) = std::env::var("DATABASE_URL") {
                print_info("Using existing PostgreSQL configuration");
                self.test_database_connection_postgres(&url).await?;
                self.run_migrations_postgres().await?;
                self.settings.database_backend = Some("postgres".to_string());
                self.settings.database_url = Some(url);
                return Ok(());
            }
            // Postgres configured but no URL — fall through to interactive
            return self.step_database().await;
        }

        #[cfg(feature = "postgres")]
        if let Ok(url) = std::env::var("DATABASE_URL") {
            print_info("Using existing PostgreSQL configuration");
            self.test_database_connection_postgres(&url).await?;
            self.run_migrations_postgres().await?;
            self.settings.database_backend = Some("postgres".to_string());
            self.settings.database_url = Some(url);
            return Ok(());
        }

        // Auto-default to libsql if the feature is compiled
        #[cfg(feature = "libsql")]
        {
            self.settings.database_backend = Some("libsql".to_string());

            let existing_path = std::env::var("LIBSQL_PATH")
                .ok()
                .or_else(|| self.settings.libsql_path.clone());

            let db_path = existing_path.unwrap_or_else(|| {
                crate::config::default_libsql_path()
                    .to_string_lossy()
                    .to_string()
            });

            let turso_url = std::env::var("LIBSQL_URL").ok();
            let turso_token = std::env::var("LIBSQL_AUTH_TOKEN").ok();

            self.test_database_connection_libsql(LibsqlConnParams {
                path: std::path::Path::new(&db_path),
                turso_url: turso_url.as_deref(),
                turso_token: turso_token.as_deref(),
            })
            .await?;

            self.run_migrations_libsql().await?;

            self.settings.libsql_path = Some(db_path.clone());
            if let Some(url) = turso_url {
                self.settings.libsql_url = Some(url);
            }

            print_success(&format!("Using embedded database at {}", db_path));
            return Ok(());
        }

        // Only postgres feature compiled — can't auto-default, use interactive
        #[allow(unreachable_code)]
        {
            self.step_database().await
        }
    }
}
