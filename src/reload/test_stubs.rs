//! Hand-rolled test stubs for hot-reload components.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::channels::ChannelSecretUpdater;
use crate::config::Config;
use crate::error::{ChannelError, ConfigError};
use crate::reload::config_loader::NativeConfigLoader;
use crate::reload::listener_controller::NativeListenerController;
use crate::reload::secret_injector::NativeSecretInjector;
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

impl NativeConfigLoader for StubConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        match &self.config {
            Some(c) => Ok(c.clone()),
            None => {
                Err(reconstruct_config_error(self.error.as_ref().expect(
                    "StubConfigLoader: either config or error must be set",
                )))
            }
        }
    }
}

/// Reconstruct a [`ConfigError`] from an `Arc` reference.
///
/// `ConfigError` does not implement `Clone` (the `Io` variant wraps
/// `std::io::Error`), so we match each variant and rebuild it from its
/// string fields. The `Io` variant is approximated as a `ParseError` in
/// test stubs because `std::io::Error` is not cloneable.
fn reconstruct_config_error(err: &ConfigError) -> ConfigError {
    match err {
        ConfigError::MissingEnvVar(s) => ConfigError::MissingEnvVar(s.clone()),
        ConfigError::MissingRequired { key, hint } => ConfigError::MissingRequired {
            key: key.clone(),
            hint: hint.clone(),
        },
        ConfigError::InvalidValue { key, message } => ConfigError::InvalidValue {
            key: key.clone(),
            message: message.clone(),
        },
        ConfigError::ParseError(s) => ConfigError::ParseError(s.clone()),
        ConfigError::Io(e) => ConfigError::ParseError(format!("IO (reconstructed): {e}")),
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

impl NativeListenerController for StubListenerController {
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

impl NativeSecretInjector for StubSecretInjector {
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

/// Spy implementation of [`ChannelSecretUpdater`] that records every call.
pub struct SpySecretUpdater {
    calls: Arc<Mutex<Vec<Option<secrecy::SecretString>>>>,
}

impl Default for SpySecretUpdater {
    fn default() -> Self {
        Self::new()
    }
}

impl SpySecretUpdater {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns the number of times `update_secret` was called.
    pub async fn call_count(&self) -> usize {
        self.calls.lock().await.len()
    }

    /// Returns true if `update_secret` was never called.
    pub async fn was_not_called(&self) -> bool {
        self.calls.lock().await.is_empty()
    }
}

impl ChannelSecretUpdater for SpySecretUpdater {
    fn update_secret<'a>(
        &'a self,
        new_secret: Option<secrecy::SecretString>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().await.push(new_secret);
        })
    }
}
