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
    configure, contains, endpoint, scope, state, status_of, test_crypto, test_crypto_with, url,
};
use super::model::{
    ENDPOINT_ENVELOPE_VERSION, EndpointKind, EndpointName, SetupField, SetupOperation,
};
use super::policy::plan_setup;
use super::service::{EndpointBindingService, EndpointState};

const SENTINEL: &str = "https://sentinel.example.com/private-path";

/// Run the full A2 contract with a factory that reopens the same database.
///
/// Each scenario keeps its own repository handle so a persistence defect
/// cannot hide behind another scenario's in-memory state.
pub(crate) async fn run_binding_contract<F>(make_repository: F)
where
    F: Fn() -> Arc<dyn EndpointBindingRepository>,
{
    let crypto = test_crypto();
    let owner = scope("alice", "calendar-ext");
    let scopes = ContractScopes {
        owner: owner.clone(),
        other_user: scope("bob", "calendar-ext"),
        other_extension: scope("alice", "other-ext"),
        thief: scope("mallory", "calendar-ext"),
    };
    let calendar = EndpointName::parse("calendar").expect("valid name");

    let repository = make_repository();
    let service = EndpointBindingService::new(repository.clone(), crypto.clone());
    let reloaded = EndpointBindingService::new(make_repository(), crypto.clone());

    scenario_persists_and_hides(&repository, &service, &reloaded, &owner).await;
    scenario_isolates_scopes(&reloaded, &scopes, &calendar).await;
    scenario_replaces_and_clears(&reloaded, &owner, &calendar).await;
    let damage = DamageInputs {
        reloaded: &reloaded,
        owner: &owner,
        calendar: &calendar,
        make_repository: &make_repository,
    };
    scenario_reports_damage(&repository, &damage).await;
    scenario_versions_are_stale(&repository, &reloaded, &owner, &calendar).await;
    scenario_transplant_fails(&repository, &reloaded, &owner, &scopes.thief).await;
    scenario_uninstall_cleanup(&service, &scopes).await;
    scenario_concurrent_settles(&make_repository, &owner, &calendar, &crypto).await;
}

/// A saved binding survives a fresh handle and never stores plaintext.
async fn scenario_persists_and_hides(
    repository: &Arc<dyn EndpointBindingRepository>,
    service: &EndpointBindingService,
    reloaded: &EndpointBindingService,
    owner: &BindingScope,
) {
    configure(service, owner, "calendar", SENTINEL).await;
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Configured,
        "a saved binding must survive a fresh repository handle"
    );

    let stored_rows = repository
        .list(owner, MAX_BINDING_BATCH)
        .await
        .expect("raw rows");
    assert_eq!(stored_rows.len(), 1);
    assert!(!contains(
        &stored_rows[0].encrypted_value,
        SENTINEL.as_bytes()
    ));
    assert!(!contains(&stored_rows[0].key_salt, SENTINEL.as_bytes()));
    assert!(!format!("{:?}", stored_rows[0]).contains("sentinel"));
}

/// Another user or extension can neither observe nor mutate the binding.
async fn scenario_isolates_scopes(
    reloaded: &EndpointBindingService,
    scopes: &ContractScopes,
    calendar: &EndpointName,
) {
    assert_eq!(
        status_of(reloaded, &scopes.other_user, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(reloaded, &scopes.other_extension, "calendar").await,
        EndpointState::Missing
    );
    assert!(
        !reloaded
            .clear(&scopes.other_user, calendar)
            .await
            .expect("scoped clear")
    );
    assert_eq!(
        status_of(reloaded, &scopes.owner, "calendar").await,
        EndpointState::Configured,
        "a foreign scope must not clear the owner's binding"
    );
}

/// Replacement updates in place, and a rejected replacement changes nothing.
async fn scenario_replaces_and_clears(
    reloaded: &EndpointBindingService,
    owner: &BindingScope,
    calendar: &EndpointName,
) {
    configure(reloaded, owner, "calendar", "https://two.example.com/").await;
    assert_eq!(
        reloaded
            .statuses(owner, &[])
            .await
            .expect("empty declarations")
            .len(),
        0
    );
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Configured
    );

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
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Configured
    );

    assert!(reloaded.clear(owner, calendar).await.expect("clear"));
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Missing
    );
    assert!(!reloaded.clear(owner, calendar).await.expect("reclear"));
}

