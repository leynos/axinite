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
    result: Result<Config, Arc<ConfigError>>,
}

impl StubConfigLoader {
    /// Construct a stub loader that always returns `config`.
    ///
    /// `config` is cloned for each `load()` call so tests can reuse the same
    /// stub across reload attempts. Returns a `StubConfigLoader` configured for
    /// successful loads.
    pub fn new_success(config: Config) -> Self {
        Self { result: Ok(config) }
    }

    /// Construct a stub loader that always returns `error`.
    ///
    /// `error` is wrapped once and cloned on each `load()` call so tests can
    /// exercise failure paths without panics. Returns a `StubConfigLoader`
    /// configured for failing loads.
    pub fn new_error(error: ConfigError) -> Self {
        Self {
            result: Err(Arc::new(error)),
        }
    }
}

impl NativeConfigLoader for StubConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        self.result.clone().map_err(|error| error.as_ref().clone())
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
    /// Construct a stub listener controller with a running listener at `addr`.
    ///
    /// `addr` seeds the current listener address returned by `current_addr()`.
    /// Returns a controller that records restarts and allows shutdown checks.
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            current_addr: Arc::new(Mutex::new(addr)),
            restart_calls: Arc::new(Mutex::new(Vec::new())),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            is_running: Arc::new(AtomicBool::new(true)),
            restart_should_fail: false,
        }
    }

    /// Construct a stub listener controller whose restarts always fail.
    ///
    /// `addr` seeds the initial listener address. Returns a controller that
    /// records restart attempts but responds with a simulated startup error.
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
    should_fail: bool,
}

impl StubSecretInjector {
    /// Construct a stub injector that records calls and otherwise succeeds.
    ///
    /// Returns a `StubSecretInjector` suitable for success-path reload tests.
    pub fn new() -> Self {
        Self {
            called: Arc::new(Mutex::new(false)),
            should_fail: false,
        }
    }

    /// Construct a stub injector that records calls and simulates failure.
    ///
    /// Returns a `StubSecretInjector` that logs a failure when invoked so tests
    /// can verify hot reload continues despite injector errors.
    pub fn new_failure() -> Self {
        Self {
            called: Arc::new(Mutex::new(false)),
            should_fail: true,
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
        if self.should_fail {
            tracing::error!("Simulated secret injector failure");
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
    /// Construct a spy updater that records every secret payload it receives.
    ///
    /// Returns a `SpySecretUpdater` with an empty call log for reload tests.
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
    #[allow(dead_code)]
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
        let calls = self.calls.lock().await;
        calls.last().map(|secret| {
            secret
                .as_ref()
                .map(|value| value.expose_secret().to_string())
        })
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
