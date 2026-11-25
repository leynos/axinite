//! Shared test doubles and builders for endpoint-binding tests.
//!
//! Kept out of the behavioural test file so each module stays within the
//! repository's module-length budget. The same builders are used by the
//! backend contract in `binding_contract`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use secrecy::SecretString;
use tokio::sync::Mutex;

use super::binding::{
    BindingRecord, BindingScope, BindingStoreError, NativeEndpointBindingRepository, StoredBinding,
};
use super::endpoint::validate_endpoint_url;
use super::model::{
    CredentialName, EndpointField, EndpointKind, EndpointName, EndpointUrl, SecretField, SetupField,
};
use super::service::{EndpointBindingService, EndpointState, EndpointStatus};
use crate::secrets::SecretsCrypto;
use crate::testing::credentials::TEST_CRYPTO_KEY;

/// Mutex-backed repository with fault injection and row mutation hooks.
#[derive(Default)]
pub(crate) struct MemoryRepository {
    rows: Mutex<Vec<(BindingScope, BindingRecord)>>,
    fail: AtomicBool,
    fail_writes: AtomicBool,
    last_limit: AtomicUsize,
}

impl MemoryRepository {
    /// Snapshot every stored row.
    pub(crate) async fn stored(&self) -> Vec<(BindingScope, BindingRecord)> {
        self.rows.lock().await.clone()
    }

    /// Apply a mutation to one stored row, simulating tampering.
    pub(crate) async fn mutate_row<F>(&self, scope: &BindingScope, name: &str, mutate: F)
    where
        F: FnOnce(&mut BindingRecord),
    {
        let mut rows = self.rows.lock().await;
        let row = rows
            .iter_mut()
            .find(|(stored_scope, record)| {
                stored_scope == scope && record.endpoint_name.as_str() == name
            })
            .expect("row exists");
        mutate(&mut row.1);
    }

    /// Insert a row without passing through the service.
    pub(crate) async fn insert_raw(&self, scope: BindingScope, record: BindingRecord) {
        self.rows.lock().await.push((scope, record));
    }

    /// Make every operation fail with [`BindingStoreError::Unavailable`].
    pub(crate) fn fail_operations(&self) {
        self.fail.store(true, Ordering::SeqCst);
    }

    /// Make only mutations fail, leaving reads working.
    ///
    /// This models a write that was refused or whose acknowledgement was lost
    /// while status reads still succeed.
    pub(crate) fn fail_writes(&self) {
        self.fail_writes.store(true, Ordering::SeqCst);
    }

    /// The limit passed to the most recent batch read.
    pub(crate) fn last_limit(&self) -> usize {
        self.last_limit.load(Ordering::SeqCst)
    }
}

impl NativeEndpointBindingRepository for MemoryRepository {
    async fn upsert<'a>(&'a self, record: &'a BindingRecord) -> Result<(), BindingStoreError> {
        if self.fail.load(Ordering::SeqCst) || self.fail_writes.load(Ordering::SeqCst) {
            return Err(BindingStoreError::Unavailable);
        }
        let mut rows = self.rows.lock().await;
        let existing = rows.iter_mut().find(|(scope, stored)| {
            *scope == record.scope && stored.endpoint_name == record.endpoint_name
        });
        match existing {
            Some((_, stored)) => *stored = record.clone(),
            None => rows.push((record.scope.clone(), record.clone())),
        }
        Ok(())
    }

    async fn delete<'a>(
        &'a self,
        scope: &'a BindingScope,
        endpoint_name: &'a str,
    ) -> Result<bool, BindingStoreError> {
        if self.fail.load(Ordering::SeqCst) || self.fail_writes.load(Ordering::SeqCst) {
            return Err(BindingStoreError::Unavailable);
        }
        let mut rows = self.rows.lock().await;
        let before = rows.len();
        rows.retain(|(stored_scope, record)| {
            !(stored_scope == scope && record.endpoint_name.as_str() == endpoint_name)
        });
        Ok(rows.len() != before)
    }

    async fn list<'a>(
        &'a self,
        scope: &'a BindingScope,
        limit: usize,
    ) -> Result<Vec<StoredBinding>, BindingStoreError> {
        self.last_limit.store(limit, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(BindingStoreError::Unavailable);
        }
        let rows = self.rows.lock().await;
        let mut stored: Vec<StoredBinding> = rows
            .iter()
            .filter(|(stored_scope, _)| stored_scope == scope)
            .take(limit)
            .map(|(_, record)| StoredBinding {
                endpoint_name: record.endpoint_name.as_str().to_string(),
                endpoint_kind: record.endpoint_kind.as_str().to_string(),
                envelope_version: record.envelope_version,
                validation_policy_version: record.validation_policy_version,
                encrypted_value: record.encrypted_value.clone(),
                key_salt: record.key_salt.clone(),
            })
            .collect();
        stored.sort_by(|left, right| left.endpoint_name.cmp(&right.endpoint_name));
        Ok(stored)
    }

    async fn delete_scope<'a>(&'a self, scope: &'a BindingScope) -> Result<u64, BindingStoreError> {
        if self.fail.load(Ordering::SeqCst) || self.fail_writes.load(Ordering::SeqCst) {
            return Err(BindingStoreError::Unavailable);
        }
        let mut rows = self.rows.lock().await;
        let before = rows.len();
        rows.retain(|(stored_scope, _)| stored_scope != scope);
        Ok((before - rows.len()) as u64)
    }
}

