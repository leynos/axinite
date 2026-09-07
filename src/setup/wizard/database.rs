//! Step 1: interactive database selection and reconnection to an
//! existing database.

#[cfg(feature = "libsql")]
use super::database_ops::LibsqlConnParams;
#[cfg(feature = "libsql")]
use super::database_prompts::prompt_turso_sync;
use super::*;

mod reconnect;

impl SetupWizard {
    /// Step 1: Database connection.
    pub(super) async fn step_database(&mut self) -> Result<(), SetupError> {
        // When both features are compiled, let the user choose.
        // If DATABASE_BACKEND is already set in the environment, respect it.
        #[cfg(all(feature = "postgres", feature = "libsql"))]
        {
            // Check if a backend is already pinned via env var
            if let Ok(backend) = {
                #[expect(clippy::disallowed_methods, reason = "transitional #333: EnvContext")]
                std::env::var("DATABASE_BACKEND")
            } {
                return self.step_database_from_env(&backend).await;
            }

            match self.choose_database_backend()? {
                1 => return self.step_database_libsql().await,
                _ => return self.step_database_postgres().await,
            }
        }

        #[cfg(all(feature = "postgres", not(feature = "libsql")))]
        {
            return self.step_database_postgres().await;
        }

        #[cfg(all(feature = "libsql", not(feature = "postgres")))]
        {
            return self.step_database_libsql().await;
        }
    }

    /// Dispatch to the backend pinned by `DATABASE_BACKEND`, defaulting to
    /// PostgreSQL for unknown values.
    #[cfg(all(feature = "postgres", feature = "libsql"))]
    async fn step_database_from_env(&mut self, backend: &str) -> Result<(), SetupError> {
        if is_libsql_backend(backend) {
            return self.step_database_libsql().await;
        }
        if !is_postgres_backend(backend) {
            print_info(&format!(
                "Unknown DATABASE_BACKEND '{}', defaulting to PostgreSQL",
                backend
            ));
        }
        self.step_database_postgres().await
    }

    /// Ask the user to pick a database backend, clearing stale connection
    /// settings when the choice differs from the previously saved backend.
    ///
    /// Returns the selected option index (0 = PostgreSQL, 1 = libSQL).
    #[cfg(all(feature = "postgres", feature = "libsql"))]
    fn choose_database_backend(&mut self) -> Result<usize, SetupError> {
        let pre_selected = self.settings.database_backend.as_deref().map(|b| match b {
            "libsql" | "turso" | "sqlite" => 1,
            _ => 0,
        });

        print_info("Which database backend would you like to use?");
        println!();

        let options = &[
            "PostgreSQL  - production-grade, requires a running server",
            "libSQL      - embedded SQLite, zero dependencies, optional Turso cloud sync",
        ];
        let choice = select_one("Select a database backend:", options).map_err(SetupError::Io)?;

        // If the user picked something different from what was pre-selected, clear
        // stale connection settings so the next step starts fresh.
        if let Some(prev) = pre_selected
            && prev != choice
        {
            self.settings.database_url = None;
            self.settings.libsql_path = None;
            self.settings.libsql_url = None;
        }

        Ok(choice)
    }

    /// Step 1 (postgres): Database connection via PostgreSQL URL.
    #[cfg(feature = "postgres")]
    async fn step_database_postgres(&mut self) -> Result<(), SetupError> {
        self.settings.database_backend = Some("postgres".to_string());

        if self.try_reuse_existing_postgres_url().await? {
            return Ok(());
        }

        println!();
        print_info("Enter your PostgreSQL connection URL.");
        print_info("Format: postgres://user:password@host:port/database");
        println!();

        self.prompt_postgres_url().await
    }

    /// Offer to reuse an existing PostgreSQL URL from the environment or
    /// saved settings.
    ///
    /// Returns `true` when the existing URL was accepted and verified.
    #[cfg(feature = "postgres")]
    async fn try_reuse_existing_postgres_url(&mut self) -> Result<bool, SetupError> {
        let existing_url = {
            #[expect(clippy::disallowed_methods, reason = "transitional #333: EnvContext")]
            std::env::var("DATABASE_URL")
        }
        .ok()
        .or_else(|| self.settings.database_url.clone());

        let Some(url) = existing_url else {
            return Ok(false);
        };

        let display_url = mask_password_in_url(&url);
        print_info(&format!("Existing database URL: {}", display_url));

        if !confirm("Use this database?", true).map_err(SetupError::Io)? {
            return Ok(false);
        }

        if let Err(e) = self.test_database_connection_postgres(&url).await {
            print_error(&format!("Connection failed: {}", e));
            print_info("Let's configure a new database URL.");
            return Ok(false);
        }

        self.run_migrations_postgres().await?;
        print_success("Database connection successful");
        self.settings.database_url = Some(url);
        Ok(true)
    }

