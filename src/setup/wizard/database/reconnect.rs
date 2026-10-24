//! Reconnection helpers for the database setup step.

use super::super::database_ops::LibsqlConnParams;
use super::super::*;

impl SetupWizard {
    /// Reconnect to the existing database and load settings.
    ///
    /// Used by channels-only mode so the wizard has a live database connection
    /// and settings reflect the saved configuration.
    pub(in crate::setup::wizard) async fn reconnect_existing_db(
        &mut self,
    ) -> Result<(), SetupError> {
        let backend = self
            .env_context
            .get_owned("DATABASE_BACKEND")
            .unwrap_or_else(|| "postgres".to_string());

        #[cfg(feature = "libsql")]
        if is_libsql_backend(&backend) {
            return self.reconnect_libsql().await;
        }

        #[cfg(feature = "postgres")]
        {
            let _ = &backend;
            return self.reconnect_postgres().await;
        }

        #[cfg(not(feature = "postgres"))]
        Err(SetupError::Database(
            "No database configured. Run full setup first (axinite onboard).".to_string(),
        ))
    }

    #[cfg(feature = "postgres")]
    async fn reconnect_postgres(&mut self) -> Result<(), SetupError> {
        let url = self.env_context.get_owned("DATABASE_URL").ok_or_else(|| {
            SetupError::Database(
                "DATABASE_URL not set. Run full setup first (axinite onboard).".to_string(),
            )
        })?;

        self.test_database_connection_postgres(&url).await?;
        self.settings.database_backend = Some("postgres".to_string());
        self.settings.database_url = Some(url.clone());
        if let Some(saved) = self.load_persisted_settings().await {
            self.settings = saved;
            self.settings.database_backend = Some("postgres".to_string());
            self.settings.database_url = Some(url);
        }
        Ok(())
    }

    async fn load_persisted_settings(&self) -> Option<Settings> {
        let persistence = self.default_settings_persistence()?;
        let map = persistence.get_all_settings_map().await.ok()?;
        Some(Settings::from_db_map(&map))
    }

    #[cfg(feature = "libsql")]
    async fn reconnect_libsql(&mut self) -> Result<(), SetupError> {
        let path = self
            .env_context
            .get_owned("LIBSQL_PATH")
            .unwrap_or_else(|| {
                crate::config::default_libsql_path()
                    .to_string_lossy()
                    .to_string()
            });
        let turso_url = self.env_context.get_owned("LIBSQL_URL");
        let turso_token = self.env_context.get_owned("LIBSQL_AUTH_TOKEN");
        self.test_database_connection_libsql(LibsqlConnParams {
            path: std::path::Path::new(&path),
            turso_url: turso_url.as_deref(),
            turso_token: turso_token.as_deref(),
        })
        .await?;
        self.apply_libsql_settings(&path, turso_url.as_deref());
        if let Some(saved) = self.load_persisted_settings().await {
            self.settings = saved;
            self.apply_libsql_settings(&path, turso_url.as_deref());
        }
        Ok(())
    }

    #[cfg(feature = "libsql")]
    fn apply_libsql_settings(&mut self, path: &str, turso_url: Option<&str>) {
        self.settings.database_backend = Some("libsql".to_string());
        self.settings.libsql_path = Some(path.to_string());
        if let Some(url) = turso_url {
            self.settings.libsql_url = Some(url.to_string());
        }
    }
}