/// Corruption, a wrong key, and a changed kind are reported, not hidden.
async fn scenario_reports_damage<F>(
    repository: &Arc<dyn EndpointBindingRepository>,
    inputs: &DamageInputs<'_, F>,
) where
    F: Fn() -> Arc<dyn EndpointBindingRepository>,
{
    let reloaded = inputs.reloaded;
    let owner = inputs.owner;
    let calendar = inputs.calendar;
    let make_repository = inputs.make_repository;
    configure(reloaded, owner, "calendar", SENTINEL).await;
    tamper(repository, owner, "calendar", |record| {
        let last = record.encrypted_value.last_mut().expect("non-empty");
        *last ^= 0x01;
    })
    .await;
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Corrupt
    );

    let wrong_key =
        EndpointBindingService::new(make_repository(), test_crypto_with(&"b".repeat(32)));
    assert_eq!(
        status_of(&wrong_key, owner, "calendar").await,
        EndpointState::Corrupt
    );

    configure(reloaded, owner, "calendar", SENTINEL).await;
    tamper(repository, owner, "calendar", |record| {
        record.endpoint_kind = EndpointKind::parse("caldav").expect("valid kind");
    })
    .await;
    let stale = status_of(reloaded, owner, "calendar").await;
    assert!(stale.needs_attention());
    assert!(!stale.is_configured());

    assert!(reloaded.clear(owner, calendar).await.expect("clear"));
    let upgraded = vec![endpoint("calendar", "caldav", false)];
    let identity = endpoint_identity(&upgraded[0]);
    reloaded
        .replace(owner, &identity, &url(SENTINEL))
        .await
        .expect("replace after kind change");
    let statuses = reloaded.statuses(owner, &upgraded).await.expect("status");
    assert_eq!(state(&statuses, "calendar"), EndpointState::Configured);
    assert!(reloaded.clear(owner, calendar).await.expect("clear"));
}

/// Unknown policy and envelope versions are stale rather than reinterpreted.
async fn scenario_versions_are_stale(
    repository: &Arc<dyn EndpointBindingRepository>,
    reloaded: &EndpointBindingService,
    owner: &BindingScope,
    calendar: &EndpointName,
) {
    configure(reloaded, owner, "calendar", SENTINEL).await;
    tamper(repository, owner, "calendar", |record| {
        record.validation_policy_version = record.validation_policy_version.saturating_add(1);
    })
    .await;
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Stale
    );

    configure(reloaded, owner, "calendar", SENTINEL).await;
    tamper(repository, owner, "calendar", |record| {
        record.envelope_version = ENDPOINT_ENVELOPE_VERSION.saturating_add(1);
    })
    .await;
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Stale
    );
    assert!(reloaded.clear(owner, calendar).await.expect("clear"));
}

/// A row transplanted into another scope fails authentication there.
async fn scenario_transplant_fails(
    repository: &Arc<dyn EndpointBindingRepository>,
    reloaded: &EndpointBindingService,
    owner: &BindingScope,
    thief: &BindingScope,
) {
    configure(reloaded, owner, "calendar", SENTINEL).await;
    let stolen = repository
        .list(owner, MAX_BINDING_BATCH)
        .await
        .expect("list")
        .into_iter()
        .find(|row| row.endpoint_name == "calendar")
        .expect("stored row");
    repository
        .upsert(&record_for(thief, &stolen))
        .await
        .expect("transplant");
    assert_eq!(
        status_of(reloaded, owner, "calendar").await,
        EndpointState::Configured
    );
    assert_eq!(
        status_of(reloaded, thief, "calendar").await,
        EndpointState::Corrupt
    );
}

/// Uninstall cleanup removes only the target scope.
async fn scenario_uninstall_cleanup(service: &EndpointBindingService, scopes: &ContractScopes) {
    configure(service, &scopes.owner, "calendar", SENTINEL).await;
    configure(service, &scopes.other_user, "calendar", SENTINEL).await;
    assert_eq!(
        service.clear_scope(&scopes.owner).await.expect("cleanup"),
        1
    );
    assert_eq!(
        status_of(service, &scopes.owner, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(service, &scopes.other_user, "calendar").await,
        EndpointState::Configured
    );
}

/// Concurrent replace and clear settle on a legal commit order.
async fn scenario_concurrent_settles<F>(
    make_repository: &F,
    owner: &BindingScope,
    calendar: &EndpointName,
    crypto: &Arc<crate::secrets::SecretsCrypto>,
) where
    F: Fn() -> Arc<dyn EndpointBindingRepository>,
{
    let concurrent = Arc::new(EndpointBindingService::new(
        make_repository(),
        crypto.clone(),
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

    let settled = status_of(&concurrent, owner, "calendar").await;
    assert!(
        matches!(settled, EndpointState::Missing | EndpointState::Configured),
        "concurrent operations must settle on a legal commit order, got {settled:?}"
    );
    concurrent.clear_scope(owner).await.expect("cleanup");
}

/// The scopes one contract run exercises.
struct ContractScopes {
    owner: BindingScope,
    other_user: BindingScope,
    other_extension: BindingScope,
    thief: BindingScope,
}

/// The state and dependencies the damage scenario needs.
struct DamageInputs<'a, F> {
    reloaded: &'a EndpointBindingService,
    owner: &'a BindingScope,
    calendar: &'a EndpointName,
    make_repository: &'a F,
}

/// Build a `BindingIdentity` from a declared endpoint field.
fn endpoint_identity(field: &SetupField) -> crate::extensions::setup::BindingIdentity {
    let SetupField::Endpoint(endpoint) = field else {
        unreachable!("fixture is an endpoint")
    };
    crate::extensions::setup::BindingIdentity {
        name: endpoint.name.clone(),
        kind: endpoint.kind.clone(),
        envelope_version: ENDPOINT_ENVELOPE_VERSION,
        policy_version: crate::extensions::setup::validation_policy_version(),
    }
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
