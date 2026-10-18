//! HTTP deployment parsing and the settings-store adapter for gateway flag services.

use crate::db::SettingsStore;
use crate::feature_flags::{
    DeploymentId, FlagName, FlagOverrideCommand, FlagSnapshot, InvalidDeploymentId,
};
use application::{FlagStore, FlagStoreError};
use axum::http::HeaderMap;

mod registry;
pub use registry::FeatureFlagRegistry;
pub(super) mod application;
pub(super) mod telemetry;

/// Header identifying the deployment; required on writes and optional on reads.
pub const DEPLOYMENT_ID_HEADER: &str = "x-deployment-id";
/// Deployment selected for reads without a header.
pub const DEFAULT_DEPLOYMENT_ID: &str = "default";

/// Normalize the optional HTTP header, then validate through the pure domain type.
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

/// Hide settings-store rows and database failures behind the typed repository port.
pub(super) struct SettingsFlagStore<'a> {
    pub store: &'a dyn SettingsStore,
}

impl FlagStore for SettingsFlagStore<'_> {
    // Read observations belong to the HTTP or command caller, never this query adapter.
    async fn load(&self, deployment: &DeploymentId) -> Result<FlagSnapshot, FlagStoreError> {
        self.load_snapshot(deployment).await
    }

    #[tracing::instrument(name = "feature_flags.write", skip_all)]
    async fn save(&self, command: &FlagOverrideCommand) -> Result<(), FlagStoreError> {
        let started = std::time::Instant::now();
        let result = self
            .store
            .set_deployment_flag(
                command.deployment.as_str(),
                command.flag.as_str(),
                command.enabled,
            )
            .await
            .map_err(|_| FlagStoreError::Save);
        telemetry::store_result("save", started, result.is_ok());
        result
    }
}

impl SettingsFlagStore<'_> {
    async fn load_snapshot(
        &self,
        deployment: &DeploymentId,
    ) -> Result<FlagSnapshot, FlagStoreError> {
        let rows = self
            .store
            .list_deployment_flags(deployment.as_str())
            .await
            .map_err(|_| FlagStoreError::Load)?;
        let values = rows
            .into_iter()
            .map(|(name, enabled)| {
                FlagName::parse(&name)
                    .map(|flag| (flag, enabled))
                    .map_err(|_| FlagStoreError::InvalidSnapshot)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FlagSnapshot::from_values(values))
    }
}

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
