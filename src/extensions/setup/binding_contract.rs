//! Backend-independent contract for confidential endpoint-binding storage.
//!
//! Both database adapters run this same sequence against a real database:
//! PostgreSQL through the embedded test cluster, and libSQL through a
//! file-backed database. Behaviour that differs between them is therefore a
//! contract failure, not an untested dialect quirk.

use std::sync::Arc;

use super::binding::{
    BindingRecord, BindingScope, EndpointBindingRepository, MAX_BINDING_BATCH, StoredBinding,
};
use super::binding_test_support::{
    configure, contains, crypto, endpoint, scope, state, status_of, test_crypto, url,
};
use super::model::{
    ENDPOINT_ENVELOPE_VERSION, EndpointKind, EndpointName, SetupField, SetupOperation,
};
use super::policy::plan_setup;
use super::service::{EndpointBindingService, EndpointState};

const SENTINEL: &str = "https://sentinel.example.com/private-path";

/// Run the full A2 contract with a factory that reopens the same database.
pub(crate) async fn run_binding_contract<F>(make_repository: F)
where
    F: Fn() -> Arc<dyn EndpointBindingRepository>,
{
    let owner_crypto = test_crypto();
    let owner = scope("alice", "calendar-ext");
    let other_user = scope("bob", "calendar-ext");
    let other_extension = scope("alice", "other-ext");
    let calendar = EndpointName::parse("calendar").expect("valid name");

    let repository = make_repository();
    let service = EndpointBindingService::new(repository.clone(), owner_crypto.clone());

    // Save, then reload through a fresh repository handle.
    configure(&service, &owner, "calendar", SENTINEL).await;
    let reloaded = EndpointBindingService::new(make_repository(), owner_crypto.clone());
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Configured,
        "a saved binding must survive a fresh repository handle"
    );

    // The stored row holds no plaintext form of the confidential URL.
    let stored_rows = repository
        .list(&owner, MAX_BINDING_BATCH)
        .await
        .expect("raw rows");
    assert_eq!(stored_rows.len(), 1);
    assert!(!contains(
        &stored_rows[0].encrypted_value,
        SENTINEL.as_bytes()
    ));
    assert!(!contains(&stored_rows[0].key_salt, SENTINEL.as_bytes()));
    assert!(!format!("{:?}", stored_rows[0]).contains("sentinel"));

    // Another user or extension can neither observe nor mutate the binding.
    assert_eq!(
        status_of(&reloaded, &other_user, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(&reloaded, &other_extension, "calendar").await,
        EndpointState::Missing
    );
    assert!(
        !reloaded
            .clear(&other_user, &calendar)
            .await
            .expect("scoped clear")
    );
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Configured,
        "a foreign scope must not clear the owner's binding"
    );

    // Replacement is atomic and unique per scoped name.
    configure(&reloaded, &owner, "calendar", "https://two.example.com/").await;
    let rows = repository
        .list(&owner, MAX_BINDING_BATCH)
        .await
        .expect("list");
    assert_eq!(rows.len(), 1, "replace must update in place");

    // A rejected replacement leaves the previous binding intact.
    let declarations = vec![endpoint("calendar", "jmap", false)];
    let rejected = plan_setup(
        &declarations,
        vec![SetupOperation::EndpointSet {
            name: calendar.clone(),
            url: "http://plain.example.com".to_string(),
        }],
    );
    assert!(rejected.is_err());
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Configured
    );

    // Clear is atomic and idempotent.
    assert!(reloaded.clear(&owner, &calendar).await.expect("clear"));
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Missing
    );
    assert!(!reloaded.clear(&owner, &calendar).await.expect("reclear"));

    // Removed and reintroduced declarations leave the stored row alone.
    configure(&reloaded, &owner, "calendar", SENTINEL).await;
    assert!(
        reloaded
            .statuses(&owner, &[])
            .await
            .expect("empty declarations")
            .is_empty()
    );
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Configured
    );

    // Corrupted ciphertext is reported as corrupt, never as absent.
    tamper(&repository, &owner, "calendar", |record| {
        let last = record.encrypted_value.last_mut().expect("non-empty");
        *last ^= 0x01;
    })
    .await;
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Corrupt
    );

    // A different master key cannot authenticate the same row.
    let wrong_key = EndpointBindingService::new(make_repository(), crypto(&"b".repeat(32)));
    assert_eq!(
        status_of(&wrong_key, &owner, "calendar").await,
        EndpointState::Corrupt
    );

    // A changed manifest kind is stale until replaced or cleared.
    configure(&reloaded, &owner, "calendar", SENTINEL).await;
    tamper(&repository, &owner, "calendar", |record| {
        record.endpoint_kind = EndpointKind::parse("caldav").expect("valid kind");
    })
    .await;
    let stale = status_of(&reloaded, &owner, "calendar").await;
    assert!(stale.needs_attention());
    assert!(!stale.is_configured());

    assert!(reloaded.clear(&owner, &calendar).await.expect("clear"));
    let upgraded = vec![endpoint("calendar", "caldav", false)];
    let (name, kind) = match &upgraded[0] {
        SetupField::Endpoint(field) => (&field.name, &field.kind),
        SetupField::Secret(_) => unreachable!("fixture is an endpoint"),
    };
    reloaded
        .replace(&owner, name, kind, &url(SENTINEL))
        .await
        .expect("replace after kind change");
    let statuses = reloaded.statuses(&owner, &upgraded).await.expect("status");
    assert_eq!(state(&statuses, "calendar"), EndpointState::Configured);

    // Unknown envelope and validation-policy versions are stale.
    tamper(&repository, &owner, "calendar", |record| {
        record.validation_policy_version = record.validation_policy_version.saturating_add(1);
    })
    .await;
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Stale
    );

    configure(&reloaded, &owner, "calendar", SENTINEL).await;
    tamper(&repository, &owner, "calendar", |record| {
        record.envelope_version = ENDPOINT_ENVELOPE_VERSION.saturating_add(1);
    })
    .await;
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Stale
    );

    // A row transplanted into another scope fails authentication there.
    configure(&reloaded, &owner, "calendar", SENTINEL).await;
    let thief = scope("mallory", "calendar-ext");
    let stolen = repository
        .list(&owner, MAX_BINDING_BATCH)
        .await
        .expect("list")
        .into_iter()
        .find(|row| row.endpoint_name == "calendar")
        .expect("stored row");
    repository
        .upsert(&record_for(&thief, &stolen))
        .await
        .expect("transplant");
    assert_eq!(
        status_of(&reloaded, &owner, "calendar").await,
        EndpointState::Configured
    );
    assert_eq!(
        status_of(&reloaded, &thief, "calendar").await,
        EndpointState::Corrupt
    );

    // An endpoint-only clear must not remove a same-named secret: secrets live
    // in a different table, so the owner's endpoint is the only affected row.
    assert!(reloaded.clear(&owner, &calendar).await.expect("clear"));

    // Uninstall cleanup removes only the target scope.
    configure(&service, &owner, "calendar", SENTINEL).await;
    configure(&service, &other_user, "calendar", SENTINEL).await;
    assert_eq!(service.clear_scope(&owner).await.expect("cleanup"), 1);
    assert_eq!(
        status_of(&service, &owner, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(&service, &other_user, "calendar").await,
        EndpointState::Configured
    );

    // Concurrent replace and clear settle on a legal commit order.
    let concurrent = Arc::new(EndpointBindingService::new(
        make_repository(),
        owner_crypto.clone(),
    ));
    let replace_target = concurrent.clone();
    let clear_target = concurrent.clone();
    let name = calendar.clone();
    let replace_scope = owner.clone();
    let clear_scope = owner.clone();
    let replace = tokio::spawn(async move {
        configure(
            &replace_target,
            &replace_scope,
            "calendar",
            "https://raced.example.com/",
        )
        .await;
    });
    let clear = tokio::spawn(async move {
        clear_target
            .clear(&clear_scope, &name)
            .await
            .expect("clear")
    });
    replace.await.expect("replace task");
    clear.await.expect("clear task");

    let settled = status_of(&concurrent, &owner, "calendar").await;
    assert!(
        matches!(settled, EndpointState::Missing | EndpointState::Configured),
        "concurrent operations must settle on a legal commit order, got {settled:?}"
    );
    concurrent.clear_scope(&owner).await.expect("cleanup");
}

fn record_for(scope: &BindingScope, stored: &StoredBinding) -> BindingRecord {
    BindingRecord {
        scope: scope.clone(),
        endpoint_name: EndpointName::parse(stored.endpoint_name.clone()).expect("stored name"),
        endpoint_kind: EndpointKind::parse(stored.endpoint_kind.clone()).expect("stored kind"),
        envelope_version: stored.envelope_version,
        validation_policy_version: stored.validation_policy_version,
        encrypted_value: stored.encrypted_value.clone(),
        key_salt: stored.key_salt.clone(),
    }
}

async fn tamper<F>(
    repository: &Arc<dyn EndpointBindingRepository>,
    scope: &BindingScope,
    name: &str,
    change: F,
) where
    F: FnOnce(&mut BindingRecord),
{
    let stored = repository
        .list(scope, MAX_BINDING_BATCH)
        .await
        .expect("list for tamper")
        .into_iter()
        .find(|row| row.endpoint_name == name)
        .expect("row to tamper with");
    let mut record = record_for(scope, &stored);
    change(&mut record);
    repository.upsert(&record).await.expect("tampered upsert");
}
