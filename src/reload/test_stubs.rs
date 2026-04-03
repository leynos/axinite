//! Hand-rolled test stubs for hot-reload components.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};

use secrecy::ExposeSecret;
use tokio::sync::Mutex;

use crate::channels::ChannelSecretUpdater;
use crate::config::Config;
use crate::error::{ChannelError, ConfigError};
use crate::reload::config_loader::NativeConfigLoader;
use crate::reload::listener_controller::NativeListenerController;
use crate::reload::secret_injector::NativeSecretInjector;

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
            None => Err(self
                .error
                .as_ref()
                .expect("StubConfigLoader: either config or error must be set")
                .as_ref()
                .clone()),
        }
    }
}

/// Stub listener controller that records restart calls.
pub struct StubListenerController {
    current_addr: Arc<Mutex<SocketAddr>>,
    restart_calls: Arc<Mutex<Vec<SocketAddr>>>,
    shutdown_calls: Arc<AtomicUsize>,
    is_running: Arc<AtomicBool>,
    restart_should_fail: bool,
}

impl StubListenerController {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            current_addr: Arc::new(Mutex::new(addr)),
            restart_calls: Arc::new(Mutex::new(Vec::new())),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            is_running: Arc::new(AtomicBool::new(true)),
            restart_should_fail: false,
        }
    }

    pub fn new_with_restart_failure(addr: SocketAddr) -> Self {
        Self {
            current_addr: Arc::new(Mutex::new(addr)),
            restart_calls: Arc::new(Mutex::new(Vec::new())),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            is_running: Arc::new(AtomicBool::new(true)),
            restart_should_fail: true,
        }
    }

    /// Returns a copy of all restart calls made to this controller.
    pub async fn restart_calls(&self) -> Vec<SocketAddr> {
        self.restart_calls.lock().await.clone()
    }

    /// Returns the number of shutdown calls made to this controller.
    pub fn shutdown_count(&self) -> usize {
        self.shutdown_calls.load(Ordering::SeqCst)
    }
}

impl NativeListenerController for StubListenerController {
    async fn current_addr(&self) -> SocketAddr {
        *self.current_addr.lock().await
    }

    async fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    async fn restart_with_addr(&self, addr: SocketAddr) -> Result<(), ChannelError> {
        self.restart_calls.lock().await.push(addr);

        if self.restart_should_fail {
            Err(ChannelError::StartupFailed {
                name: "stub_listener".to_string(),
                reason: "Simulated restart failure".to_string(),
            })
        } else {
            *self.current_addr.lock().await = addr;
            self.is_running.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    async fn shutdown(&self) {
        self.shutdown_calls.fetch_add(1, Ordering::SeqCst);
        self.is_running.store(false, Ordering::SeqCst);
    }
}

/// Stub secret injector that records whether inject was called.
pub struct StubSecretInjector {
    called: Arc<Mutex<bool>>,
}

impl StubSecretInjector {
    pub fn new() -> Self {
        Self {
            called: Arc::new(Mutex::new(false)),
        }
    }

    /// Returns true if inject() was called.
    pub async fn was_called(&self) -> bool {
        *self.called.lock().await
    }
}

impl Default for StubSecretInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSecretInjector for StubSecretInjector {
    async fn inject(&self) {
        *self.called.lock().await = true;
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

    /// Returns the recorded secrets as plain strings for assertion purposes.
    pub async fn recorded_secrets(&self) -> Vec<Option<String>> {
        self.calls
            .lock()
            .await
            .iter()
            .map(|secret| {
                secret
                    .as_ref()
                    .map(|value| value.expose_secret().to_string())
            })
            .collect()
    }

    /// Returns the last secret payload recorded by the spy.
    pub async fn last_secret(&self) -> Option<Option<String>> {
        self.recorded_secrets().await.into_iter().last()
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
