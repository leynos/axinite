//! Step 2: security (secrets master key) setup and secrets-store
//! context initialization.

use super::*;

/// Hex-encode raw master key bytes for use with [`SecretsCrypto`].
fn hex_encode_key(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Build a shared [`SecretsCrypto`] from a hex-encoded master key.
fn build_secrets_crypto(key_hex: String) -> Result<Arc<SecretsCrypto>, SetupError> {
    Ok(Arc::new(
        SecretsCrypto::new(SecretString::from(key_hex))
            .map_err(|e| SetupError::Config(e.to_string()))?,
    ))
}

/// Default database backend for secrets storage: whichever backend is
/// compiled in, preferring PostgreSQL. When only libsql is available, we
/// must not default to "postgres" or we'd skip store creation.
fn default_secrets_backend() -> &'static str {
    #[cfg(feature = "postgres")]
    {
        "postgres"
    }
    #[cfg(not(feature = "postgres"))]
    {
        "libsql"
    }
}

impl SetupWizard {
    /// Step 2: Security (secrets master key).
    pub(super) async fn step_security(&mut self) -> Result<(), SetupError> {
        // Check current configuration
        let env_key_exists = std::env::var("SECRETS_MASTER_KEY").is_ok();

        if env_key_exists {
            print_info("Secrets master key found in SECRETS_MASTER_KEY environment variable.");
            self.settings.secrets_master_key_source = KeySource::Env;
            print_success("Security configured (env var)");
            return Ok(());
        }

        // Try to retrieve existing key from keychain. We use get_master_key()
        // instead of has_master_key() so we can cache the key bytes and build
        // SecretsCrypto eagerly, avoiding redundant keychain accesses later
        // (each access triggers macOS system dialogs).
        print_info("Checking OS keychain for existing master key...");
        if let Ok(keychain_key_bytes) = crate::secrets::keychain::get_master_key().await {
            self.secrets_crypto = Some(build_secrets_crypto(hex_encode_key(&keychain_key_bytes))?);

            print_info("Existing master key found in OS keychain.");
            if confirm("Use existing keychain key?", true).map_err(SetupError::Io)? {
                self.settings.secrets_master_key_source = KeySource::Keychain;
                print_success("Security configured (keychain)");
                return Ok(());
            }
            // User declined the existing key; clear the cached crypto so a fresh
            // key can be generated below.
            self.secrets_crypto = None;
        }

        // Offer options
        println!();
        print_info("The secrets master key encrypts sensitive data like API tokens.");
        print_info("Choose where to store it:");
        println!();

        let options = [
            "OS Keychain (recommended for local installs)",
            "Environment variable (for CI/Docker)",
            "Skip (disable secrets features)",
        ];

        let choice = select_one("Select storage method:", &options).map_err(SetupError::Io)?;

        match choice {
            0 => {
                // Generate and store in keychain
                print_info("Generating master key...");
                let key = crate::secrets::keychain::generate_master_key();

                crate::secrets::keychain::store_master_key(&key)
                    .await
                    .map_err(|e| {
                        SetupError::Config(format!("Failed to store in keychain: {}", e))
                    })?;

                // Also create crypto instance
                self.secrets_crypto = Some(build_secrets_crypto(hex_encode_key(&key))?);

                self.settings.secrets_master_key_source = KeySource::Keychain;
                print_success("Master key generated and stored in OS keychain");
            }
            1 => {
                // Env var mode — generate key, init crypto, and persist to .env
                let key_hex = crate::secrets::keychain::generate_master_key_hex();

                // Initialize crypto so subsequent wizard steps (channel setup,
                // API key storage) can encrypt secrets immediately.
                self.secrets_crypto = Some(build_secrets_crypto(key_hex.clone())?);

                // Make visible to optional_env() for any subsequent config resolution.
                crate::config::inject_single_var("SECRETS_MASTER_KEY", &key_hex);

                // Store hex for write_bootstrap_env to persist to ~/.ironclaw/.env.
                self.settings.secrets_master_key_hex = Some(key_hex.clone());

                println!();
                print_info("Master key generated and will be saved to ~/.ironclaw/.env");
                println!();
                println!("  SECRETS_MASTER_KEY={}", key_hex);
                println!();
                print_info("You can also copy this to another .env file or CI secrets.");

                self.settings.secrets_master_key_source = KeySource::Env;
                print_success("Configured for environment variable");
            }
            _ => {
                self.settings.secrets_master_key_source = KeySource::None;
                print_info("Secrets features disabled. Channel tokens must be set via env vars.");
            }
        }

        Ok(())
    }

    /// Auto-setup security with zero prompts (quick mode).
    ///
    /// Silently configures the master key: uses existing env var or keychain
    /// key if available, otherwise generates and stores one automatically
    /// (keychain on macOS, env var fallback).
    pub(super) async fn auto_setup_security(&mut self) -> Result<(), SetupError> {
        // Check env var first
        if std::env::var("SECRETS_MASTER_KEY").is_ok() {
            self.settings.secrets_master_key_source = KeySource::Env;
            print_success("Security configured (env var)");
            return Ok(());
        }

        // Try existing keychain key (no prompts — get_master_key may show
        // OS dialogs on macOS, but that's unavoidable for keychain access)
        if let Ok(keychain_key_bytes) = crate::secrets::keychain::get_master_key().await {
            self.secrets_crypto = Some(build_secrets_crypto(hex_encode_key(&keychain_key_bytes))?);
            self.settings.secrets_master_key_source = KeySource::Keychain;
            print_success("Security configured (keychain)");
            return Ok(());
        }

        // No existing key — generate one
        // Try keychain first (preferred on macOS)
        let key = crate::secrets::keychain::generate_master_key();
        if crate::secrets::keychain::store_master_key(&key)
            .await
            .is_ok()
        {
            self.secrets_crypto = Some(build_secrets_crypto(hex_encode_key(&key))?);
            self.settings.secrets_master_key_source = KeySource::Keychain;
            print_success("Master key stored in OS keychain");
            return Ok(());
        }

        // Keychain unavailable — fall back to env var mode
        let key_hex = crate::secrets::keychain::generate_master_key_hex();
        self.secrets_crypto = Some(build_secrets_crypto(key_hex.clone())?);
        crate::config::inject_single_var("SECRETS_MASTER_KEY", &key_hex);
        self.settings.secrets_master_key_hex = Some(key_hex);
        self.settings.secrets_master_key_source = KeySource::Env;
        print_success("Master key stored in ~/.ironclaw/.env");
        Ok(())
    }

