//! Behavioural tests for the endpoint-binding service.
//!
//! A mutex-backed repository keeps these tests independent of either database
//! backend. The shared backend contract in `binding_contract` exercises the
//! same service against real PostgreSQL and file-backed libSQL.

use std::sync::Arc;

use super::binding::MAX_BINDING_BATCH;
use super::binding_test_support::{
    MemoryRepository, configure, contains, crypto, endpoint, endpoint_parts, scope, secret,
    status_of, test_crypto, url,
};
use super::model::{ENDPOINT_ENVELOPE_VERSION, EndpointKind, EndpointName};
use super::service::{BindingServiceError, EndpointBindingService, EndpointState};

#[tokio::test]
async fn replace_then_status_is_configured() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository, test_crypto());
    let scope = scope("user", "calendar-extension");

    configure(
        &service,
        &scope,
        "calendar",
        "https://mail.example.com/jmap/",
    )
    .await;

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Configured
    );
}

#[tokio::test]
async fn replacement_overwrites_the_previous_value() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://one.example.com/").await;
    configure(&service, &scope, "calendar", "https://two.example.com/").await;

    assert_eq!(repository.stored().await.len(), 1);
    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Configured
    );
}

#[tokio::test]
async fn clear_removes_the_binding_and_is_idempotent() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository, test_crypto());
    let scope = scope("user", "ext");
    let name = EndpointName::parse("calendar").expect("valid name");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    assert!(service.clear(&scope, &name).await.expect("first clear"));

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Missing
    );
    assert!(!service.clear(&scope, &name).await.expect("second clear"));
}

#[tokio::test]
async fn scopes_are_isolated_by_user_and_extension() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository, test_crypto());
    let owner = scope("alice", "ext");
    let other_user = scope("bob", "ext");
    let other_extension = scope("alice", "other-ext");

    configure(&service, &owner, "calendar", "https://mail.example.com/").await;

    assert_eq!(
        status_of(&service, &owner, "calendar").await,
        EndpointState::Configured
    );
    assert_eq!(
        status_of(&service, &other_user, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(&service, &other_extension, "calendar").await,
        EndpointState::Missing
    );
}

#[tokio::test]
async fn a_changed_kind_is_stale() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    repository
        .mutate_row(&scope, "calendar", |row| {
            row.endpoint_kind = EndpointKind::parse("caldav").expect("valid kind");
        })
        .await;

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Stale
    );
}

#[tokio::test]
async fn an_unknown_validation_policy_is_stale() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    repository
        .mutate_row(&scope, "calendar", |row| {
            row.validation_policy_version = row.validation_policy_version.saturating_add(1);
        })
        .await;

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Stale
    );
}

#[tokio::test]
async fn an_unknown_envelope_version_is_stale() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    repository
        .mutate_row(&scope, "calendar", |row| {
            row.envelope_version = ENDPOINT_ENVELOPE_VERSION.saturating_add(1);
        })
        .await;

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Stale
    );
}

#[tokio::test]
async fn corrupted_ciphertext_is_not_configured() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    repository
        .mutate_row(&scope, "calendar", |row| {
            let last = row.encrypted_value.last_mut().expect("non-empty envelope");
            *last ^= 0x01;
        })
        .await;

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Corrupt
    );
}

#[tokio::test]
async fn a_different_master_key_cannot_authenticate_the_record() {
    let repository = Arc::new(MemoryRepository::default());
    let writer = EndpointBindingService::new(repository.clone(), test_crypto());
    let reader = EndpointBindingService::new(repository, crypto(&"b".repeat(32)));
    let scope = scope("user", "ext");

    configure(&writer, &scope, "calendar", "https://mail.example.com/").await;

    assert_eq!(
        status_of(&reader, &scope, "calendar").await,
        EndpointState::Corrupt
    );
}

#[tokio::test]
async fn a_transplanted_record_is_corrupt() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let owner = scope("alice", "ext");
    let thief = scope("bob", "ext");

    configure(&service, &owner, "calendar", "https://mail.example.com/").await;
    let mut record = repository.stored().await.remove(0).1;
    record.scope = thief.clone();
    repository.insert_raw(thief.clone(), record).await;

    assert_eq!(
        status_of(&service, &owner, "calendar").await,
        EndpointState::Configured
    );
    assert_eq!(
        status_of(&service, &thief, "calendar").await,
        EndpointState::Corrupt
    );
}