    /// Prompt for a PostgreSQL URL until a connection succeeds or the user
    /// gives up.
    #[cfg(feature = "postgres")]
    async fn prompt_postgres_url(&mut self) -> Result<(), SetupError> {
        loop {
            let url = input("Database URL").map_err(SetupError::Io)?;

            if url.is_empty() {
                print_error("Database URL is required.");
                continue;
            }

            if self.try_postgres_url(&url).await? {
                return Ok(());
            }
        }
    }

    /// Test one candidate PostgreSQL URL, running migrations and saving the
    /// URL on success.
    ///
    /// Returns `Ok(true)` when the URL was accepted, `Ok(false)` to prompt
    /// again, and `Err` when the user gives up after a failed connection.
    #[cfg(feature = "postgres")]
    async fn try_postgres_url(&mut self, url: &str) -> Result<bool, SetupError> {
        print_info("Testing connection...");
        if let Err(e) = self.test_database_connection_postgres(url).await {
            print_error(&format!("Connection failed: {}", e));
            if confirm("Try again?", true).map_err(SetupError::Io)? {
                return Ok(false);
            }
            return Err(SetupError::Database(
                "Database connection failed".to_string(),
            ));
        }

        print_success("Database connection successful");
        if confirm("Run database migrations?", true).map_err(SetupError::Io)? {
            self.run_migrations_postgres().await?;
        }
        self.settings.database_url = Some(url.to_string());
        Ok(true)
    }

    /// Step 1 (libsql): Database connection via local file or Turso remote replica.
    #[cfg(feature = "libsql")]
    async fn step_database_libsql(&mut self) -> Result<(), SetupError> {
        self.settings.database_backend = Some("libsql".to_string());

        if self.try_reuse_existing_libsql_path().await? {
            return Ok(());
        }

        println!();
        print_info("Axinite uses an embedded SQLite database (libSQL).");
        print_info("No external database server required.");
        println!();

        let default_path_str = crate::config::default_libsql_path()
            .to_string_lossy()
            .to_string();
        let path_input = optional_input(
            "Database file path",
            Some(&format!("default: {}", default_path_str)),
        )
        .map_err(SetupError::Io)?;

        let db_path = path_input.unwrap_or(default_path_str);

        // Ask about Turso cloud sync
        println!();
        let (turso_url, turso_token) = prompt_turso_sync()?;

        self.connect_new_libsql(db_path, turso_url, turso_token)
            .await
    }

    /// Offer to reuse an existing libSQL path from the environment or saved
    /// settings.
    ///
    /// Returns `true` when the existing configuration was accepted and verified.
    #[cfg(feature = "libsql")]
    async fn try_reuse_existing_libsql_path(&mut self) -> Result<bool, SetupError> {
        let existing_path = {
            #[expect(clippy::disallowed_methods, reason = "transitional #333: EnvContext")]
            std::env::var("LIBSQL_PATH")
        }
        .ok()
        .or_else(|| self.settings.libsql_path.clone());

        let Some(path) = existing_path else {
            return Ok(false);
        };

        print_info(&format!("Existing database path: {}", path));
        if !confirm("Use this database?", true).map_err(SetupError::Io)? {
            return Ok(false);
        }

        let turso_url = {
            #[expect(clippy::disallowed_methods, reason = "transitional #333: EnvContext")]
            std::env::var("LIBSQL_URL")
        }
        .ok()
        .or_else(|| self.settings.libsql_url.clone());
        let turso_token = {
            #[expect(clippy::disallowed_methods, reason = "transitional #333: EnvContext")]
            std::env::var("LIBSQL_AUTH_TOKEN")
        }
        .ok();

        match self
            .test_database_connection_libsql(LibsqlConnParams {
                path: std::path::Path::new(&path),
                turso_url: turso_url.as_deref(),
                turso_token: turso_token.as_deref(),
            })
            .await
        {
            Ok(()) => {
                print_success("Database connection successful");
                self.settings.libsql_path = Some(path);
                if let Some(url) = turso_url {
                    self.settings.libsql_url = Some(url);
                }
                Ok(true)
            }
            Err(e) => {
                print_error(&format!("Connection failed: {}", e));
                print_info("Let's configure a new database path.");
                Ok(false)
            }
        }
    }

    /// Verify a freshly configured libSQL database, run migrations, and save
    /// the connection settings.
    #[cfg(feature = "libsql")]
    async fn connect_new_libsql(
        &mut self,
        db_path: String,
        turso_url: Option<String>,
        turso_token: Option<String>,
    ) -> Result<(), SetupError> {
        print_info("Testing connection...");
        match self
            .test_database_connection_libsql(LibsqlConnParams {
                path: std::path::Path::new(&db_path),
                turso_url: turso_url.as_deref(),
                turso_token: turso_token.as_deref(),
            })
            .await
        {
            Ok(()) => {
                print_success("Database connection successful");

                // Always run migrations for libsql (they're idempotent)
                self.run_migrations_libsql().await?;

                self.settings.libsql_path = Some(db_path);
                if let Some(url) = turso_url {
                    self.settings.libsql_url = Some(url);
                }
                Ok(())
            }
            Err(e) => Err(SetupError::Database(format!("Connection failed: {}", e))),
        }
    }
}
