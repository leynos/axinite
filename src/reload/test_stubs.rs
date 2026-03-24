//! Hand-rolled test stubs for hot-reload components.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::error::{ChannelError, ConfigError};
use crate::reload::{ConfigLoader, ListenerController, SecretInjector};
use crate::secrets::SecretError;

/// Stub config loader that returns a pre-configured result.
pub struct StubConfigLoader {
    config: Option<Config>,
    error: Option<Arc<ConfigError>>,
}

impl StubConfigLoader {
    pub fn new_success(config: Config) -> Self {
        Self {
            config: Some(config),
            error: None,
        }
    }

    pub fn new_error(error: ConfigError) -> Self {
        Self {
            config: None,
            error: Some(Arc::new(error)),
        }
    }
}

#[async_trait]
impl ConfigLoader for StubConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        match &self.config {
            Some(c) => Ok(c.clone()),
            None => {
                // Clone the Arc and extract error via Display -> String -> MissingEnvVar
                // This preserves the error message while keeping StubConfigLoader cloneable
                let err_msg = self
                    .error
                    .as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "Test error".to_string());
                Err(ConfigError::MissingEnvVar(err_msg))
            }
        }
    }
}

/// Stub listener controller that records restart calls.
pub struct StubListenerController {
    current_addr: SocketAddr,
    restart_calls: Arc<Mutex<Vec<SocketAddr>>>,
    restart_should_fail: bool,
}

impl StubListenerController {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            current_addr: addr,
            restart_calls: Arc::new(Mutex::new(Vec::new())),
            restart_should_fail: false,
        }
    }

    pub fn new_with_restart_failure(addr: SocketAddr) -> Self {
        Self {
            current_addr: addr,
            restart_calls: Arc::new(Mutex::new(Vec::new())),
            restart_should_fail: true,
        }
    }

    /// Returns a copy of all restart calls made to this controller.
    pub async fn restart_calls(&self) -> Vec<SocketAddr> {
        self.restart_calls.lock().await.clone()
    }
}

#[async_trait]
impl ListenerController for StubListenerController {
    async fn current_addr(&self) -> SocketAddr {
        self.current_addr
    }

    async fn restart_with_addr(&self, addr: SocketAddr) -> Result<(), ChannelError> {
        self.restart_calls.lock().await.push(addr);

        if self.restart_should_fail {
            Err(ChannelError::StartupFailed {
                name: "stub_listener".to_string(),
                reason: "Simulated restart failure".to_string(),
            })
        } else {
            Ok(())
        }
    }
}

/// Stub secret injector that records whether inject was called.
pub struct StubSecretInjector {
    called: Arc<Mutex<bool>>,
    should_fail: bool,
}

impl StubSecretInjector {
    pub fn new(should_fail: bool) -> Self {
        Self {
            called: Arc::new(Mutex::new(false)),
            should_fail,
        }
    }

    /// Returns true if inject() was called.
    pub async fn was_called(&self) -> bool {
        *self.called.lock().await
    }
}

#[async_trait]
impl SecretInjector for StubSecretInjector {
    async fn inject(&self) -> Result<(), SecretError> {
        *self.called.lock().await = true;

        if self.should_fail {
            Err(SecretError::Database(
                "Simulated inject failure".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}
