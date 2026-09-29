//! In-memory registry of deployment-scoped feature-flag overrides (RFC 0009).
//!
//! The registry caches the operator overrides persisted in
//! `feature_flag_overrides`, keyed by deployment. It holds only the override
//! layer, not fully resolved flag values: resolution (environment variable >
//! deployment override > compiled default) happens in
//! [`super::features`] when serving `GET /api/features`.
//!
//! The registry is held in `GatewayState` behind an `Arc<RwLock<..>>` so
//! handlers can read it on the hot path and update it synchronously when an
//! operator writes an override through the settings API. Writes update both the
//! database and this registry, so the effect is visible on the next
//! `GET /api/features` without a restart.
//!
//! Hydration is lazy: on the first read for a deployment that has not yet been
//! loaded, [`super::features`] queries the store once and caches the overrides
//! here (see `ensure_deployment_hydrated`). This avoids threading an async
//! store load through the synchronous `GatewayChannel::new()` construction path.

use std::collections::HashMap;

use axum::http::HeaderMap;

/// A deployment identifier (for example `"production"` or `"default"`).
pub type DeploymentId = String;

/// Header carrying the deployment identifier for feature-flag reads and writes.
///
/// Reads (`GET /api/features`) treat it as optional and fall back to
/// [`DEFAULT_DEPLOYMENT_ID`]; writes (`PUT /api/settings/feature_flag:<name>`)
/// require it.
pub const DEPLOYMENT_ID_HEADER: &str = "x-deployment-id";

/// Deployment used when the `X-Deployment-Id` header is absent on reads.
pub const DEFAULT_DEPLOYMENT_ID: &str = "default";

/// Longest deployment identifier accepted from the `X-Deployment-Id` header.
///
/// Identifiers key both the in-memory registry and the persisted override
/// rows, so the bound keeps a caller from growing either without limit.
pub const MAX_DEPLOYMENT_ID_LEN: usize = 64;

/// The `X-Deployment-Id` header is present but not a valid identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDeploymentId;

impl std::fmt::Display for InvalidDeploymentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "X-Deployment-Id must be 1-{MAX_DEPLOYMENT_ID_LEN} characters of [a-z0-9_]"
        )
    }
}

impl std::error::Error for InvalidDeploymentId {}

/// Extract and validate the deployment identifier from request headers.
///
/// Returns `Ok(None)` when the header is absent, empty, or whitespace-only, and
/// `Ok(Some(id))` for a trimmed identifier of at most
/// [`MAX_DEPLOYMENT_ID_LEN`] lowercase ASCII letters, digits, and underscores
/// (the same alphabet as flag names). Any other value, including one that is
/// not valid UTF-8, is rejected with [`InvalidDeploymentId`].
pub fn deployment_id_from_headers(
    headers: &HeaderMap,
) -> Result<Option<DeploymentId>, InvalidDeploymentId> {
    let Some(raw) = headers.get(DEPLOYMENT_ID_HEADER) else {
        return Ok(None);
    };
    let value = raw.to_str().map_err(|_| InvalidDeploymentId)?.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_DEPLOYMENT_ID_LEN
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(InvalidDeploymentId);
    }
    Ok(Some(value.to_string()))
}

/// A mutable registry of deployment-scoped feature-flag overrides.
///
/// Maps deployment -> (flag name -> enabled). Presence of a deployment key
/// means it has been hydrated from the store, even if it has no overrides.
#[derive(Debug, Default)]
pub struct FeatureFlagRegistry {
    /// Cached override states: deployment -> (name -> enabled).
    flags: HashMap<DeploymentId, HashMap<String, bool>>,
}

