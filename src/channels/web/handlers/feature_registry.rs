//! HTTP deployment parsing and the existing settings-store feature-flag adapter.

use crate::db::SettingsStore;
use crate::error::DatabaseError;
pub use crate::feature_flags::registry::FeatureFlagRegistry;
use crate::feature_flags::{DeploymentId, FlagName, InvalidDeploymentId, service::FlagStore};
use axum::http::HeaderMap;

/// Header identifying the deployment; required on writes and optional on reads.
pub const DEPLOYMENT_ID_HEADER: &str = "x-deployment-id";
/// Deployment selected for reads without a header.
pub const DEFAULT_DEPLOYMENT_ID: &str = "default";

/// Normalize the optional HTTP header, then validate through the domain type.
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
    DeploymentId::parse(value).map(Some)
}

/// Translate the existing settings-store surface into the narrow domain port.
pub(super) struct SettingsFlagStore<'a> {
    pub store: &'a dyn SettingsStore,
    pub registry: &'a tokio::sync::RwLock<FeatureFlagRegistry>,
}

impl FlagStore for SettingsFlagStore<'_> {
    type Error = DatabaseError;

    #[tracing::instrument(name = "feature_flags.hydrate", skip_all)]
    async fn load(&self, deployment: &DeploymentId) -> Result<Vec<(String, bool)>, DatabaseError> {
        telemetry::count("cache", "miss");
        if self.registry.read().await.is_retry(deployment.as_str()) {
            telemetry::count("hydration", "retry");
        }
        let started = std::time::Instant::now();
        let result = self.store.list_deployment_flags(deployment.as_str()).await;
        telemetry::store_result("load", started, result.is_ok());
        telemetry::count(
            "hydration",
            if result.is_ok() { "success" } else { "error" },
        );
        result
    }

    #[tracing::instrument(name = "feature_flags.write", skip_all)]
    async fn save(
        &self,
        deployment: &DeploymentId,
        flag: &FlagName,
        enabled: bool,
    ) -> Result<(), DatabaseError> {
        let started = std::time::Instant::now();
        let result = self
            .store
            .set_deployment_flag(deployment.as_str(), flag.as_str(), enabled)
            .await;
        telemetry::store_result("save", started, result.is_ok());
        result
    }
}

pub(super) mod telemetry;

#[cfg(test)]
mod tests {
    //! Unit tests for the deployment-scoped feature-flag registry.

    use super::*;
    use crate::feature_flags::MAX_DEPLOYMENT_ID_LEN;
    use crate::test_support::ExpectValid;

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
    #[case::alphabet_boundaries(b"az09_", Ok(Some("az09_")))]
    #[case::at_sign_before_uppercase(b"@", Err(InvalidDeploymentId))]
    #[case::bracket_after_uppercase(b"[", Err(InvalidDeploymentId))]
    #[case::backtick_before_lowercase(b"`", Err(InvalidDeploymentId))]
    #[case::brace_after_lowercase(b"{", Err(InvalidDeploymentId))]
    #[case::colon_after_digits(b":", Err(InvalidDeploymentId))]
    #[case::embedded_whitespace(b"eu west", Err(InvalidDeploymentId))]
    fn deployment_id_header_is_trimmed_and_validated(
        #[case] raw: &[u8],
        #[case] expected: Result<Option<&str>, InvalidDeploymentId>,
    ) {
        let parsed = deployment_id_from_headers(&headers_with(raw));
        assert_eq!(
            parsed,
            expected.map(|id| id
                .map(|value| DeploymentId::parse(value).expect_valid("valid expected deployment")))
        );
    }

    #[test]
    fn deployment_id_header_is_optional_and_bounded() {
        assert_eq!(deployment_id_from_headers(&HeaderMap::new()), Ok(None));
        let longest = "a".repeat(MAX_DEPLOYMENT_ID_LEN);
        assert_eq!(
            deployment_id_from_headers(&headers_with(longest.as_bytes())),
            Ok(Some(
                DeploymentId::parse(&longest).expect_valid("bounded deployment")
            ))
        );
        let overlong = "a".repeat(MAX_DEPLOYMENT_ID_LEN + 1);
        assert_eq!(
            deployment_id_from_headers(&headers_with(overlong.as_bytes())),
            Err(InvalidDeploymentId)
        );
    }
}