#[tokio::test]
async fn status_only_reports_declared_endpoints_in_order() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "second", "https://two.example.com/").await;
    configure(&service, &scope, "first", "https://one.example.com/").await;

    let declarations = vec![
        secret("api_token"),
        endpoint("second", "jmap", false),
        endpoint("first", "jmap", true),
    ];
    let statuses = service
        .statuses(&scope, &declarations)
        .await
        .expect("status");

    let names: Vec<&str> = statuses.iter().map(|s| s.field().as_str()).collect();
    assert_eq!(names, ["second", "first"]);
    assert!(statuses.iter().all(|s| s.state().is_configured()));
    assert_eq!(repository.last_limit(), MAX_BINDING_BATCH);
}

#[tokio::test]
async fn removed_and_reintroduced_declarations_retain_the_row() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");

    configure(&service, &scope, "calendar", "https://mail.example.com/").await;
    assert!(
        service
            .statuses(&scope, &[])
            .await
            .expect("empty")
            .is_empty()
    );

    assert_eq!(
        status_of(&service, &scope, "calendar").await,
        EndpointState::Configured
    );
    assert_eq!(repository.stored().await.len(), 1);
}

#[tokio::test]
async fn uninstall_cleanup_removes_only_the_target_scope() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository, test_crypto());
    let removed = scope("alice", "ext");
    let retained = scope("bob", "ext");

    configure(&service, &removed, "calendar", "https://mail.example.com/").await;
    configure(&service, &retained, "calendar", "https://mail.example.com/").await;

    assert_eq!(service.clear_scope(&removed).await.expect("cleanup"), 1);
    assert_eq!(
        status_of(&service, &removed, "calendar").await,
        EndpointState::Missing
    );
    assert_eq!(
        status_of(&service, &retained, "calendar").await,
        EndpointState::Configured
    );
}

#[tokio::test]
async fn a_repository_failure_is_not_reported_as_absence() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");
    let field = endpoint("calendar", "jmap", false);
    let (name, kind) = endpoint_parts(&field);
    let identity = crate::extensions::setup::BindingIdentity {
        name: name.clone(),
        kind: kind.clone(),
        envelope_version: crate::extensions::setup::ENDPOINT_ENVELOPE_VERSION,
        policy_version: crate::extensions::setup::validation_policy_version(),
    };

    repository.fail_operations();

    let replace = service
        .replace(&scope, &identity, &url("https://mail.example.com/"))
        .await;
    assert!(matches!(
        replace,
        Err(BindingServiceError::StoreUnavailable { .. })
    ));

    let statuses = service.statuses(&scope, &[field]).await;
    assert!(matches!(
        statuses,
        Err(BindingServiceError::StoreUnavailable { .. })
    ));
}

#[tokio::test]
async fn persisted_rows_never_contain_the_endpoint_text() {
    let repository = Arc::new(MemoryRepository::default());
    let service = EndpointBindingService::new(repository.clone(), test_crypto());
    let scope = scope("user", "ext");
    let sentinel = "https://sentinel.example.com/private-path";

    configure(&service, &scope, "calendar", sentinel).await;

    let rows = repository.stored().await;
    let (_, record) = rows.first().expect("one row");
    assert!(!contains(
        record.encrypted_value.as_slice(),
        sentinel.as_bytes()
    ));
    assert!(!contains(record.key_salt.as_slice(), sentinel.as_bytes()));
    let rendered = format!("{record:?}");
    assert!(!rendered.contains("sentinel"));
    assert!(rendered.contains("[REDACTED]"));
}

#[test]
fn state_helpers_distinguish_readiness() {
    assert!(EndpointState::Configured.is_configured());
    assert!(!EndpointState::Configured.needs_attention());
    assert!(EndpointState::Stale.needs_attention());
    assert!(EndpointState::Corrupt.needs_attention());
    assert!(!EndpointState::Missing.is_configured());
    assert!(!EndpointState::Missing.needs_attention());
}
