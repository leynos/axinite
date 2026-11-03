//! Property-based tests for channel credential injection.

use std::collections::HashMap;

use proptest::prelude::*;

use super::{in_memory_store, test_channel_with_mappings};
use crate::channels::wasm::credentials::inject_channel_credentials_from_context;
use crate::config::EnvContext;
use crate::secrets::{CreateSecretParams, NativeSecretsStore, SecretsStore};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Only channel-prefixed placeholders are injected, persistent secrets
    /// take precedence over environment values, and the reported count
    /// matches the injected placeholder set.
    #[test]
    fn prop_credential_injection_respects_prefix_and_precedence(
        channel_suffix in 0u8..3,
        secret_specs in prop::collection::vec((0u8..4, any::<bool>(), any::<bool>()), 0..6),
        mapping_specs in prop::collection::vec((0u8..4, any::<bool>()), 0..6),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let channel_name = format!("chan{channel_suffix}");
            let prefix = format!("{}_", channel_name.to_ascii_uppercase());

            let secret_name = |id: u8, prefixed: bool| {
                if prefixed {
                    format!("{channel_name}_secret{id}")
                } else {
                    format!("other_secret{id}")
                }
            };

            let mut mapping_names: Vec<String> = mapping_specs
                .iter()
                .map(|(id, prefixed)| secret_name(*id, *prefixed))
                .collect();
            mapping_names.sort();
            mapping_names.dedup();

            let channel = test_channel_with_mappings(&channel_name, &mapping_names);
            let store = in_memory_store();
            let mut expected_store = HashMap::new();
            for (index, (id, prefixed, persistent)) in secret_specs.iter().enumerate() {
                if !*persistent {
                    continue;
                }
                let name = secret_name(*id, *prefixed);
                let value = format!("store-{index}");
                NativeSecretsStore::create(
                    store.as_ref(),
                    "default",
                    CreateSecretParams::new(&name, &value),
                )
                .await
                .expect("test secret should be created");
                if *prefixed {
                    expected_store.insert(name.to_uppercase(), value);
                }
            }

            let mut env = EnvContext::default();
            let mut expected_env = HashMap::new();
            for (index, (id, prefixed)) in mapping_specs.iter().enumerate() {
                let placeholder = secret_name(*id, *prefixed).to_uppercase();
                if !placeholder.starts_with(&prefix)
                    || expected_store.contains_key(&placeholder)
                {
                    continue;
                }
                let value = format!("env-{index}");
                env = env.with_env(placeholder.as_str(), value.as_str());
                expected_env.insert(placeholder, value);
            }

            let count = inject_channel_credentials_from_context(
                &channel,
                Some(store.as_ref() as &dyn SecretsStore),
                &channel_name,
                &env,
            )
            .await
            .expect("injection should succeed");

            let credentials = channel.get_credentials().await;
            assert_eq!(count, credentials.len());
            for (placeholder, value) in &credentials {
                assert!(
                    placeholder.starts_with(&prefix),
                    "unprefixed placeholder {placeholder} was injected"
                );
                if let Some(expected) = expected_store.get(placeholder) {
                    assert_eq!(value, expected, "persistent secret must win");
                } else if let Some(expected) = expected_env.get(placeholder) {
                    assert_eq!(value, expected);
                } else {
                    panic!("unexpected placeholder {placeholder}");
                }
            }
            for placeholder in expected_store.keys().chain(expected_env.keys()) {
                assert!(
                    credentials.contains_key(placeholder),
                    "missing placeholder {placeholder}"
                );
            }
        });
    }
}