    /// Initialize secrets context for channel setup.
    pub(super) async fn init_secrets_context(&mut self) -> Result<SecretsContext, SetupError> {
        // Get crypto (should be set from step 2, or load from keychain/env)
        let crypto = self.ensure_secrets_crypto().await?;

        if let Some(store) = self.secrets_store_for_backend(&crypto).await? {
            return Ok(SecretsContext::from_store(store, "default"));
        }

        Err(SetupError::Config(
            "No database backend available for secrets storage".to_string(),
        ))
    }

    /// Return the cached secrets crypto, loading the master key from the
    /// environment or the OS keychain when no crypto is cached yet.
    async fn ensure_secrets_crypto(&mut self) -> Result<Arc<SecretsCrypto>, SetupError> {
        if let Some(ref c) = self.secrets_crypto {
            return Ok(Arc::clone(c));
        }

        // Try to load master key from keychain or env
        let key = if let Ok(env_key) = std::env::var("SECRETS_MASTER_KEY") {
            env_key
        } else if let Ok(keychain_key) = crate::secrets::keychain::get_master_key().await {
            hex_encode_key(&keychain_key)
        } else {
            return Err(SetupError::Config(
                "Secrets not configured. Run full setup or set SECRETS_MASTER_KEY.".to_string(),
            ));
        };

        let crypto = build_secrets_crypto(key)?;
        self.secrets_crypto = Some(Arc::clone(&crypto));
        Ok(crypto)
    }

    /// Create a backend-appropriate secrets store.
    ///
    /// Uses runtime dispatch based on the user's selected backend, falling
    /// back to the other compiled-in backend when store creation returns
    /// `None`.
    async fn secrets_store_for_backend(
        &mut self,
        crypto: &Arc<SecretsCrypto>,
    ) -> Result<Option<Arc<dyn SecretsStore>>, SetupError> {
        let selected_backend = self
            .settings
            .database_backend
            .clone()
            .unwrap_or_else(|| default_secrets_backend().to_string());

        match selected_backend.as_str() {
            #[cfg(feature = "libsql")]
            "libsql" | "turso" | "sqlite" => {
                if let Some(store) = self.create_libsql_secrets_store(crypto)? {
                    return Ok(Some(store));
                }
                // Fallback to postgres if libsql store creation returned None
                #[cfg(feature = "postgres")]
                if let Some(store) = self.create_postgres_secrets_store(crypto).await? {
                    return Ok(Some(store));
                }
                Ok(None)
            }
            #[cfg(feature = "postgres")]
            _ => {
                if let Some(store) = self.create_postgres_secrets_store(crypto).await? {
                    return Ok(Some(store));
                }
                // Fallback to libsql if postgres store creation returned None
                #[cfg(feature = "libsql")]
                if let Some(store) = self.create_libsql_secrets_store(crypto)? {
                    return Ok(Some(store));
                }
                Ok(None)
            }
            #[cfg(not(feature = "postgres"))]
            _ => Ok(None),
        }
    }

    /// Create a PostgreSQL secrets store from the current pool.
    #[cfg(feature = "postgres")]
    async fn create_postgres_secrets_store(
        &mut self,
        crypto: &Arc<SecretsCrypto>,
    ) -> Result<Option<Arc<dyn SecretsStore>>, SetupError> {
        let pool = if let Some(ref p) = self.db_pool {
            p.clone()
        } else {
            // Fall back to creating one from settings/env
            let url = self
                .settings
                .database_url
                .clone()
                .or_else(|| std::env::var("DATABASE_URL").ok());

            if let Some(url) = url {
                self.test_database_connection_postgres(&url).await?;
                self.run_migrations_postgres().await?;
                match self.db_pool.clone() {
                    Some(pool) => pool,
                    None => {
                        return Err(SetupError::Database(
                            "Database pool not initialized after connection test".to_string(),
                        ));
                    }
                }
            } else {
                return Ok(None);
            }
        };

        let store: Arc<dyn SecretsStore> = Arc::new(crate::secrets::PostgresSecretsStore::new(
            pool,
            Arc::clone(crypto),
        ));
        Ok(Some(store))
    }

    /// Create a libSQL secrets store from the current backend.
    #[cfg(feature = "libsql")]
    fn create_libsql_secrets_store(
        &self,
        crypto: &Arc<SecretsCrypto>,
    ) -> Result<Option<Arc<dyn SecretsStore>>, SetupError> {
        if let Some(ref backend) = self.db_backend {
            let store: Arc<dyn SecretsStore> = Arc::new(crate::secrets::LibSqlSecretsStore::new(
                backend.shared_db(),
                Arc::clone(crypto),
            ));
            Ok(Some(store))
        } else {
            Ok(None)
        }
    }
}
