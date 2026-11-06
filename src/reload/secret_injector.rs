//! Secret injection abstraction for hot-reload.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::config::EnvContext;
use crate::db::UserId;
use crate::secrets::{SecretError, SecretsStore};

const HTTP_WEBHOOK_SECRET_KEY: &str = "HTTP_WEBHOOK_SECRET";
const SECRETS_STORE_KEY: &str = "http_webhook_secret";

/// Boxed future used at the dyn secret-injector boundary.
pub type SecretInjectorFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Trait for injecting secrets into an explicit environment snapshot.
///
/// Implementations fetch secrets from storage and update the snapshot used by
/// the reload's config loader.
pub trait SecretInjector: Send + Sync {
    /// Inject secrets into the supplied snapshot.
    ///
    /// Errors are logged internally and do not fail the reload.
    fn inject<'a>(&'a self, ctx: &'a mut EnvContext) -> SecretInjectorFuture<'a>;
}

/// Native async sibling trait for concrete secret-injector implementations.
pub trait NativeSecretInjector: Send + Sync {
    /// See [`SecretInjector::inject`].
    fn inject<'a>(&'a self, ctx: &'a mut EnvContext) -> impl Future<Output = ()> + Send + 'a;
}

impl<T> SecretInjector for T
where
    T: NativeSecretInjector + Send + Sync,
{
    fn inject<'a>(&'a self, ctx: &'a mut EnvContext) -> SecretInjectorFuture<'a> {
        Box::pin(NativeSecretInjector::inject(self, ctx))
    }
}

/// Retention state for the webhook secret across reload cycles.
///
/// A confirmed deletion must be distinguishable from "no successful read yet":
/// both leave no retained value, but only a deletion must be re-applied to
/// every later snapshot until the store returns a new secret.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WebhookSecretState {
    /// No successful read and no confirmed deletion yet.
    Unknown,
    /// Last value successfully read from the store.
    Present(String),
    /// The store definitively reported the secret as absent.
    ConfirmedDeleted,
}

/// Secret injector that reads from a database-backed secrets store.
pub struct DbSecretInjector {
    secrets_store: Arc<dyn SecretsStore + Send + Sync>,
    user_id: UserId,
    /// Retention state, so a transient retrieval error cannot downgrade the
    /// live credential to the startup snapshot, and a confirmed deletion is
    /// not undone by a later error.
    state: tokio::sync::Mutex<WebhookSecretState>,
}

impl DbSecretInjector {
    /// Create a new database-backed secret injector.
    ///
    /// On each hot-reload cycle, `secrets_store` is queried for secrets belonging to `user_id`.
    ///
    /// `secrets_store` — database-backed store for encrypted secrets.
    /// `user_id` — identifier of the user whose secrets should be loaded.
    pub fn new(secrets_store: Arc<dyn SecretsStore + Send + Sync>, user_id: UserId) -> Self {
        Self {
            secrets_store,
            user_id,
            state: tokio::sync::Mutex::new(WebhookSecretState::Unknown),
        }
    }
}

impl NativeSecretInjector for DbSecretInjector {
    async fn inject<'a>(&'a self, ctx: &'a mut EnvContext) {
        self.inject_webhook_secret(ctx).await;
    }
}

impl DbSecretInjector {
    /// Inject HTTP webhook secret from encrypted store.
    ///
    /// If the secret does not exist, logs and clears the snapshot entry — absence is not an error.
    /// A confirmed deletion is remembered and re-applied to every later
    /// snapshot, so an intervening store error cannot resurrect a secret the
    /// operator has deleted. Other errors are logged but do not fail the
    /// reload: the last successfully injected value is re-applied so a
    /// transient store or decryption failure cannot replace the live
    /// credential with the startup snapshot (which may be absent, disabling
    /// webhook authentication).
    ///
    /// Only the snapshot's secret overlay is cleared. Environment variables
    /// remain the operator's explicit override, matching `EnvContext::get`'s
    /// documented environment-over-secret precedence.
    async fn inject_webhook_secret(&self, ctx: &mut EnvContext) {
        let webhook_secret = match self
            .secrets_store
            .get_decrypted(self.user_id.as_str(), SECRETS_STORE_KEY)
            .await
        {
            Ok(s) => s,
            Err(SecretError::NotFound(_)) => {
                *self.state.lock().await = WebhookSecretState::ConfirmedDeleted;
                ctx.remove_secret(HTTP_WEBHOOK_SECRET_KEY);
                tracing::debug!(
                    "{HTTP_WEBHOOK_SECRET_KEY} not found in secrets store; cleared snapshot entry"
                );
                super::telemetry::secret_injection("missing");
                return;
            }
            Err(e) => {
                tracing::error!("Failed to inject {HTTP_WEBHOOK_SECRET_KEY}: {e}");
                match self.state.lock().await.clone() {
                    WebhookSecretState::Present(previous) => {
                        ctx.inject_secret(HTTP_WEBHOOK_SECRET_KEY, previous);
                        tracing::warn!(
                            "Retained last successfully injected {HTTP_WEBHOOK_SECRET_KEY} \
                             after retrieval error"
                        );
                        super::telemetry::secret_injection("retained_after_error");
                    }
                    WebhookSecretState::ConfirmedDeleted => {
                        ctx.remove_secret(HTTP_WEBHOOK_SECRET_KEY);
                        tracing::warn!(
                            "{HTTP_WEBHOOK_SECRET_KEY} remains deleted after a retrieval \
                             error; cleared snapshot entry"
                        );
                        super::telemetry::secret_injection("missing");
                    }
                    WebhookSecretState::Unknown => {
                        super::telemetry::secret_injection("error");
                    }
                }
                return;
            }
        };

        let value = webhook_secret.expose().to_string();
        *self.state.lock().await = WebhookSecretState::Present(value.clone());
        ctx.inject_secret(HTTP_WEBHOOK_SECRET_KEY, value);
        tracing::debug!("Injected {HTTP_WEBHOOK_SECRET_KEY} from secrets store");
        super::telemetry::secret_injection("injected");
    }
}

#[cfg(test)]
mod tests;
