//! Property tests for webhook-secret retention across reload cycles.
//!
//! Each generated sequence of store reads is compared with a small reference
//! model. The model states the intended contract: the latest successful read
//! is retained until a missing result clears it, a confirmed deletion stays
//! deleted through later errors, an error with no retained state preserves the
//! input snapshot, and environment values keep precedence over injected
//! secrets.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use proptest::prelude::*;

use super::{
    DbSecretInjector, HTTP_WEBHOOK_SECRET_KEY, MockSecretsStore, MockSyncSecretsStore,
    NativeSecretInjector, SECRETS_STORE_KEY,
};
use crate::config::EnvContext;
use crate::db::UserId;
use crate::secrets::{DecryptedSecret, SecretError, SecretsStore};

/// One scripted result from the secrets store.
#[derive(Debug, Clone)]
enum Read {
    Success(String),
    NotFound,
    Error,
}

fn read_strategy() -> impl Strategy<Value = Read> {
    prop_oneof![
        "[a-z]{1,16}".prop_map(Read::Success),
        Just(Read::NotFound),
        Just(Read::Error),
    ]
}

/// Reference model for the injector's retention state.
#[derive(Debug, Clone, Default)]
struct Model {
    retained: Option<String>,
    deleted: bool,
}

impl Model {
    /// Expected snapshot value after `read`, plus the next model state.
    fn step(&self, read: &Read, startup: Option<&str>) -> (Option<String>, Self) {
        match read {
            Read::Success(value) => (
                Some(value.clone()),
                Self {
                    retained: Some(value.clone()),
                    deleted: false,
                },
            ),
            Read::NotFound => (
                None,
                Self {
                    retained: None,
                    deleted: true,
                },
            ),
            Read::Error => {
                let snapshot = if let Some(value) = &self.retained {
                    Some(value.clone())
                } else if self.deleted {
                    None
                } else {
                    startup.map(str::to_string)
                };
                (snapshot, self.clone())
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every reload cycle matches the reference model, including the
    /// environment-over-secret precedence applied by `EnvContext::get`.
    #[test]
    fn prop_secret_retention_matches_reference_model(
        reads in prop::collection::vec(read_strategy(), 1..10),
        startup in prop::option::of("[a-z]{1,16}"),
        env_value in prop::option::of("[a-z]{1,16}"),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let script: Arc<Mutex<VecDeque<Read>>> =
                Arc::new(Mutex::new(reads.iter().cloned().collect()));
            let mut mock = MockSyncSecretsStore::new();
            mock.expect_get_decrypted()
                .times(reads.len())
                .returning(move |_, _| match script
                    .lock()
                    .expect("script lock")
                    .pop_front()
                    .expect("one scripted read per cycle")
                {
                    Read::Success(value) => Ok(
                        DecryptedSecret::from_bytes(value.into_bytes())
                            .expect("test secret should decode"),
                    ),
                    Read::NotFound => {
                        Err(SecretError::NotFound(SECRETS_STORE_KEY.to_string()))
                    }
                    Read::Error => {
                        Err(SecretError::Database("transient failure".to_string()))
                    }
                });

            let store: Arc<dyn SecretsStore + Send + Sync> =
                Arc::new(MockSecretsStore::new(mock));
            let injector = DbSecretInjector::new(store, UserId::from("test_user"));
            let mut model = Model::default();

            for read in &reads {
                let (expected_snapshot, next) = model.step(read, startup.as_deref());
                model = next;

                let mut ctx = match &startup {
                    Some(value) => {
                        EnvContext::default().with_secret(HTTP_WEBHOOK_SECRET_KEY, value.clone())
                    }
                    None => EnvContext::default(),
                };
                if let Some(value) = &env_value {
                    ctx = ctx.with_env(HTTP_WEBHOOK_SECRET_KEY, value.clone());
                }

                NativeSecretInjector::inject(&injector, &mut ctx).await;

                let expected = env_value.clone().or(expected_snapshot);
                prop_assert_eq!(
                    ctx.get(HTTP_WEBHOOK_SECRET_KEY).map(str::to_string),
                    expected,
                    "cycle result diverged from the reference model for {:?}",
                    read
                );
            }

            Ok::<(), TestCaseError>(())
        })?;
    }
}
