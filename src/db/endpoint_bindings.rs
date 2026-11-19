//! Database adapters for confidential endpoint bindings.
//!
//! The port lives in the setup feature; these adapters only persist and load
//! ciphertext records. Backend selection happens here so callers never branch
//! on feature flags.

#[cfg(feature = "libsql")]
mod libsql;
#[cfg(all(test, feature = "libsql"))]
mod libsql_tests;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(all(test, feature = "postgres"))]
mod postgres_tests;

#[cfg(feature = "libsql")]
pub use libsql::LibSqlEndpointBindingRepository;
#[cfg(feature = "postgres")]
pub use postgres::PostgresEndpointBindingRepository;

use std::sync::Arc;

use crate::extensions::setup::EndpointBindingRepository;

/// Create an endpoint-binding repository from the connected backend handles.
///
/// Returns `None` when no database handle is available. There is no compile-
/// time backend choice here: with both features enabled the handle that was
/// actually connected decides, mirroring [`crate::secrets::create_secrets_store`].
pub fn create_endpoint_binding_repository(
    handles: &crate::db::DatabaseHandles,
) -> Option<Arc<dyn EndpointBindingRepository>> {
    #[cfg(not(any(feature = "libsql", feature = "postgres")))]
    let _ = handles;

    let repository: Option<Arc<dyn EndpointBindingRepository>> = None;

    #[cfg(feature = "libsql")]
    let repository = repository.or_else(|| {
        handles.libsql_db.as_ref().map(|db| {
            Arc::new(LibSqlEndpointBindingRepository::new(Arc::clone(db)))
                as Arc<dyn EndpointBindingRepository>
        })
    });

    #[cfg(feature = "postgres")]
    let repository = repository.or_else(|| {
        handles.pg_pool.as_ref().map(|pool| {
            Arc::new(PostgresEndpointBindingRepository::new(pool.clone()))
                as Arc<dyn EndpointBindingRepository>
        })
    });

    repository
}