impl FeatureFlagRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the override for a single flag, if one is cached.
    pub fn get(&self, deployment_id: &str, name: &str) -> Option<bool> {
        self.flags
            .get(deployment_id)
            .and_then(|deployment_flags| deployment_flags.get(name).copied())
    }

    /// Insert or replace one deployment-scoped override.
    pub fn set(&mut self, deployment_id: DeploymentId, name: String, enabled: bool) {
        self.flags
            .entry(deployment_id)
            .or_default()
            .insert(name, enabled);
    }

    /// Whether a deployment's overrides have been loaded from the store.
    ///
    /// Returns `true` once the deployment has been hydrated (including when it
    /// has no overrides), so callers can skip a repeat store query.
    pub fn is_hydrated(&self, deployment_id: &str) -> bool {
        self.flags.contains_key(deployment_id)
    }

    /// Cache a deployment's overrides loaded from the store.
    ///
    /// Marks the deployment as hydrated even when `overrides` is empty.
    /// Hydration is additive: a flag already cached (for example by a
    /// [`set`](Self::set) that landed while the store query was in flight) is
    /// newer than the snapshot and is kept.
    pub fn hydrate(&mut self, deployment_id: DeploymentId, overrides: Vec<(String, bool)>) {
        let entry = self.flags.entry(deployment_id).or_default();
        for (name, enabled) in overrides {
            entry.entry(name).or_insert(enabled);
        }
    }

    /// Return a copy of a deployment's cached overrides, if any.
    pub fn overrides_for(&self, deployment_id: &str) -> HashMap<String, bool> {
        self.flags.get(deployment_id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the deployment-scoped feature-flag registry.

    use super::*;
    use crate::test_support::ExpectValid;

    #[test]
    fn get_returns_none_for_unknown_deployment_or_flag() {
        let mut registry = FeatureFlagRegistry::new();
        assert_eq!(registry.get("production", "route_chat"), None);
        registry.set("production".to_string(), "route_chat".to_string(), true);
        assert_eq!(registry.get("production", "route_chat"), Some(true));
        assert_eq!(registry.get("production", "unknown"), None);
        assert_eq!(registry.get("staging", "route_chat"), None);
    }

    #[test]
    fn hydrate_marks_deployment_loaded_even_when_empty() {
        let mut registry = FeatureFlagRegistry::new();
        assert!(!registry.is_hydrated("default"));
        registry.hydrate("default".to_string(), vec![]);
        assert!(registry.is_hydrated("default"));
        assert!(registry.overrides_for("default").is_empty());
    }

    #[test]
    fn hydrate_populates_and_set_overwrites() {
        let mut registry = FeatureFlagRegistry::new();
        registry.hydrate(
            "production".to_string(),
            vec![("panel_logs".to_string(), false)],
        );
        assert_eq!(registry.get("production", "panel_logs"), Some(false));
        registry.set("production".to_string(), "panel_logs".to_string(), true);
        assert_eq!(registry.get("production", "panel_logs"), Some(true));

        let overrides = registry.overrides_for("production");
        assert_eq!(overrides.get("panel_logs"), Some(&true));
    }

    #[test]
    fn hydrate_keeps_overrides_set_while_the_store_query_was_in_flight() {
        let mut registry = FeatureFlagRegistry::new();
        registry.set("production".to_string(), "panel_logs".to_string(), true);
        registry.hydrate(
            "production".to_string(),
            vec![
                ("panel_logs".to_string(), false),
                ("route_chat".to_string(), false),
            ],
        );
        assert_eq!(registry.get("production", "panel_logs"), Some(true));
        assert_eq!(registry.get("production", "route_chat"), Some(false));
    }

    fn headers_with(value: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            DEPLOYMENT_ID_HEADER,
            axum::http::HeaderValue::from_bytes(value).expect_valid("header value bytes"),
        );
        headers
    }

    #[rstest::rstest]
    #[case::trimmed(b"  production  ", Ok(Some("production")))]
    #[case::digits_and_underscore(b"eu_west_2", Ok(Some("eu_west_2")))]
    #[case::whitespace_only(b"   ", Ok(None))]
    #[case::uppercase(b"Production", Err(InvalidDeploymentId))]
    #[case::hyphen(b"eu-west", Err(InvalidDeploymentId))]
    #[case::path_separator(b"a/b", Err(InvalidDeploymentId))]
    #[case::non_utf8(b"\xff", Err(InvalidDeploymentId))]
    fn deployment_id_header_is_trimmed_and_validated(
        #[case] raw: &[u8],
        #[case] expected: Result<Option<&str>, InvalidDeploymentId>,
    ) {
        let parsed = deployment_id_from_headers(&headers_with(raw));
        assert_eq!(parsed, expected.map(|id| id.map(str::to_string)));
    }

    #[test]
    fn deployment_id_header_is_optional_and_bounded() {
        assert_eq!(deployment_id_from_headers(&HeaderMap::new()), Ok(None));
        let longest = "a".repeat(MAX_DEPLOYMENT_ID_LEN);
        assert_eq!(
            deployment_id_from_headers(&headers_with(longest.as_bytes())),
            Ok(Some(longest))
        );
        let overlong = "a".repeat(MAX_DEPLOYMENT_ID_LEN + 1);
        assert_eq!(
            deployment_id_from_headers(&headers_with(overlong.as_bytes())),
            Err(InvalidDeploymentId)
        );
    }
}
