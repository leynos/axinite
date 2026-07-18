//! Wizard construction, defaults, and top-level step orchestration.

use super::*;

impl SetupWizard {
    /// Create a new setup wizard.
    pub fn new() -> Self {
        Self {
            config: SetupConfig::default(),
            settings: Settings::default(),
            session_manager: None,
            #[cfg(feature = "postgres")]
            db_pool: None,
            #[cfg(feature = "libsql")]
            db_backend: None,
            secrets_crypto: None,
            llm_api_key: None,
        }
    }

    /// Create a wizard with custom configuration.
    pub fn with_config(config: SetupConfig) -> Self {
        Self {
            config,
            settings: Settings::default(),
            session_manager: None,
            #[cfg(feature = "postgres")]
            db_pool: None,
            #[cfg(feature = "libsql")]
            db_backend: None,
            secrets_crypto: None,
            llm_api_key: None,
        }
    }

    /// Set the session manager (for reusing existing auth).
    pub fn with_session(mut self, session: Arc<SessionManager>) -> Self {
        self.session_manager = Some(Arc::clone(&session));
        self
    }

    /// Construct a DefaultSettingsPersistence from the current database backend.
    ///
    /// Returns None if no backend has been initialized yet.
    /// Routes based on `settings.database_backend` to avoid misrouting when both handles exist.
    pub(super) fn default_settings_persistence(&self) -> Option<DefaultSettingsPersistence> {
        match self.settings.database_backend.as_deref() {
            #[cfg(feature = "postgres")]
            Some("postgres") => {
                if let Some(ref pool) = self.db_pool {
                    let backend = Arc::new(crate::db::postgres::PgBackend::from_pool(pool.clone()));
                    return Some(DefaultSettingsPersistence::new(backend));
                }
            }
            #[cfg(feature = "libsql")]
            Some("libsql") => {
                if let Some(ref backend) = self.db_backend {
                    return Some(DefaultSettingsPersistence::new(backend.clone()));
                }
            }
            _ => {}
        }

        None
    }

    /// Run the setup wizard.
    ///
    /// Settings are persisted incrementally after each successful step so
    /// that progress is not lost if a later step fails. On re-run, existing
    /// settings are loaded from the database after Step 1 establishes a
    /// connection, so users don't have to re-enter everything.
    pub async fn run(&mut self) -> Result<(), SetupError> {
        print_header("Axinite Setup Wizard");

        if self.config.channels_only {
            // Channels-only mode: reconnect to existing DB and load settings
            // before running the channel step, so secrets and save work.
            self.reconnect_existing_db().await?;
            print_step(1, 1, "Channel Configuration");
            self.step_channels().await?;
        } else if self.config.provider_only {
            // Provider-only mode: reconnect to existing DB, then run just
            // inference provider + model selection steps.
            self.reconnect_existing_db().await?;
            print_step(1, 2, "Inference Provider");
            self.step_inference_provider().await?;
            self.persist_after_step().await;
            print_step(2, 2, "Model Selection");
            self.step_model_selection().await?;
            self.persist_after_step().await;
        } else if self.config.quick {
            // Quick mode: auto-default database + security, only ask for
            // LLM provider + model. Designed for first-run experience.
            self.auto_setup_database().await?;

            // Load existing settings from DB (if any prior partial run)
            let step1_settings = self.settings.clone();
            self.try_load_existing_settings().await;
            self.settings.merge_from(&step1_settings);

            self.auto_setup_security().await?;
            self.persist_after_step().await;

            print_step(1, 2, "Inference Provider");
            self.step_inference_provider().await?;
            self.persist_after_step().await;

            print_step(2, 2, "Model Selection");
            self.step_model_selection().await?;
            self.persist_after_step().await;
        } else {
            let total_steps = 9;

            // Step 1: Database
            print_step(1, total_steps, "Database Connection");
            self.step_database().await?;

            // After establishing a DB connection, load any previously saved
            // settings so we recover progress from prior partial runs.
            // We must load BEFORE persisting, otherwise persist_after_step()
            // would overwrite prior settings with defaults.
            // Save Step 1 choices first so they aren't clobbered by stale
            // DB values (merge_from only applies non-default fields).
            let step1_settings = self.settings.clone();
            self.try_load_existing_settings().await;
            self.settings.merge_from(&step1_settings);

            self.persist_after_step().await;

            // Step 2: Security
            print_step(2, total_steps, "Security");
            self.step_security().await?;
            self.persist_after_step().await;

            // Step 3: Inference provider selection (unless skipped)
            if !self.config.skip_auth {
                print_step(3, total_steps, "Inference Provider");
                self.step_inference_provider().await?;
            } else {
                print_info("Skipping inference provider setup (using existing config)");
            }
            self.persist_after_step().await;

            // Step 4: Model selection
            print_step(4, total_steps, "Model Selection");
            self.step_model_selection().await?;
            self.persist_after_step().await;

            // Step 5: Embeddings
            print_step(5, total_steps, "Embeddings (Semantic Search)");
            self.step_embeddings()?;
            self.persist_after_step().await;

            // Step 6: Channel configuration
            print_step(6, total_steps, "Channel Configuration");
            self.step_channels().await?;
            self.persist_after_step().await;

            // Step 7: Extensions (tools)
            print_step(7, total_steps, "Extensions");
            self.step_extensions().await?;

            // Step 8: Docker Sandbox
            print_step(8, total_steps, "Docker Sandbox");
            self.step_docker_sandbox().await?;
            self.persist_after_step().await;

            // Step 9: Heartbeat
            print_step(9, total_steps, "Background Tasks");
            self.step_heartbeat()?;
            self.persist_after_step().await;
        }

        // Save settings and print summary
        self.save_and_summarize().await?;

        Ok(())
    }
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}