/// Build a crypto holder from an explicit master key.
pub(crate) fn crypto(key: &str) -> Arc<SecretsCrypto> {
    Arc::new(SecretsCrypto::new(SecretString::from(key.to_string())).expect("test key"))
}

/// Build a crypto holder from the repository's standard test key.
pub(crate) fn test_crypto() -> Arc<SecretsCrypto> {
    crypto(TEST_CRYPTO_KEY)
}

/// Build a host scope.
pub(crate) fn scope(user: &str, extension: &str) -> BindingScope {
    BindingScope::new(user, extension)
}

/// Build an endpoint declaration.
pub(crate) fn endpoint(name: &str, kind: &str, optional: bool) -> SetupField {
    SetupField::Endpoint(EndpointField {
        name: EndpointName::parse(name).expect("valid endpoint name"),
        kind: EndpointKind::parse(kind).expect("valid endpoint kind"),
        prompt: "Endpoint".to_string(),
        optional,
    })
}

/// Build a secret declaration, to prove status ignores it.
pub(crate) fn secret(name: &str) -> SetupField {
    SetupField::Secret(SecretField {
        name: CredentialName::parse(name).expect("valid credential name"),
        prompt: "Secret".to_string(),
        optional: false,
        auto_generate: false,
    })
}

/// Borrow the name and kind from an endpoint declaration.
pub(crate) fn endpoint_parts(field: &SetupField) -> (&EndpointName, &EndpointKind) {
    match field {
        SetupField::Endpoint(endpoint) => (&endpoint.name, &endpoint.kind),
        SetupField::Secret(_) => panic!("expected an endpoint declaration"),
    }
}

/// Validate a fixture URL.
pub(crate) fn url(raw: &str) -> EndpointUrl {
    validate_endpoint_url(raw).expect("valid endpoint URL")
}

/// Replace one endpoint binding with a valid fixture URL.
pub(crate) async fn configure(
    service: &EndpointBindingService,
    scope: &BindingScope,
    name: &str,
    raw: &str,
) {
    let field = endpoint(name, "jmap", false);
    let (endpoint_name, kind) = endpoint_parts(&field);
    let identity = crate::extensions::setup::BindingIdentity {
        name: endpoint_name.clone(),
        kind: kind.clone(),
        envelope_version: crate::extensions::setup::ENDPOINT_ENVELOPE_VERSION,
        policy_version: crate::extensions::setup::validation_policy_version(),
    };
    service
        .replace(scope, &identity, &url(raw))
        .await
        .expect("replace succeeds");
}

/// Find one endpoint's state in a status list.
pub(crate) fn state(statuses: &[EndpointStatus], name: &str) -> EndpointState {
    statuses
        .iter()
        .find(|status| status.field().as_str() == name)
        .expect("declared endpoint has a status")
        .state()
}

/// Read one endpoint's state through a single-declaration status call.
pub(crate) async fn status_of(
    service: &EndpointBindingService,
    scope: &BindingScope,
    name: &str,
) -> EndpointState {
    let declarations = vec![endpoint(name, "jmap", false)];
    let statuses = service
        .statuses(scope, &declarations)
        .await
        .expect("status");
    state(&statuses, name)
}

/// Byte-substring search used to prove plaintext never reaches storage.
pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
