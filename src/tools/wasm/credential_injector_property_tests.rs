//! Property tests for shared WASM credential registry invariants.
//!
//! These tests exercise ownerless deduplication, owner-scoped replacement and
//! removal, consistency between host-existence queries and host lookups, and
//! concurrent registry access.

use std::collections::HashSet;

use proptest::prelude::*;

use crate::secrets::{CredentialLocation, CredentialMapping};
use crate::tools::wasm::SharedCredentialRegistry;

fn property_location(location_id: u8) -> CredentialLocation {
    match location_id % 3 {
        0 => CredentialLocation::AuthorizationBearer,
        1 => CredentialLocation::Header {
            name: "X-Test-Token".to_string(),
            prefix: None,
        },
        _ => CredentialLocation::AuthorizationBasic {
            username: "test-user".to_string(),
        },
    }
}

fn property_mapping(secret_id: u8, location_id: u8, host_id: u8) -> CredentialMapping {
    CredentialMapping {
        secret_name: format!("secret_{secret_id}"),
        location: property_location(location_id),
        host_patterns: vec![format!("api-{host_id}.example.test")],
    }
}

proptest! {
    #[test]
    fn prop_ownerless_registry_preserves_secret_location_keys(
        specs in prop::collection::vec((0u8..4, 0u8..3, 0u8..8), 1..40)
    ) {
        let registry = SharedCredentialRegistry::new();
        let mappings = specs
            .iter()
            .map(|(secret_id, location_id, host_id)| {
                property_mapping(*secret_id, *location_id, *host_id)
            })
            .collect::<Vec<_>>();
        let expected_keys = mappings
            .iter()
            .map(|mapping| (mapping.secret_name.clone(), mapping.location.clone()))
            .collect::<HashSet<_>>();

        registry.add_mappings(mappings);

        let mut observed_keys = HashSet::new();
        for (_, _, host_id) in specs {
            let found = registry.find_for_host(&format!("api-{host_id}.example.test"));
            let found_keys = found
                .iter()
                .map(|mapping| (mapping.secret_name.clone(), mapping.location.clone()))
                .collect::<HashSet<_>>();

            prop_assert_eq!(
                found.len(),
                found_keys.len(),
                "deduplication must leave at most one mapping per secret/location key per host"
            );
            observed_keys.extend(found_keys);
        }

        prop_assert_eq!(observed_keys, expected_keys);
    }

    #[test]
    fn prop_tool_owned_replacement_and_removal_are_owner_scoped(
        secret_id in 0u8..4,
        old_host_id in 0u8..8,
        new_host_id in 0u8..8,
        other_host_id in 0u8..8,
    ) {
        let registry = SharedCredentialRegistry::new();
        let secret_name = format!("secret_{secret_id}");
        let old_host = format!("tool-a-old-{old_host_id}.example.test");
        let new_host = format!("tool-a-new-{new_host_id}.example.test");
        let other_host = format!("tool-b-{other_host_id}.example.test");

        registry.add_mappings_for_tool(
            "tool_a",
            vec![CredentialMapping::bearer(&secret_name, &old_host)],
        );
        registry.add_mappings_for_tool(
            "tool_b",
            vec![CredentialMapping::bearer(&secret_name, &other_host)],
        );
        registry.add_mappings_for_tool(
            "tool_a",
            vec![CredentialMapping::bearer(&secret_name, &new_host)],
        );

        prop_assert!(registry.find_for_host(&old_host).is_empty());
        prop_assert_eq!(registry.find_for_host(&new_host).len(), 1);
        prop_assert_eq!(registry.find_for_host(&other_host).len(), 1);

        registry.remove_mappings_for_tool_secrets("tool_a", &[secret_name]);

        prop_assert!(registry.find_for_host(&new_host).is_empty());
        prop_assert_eq!(registry.find_for_host(&other_host).len(), 1);
    }

    #[test]
    fn prop_has_credentials_matches_find_for_host(
        specs in prop::collection::vec((0u8..4, 0u8..3, 0u8..8), 0..40),
        probe_host_id in 0u8..10,
    ) {
        let registry = SharedCredentialRegistry::new();
        registry.add_mappings(
            specs
                .into_iter()
                .map(|(secret_id, location_id, host_id)| {
                    property_mapping(secret_id, location_id, host_id)
                }),
        );

        let host = format!("api-{probe_host_id}.example.test");
        prop_assert_eq!(
            registry.has_credentials_for_host(&host),
            !registry.find_for_host(&host).is_empty()
        );
    }
}

#[test]
fn test_shared_registry_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let registry = Arc::new(SharedCredentialRegistry::new());

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let r = Arc::clone(&registry);
            thread::spawn(move || {
                r.add_mappings(vec![CredentialMapping::bearer(
                    format!("key_{i}"),
                    "api.example.com",
                )]);
            })
        })
        .collect();

    for h in handles {
        h.join().expect("registry writer thread panicked");
    }

    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 4);
}
