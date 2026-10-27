//! Credential resolution from explicit configuration snapshots.

use aws_sdk_bedrockruntime::config::{Credentials, ProvideCredentials};
use secrecy::ExposeSecret;

use crate::config::{EnvContext, LlmConfig};
use crate::llm::bedrock::configure_credentials;
use crate::llm::config::BedrockConfig;
use crate::settings::Settings;

fn resolve_bedrock(
    context: &EnvContext,
) -> Result<Option<BedrockConfig>, crate::error::ConfigError> {
    let settings = Settings {
        llm_backend: Some("bedrock".to_string()),
        ..Default::default()
    };
    LlmConfig::resolve_from(context, &settings).map(|config| config.bedrock)
}

#[rstest::rstest]
#[case(None)]
#[case(Some("snapshot-session"))]
#[tokio::test]
async fn snapshot_credentials_reach_sdk_without_exposing_them_in_debug(
    #[case] session_token: Option<&str>,
) {
    let mut context = EnvContext::default()
        .with_env("BEDROCK_MODEL", "model")
        .with_env("AWS_ACCESS_KEY_ID", "snapshot-access")
        .with_env("AWS_SECRET_ACCESS_KEY", "snapshot-secret");
    if let Some(token) = session_token {
        context = context.with_env("AWS_SESSION_TOKEN", token);
    }
    let config = resolve_bedrock(&context)
        .expect("resolve Bedrock settings")
        .expect("Bedrock configuration");
    let debug = format!("{config:?}");
    assert!(!debug.contains("snapshot-access"));
    assert!(!debug.contains("snapshot-secret"));
    assert!(!debug.contains("snapshot-session"));
    let captured = config.credentials.as_ref().expect("captured credentials");
    assert_eq!(captured.access_key_id.expose_secret(), "snapshot-access");
    assert_eq!(
        captured.secret_access_key.expose_secret(),
        "snapshot-secret"
    );
    assert_eq!(
        captured
            .session_token
            .as_ref()
            .map(|token| token.expose_secret()),
        session_token
    );

    let builder = aws_config::defaults(aws_config::BehaviorVersion::latest());
    let sdk_config = configure_credentials(builder, &config).load().await;
    let actual = sdk_config
        .credentials_provider()
        .expect("configured credentials provider")
        .provide_credentials()
        .await
        .expect("resolve snapshot credentials");
    assert_eq!(actual.access_key_id(), "snapshot-access");
    assert_eq!(actual.secret_access_key(), "snapshot-secret");
    assert_eq!(actual.session_token(), session_token);
}

#[rstest::rstest]
#[case(None, None)]
#[case(Some("access"), None)]
#[case(None, Some("secret"))]
#[case(Some(""), Some("secret"))]
#[tokio::test]
async fn incomplete_key_pair_leaves_sdk_provider_unchanged(
    #[case] access_key: Option<&str>,
    #[case] secret_key: Option<&str>,
) {
    let mut context = EnvContext::default().with_env("BEDROCK_MODEL", "model");
    if let Some(access_key) = access_key {
        context = context.with_env("AWS_ACCESS_KEY_ID", access_key);
    }
    if let Some(secret_key) = secret_key {
        context = context.with_env("AWS_SECRET_ACCESS_KEY", secret_key);
    }
    let config = resolve_bedrock(&context)
        .expect("resolve Bedrock settings")
        .expect("Bedrock configuration");
    assert!(config.credentials.is_none());

    let fallback = Credentials::new("chain-access", "chain-secret", None, None, "test-chain");
    let builder =
        aws_config::defaults(aws_config::BehaviorVersion::latest()).credentials_provider(fallback);
    let sdk_config = configure_credentials(builder, &config).load().await;
    let actual = sdk_config
        .credentials_provider()
        .expect("fallback provider")
        .provide_credentials()
        .await
        .expect("resolve fallback credentials");
    assert_eq!(actual.access_key_id(), "chain-access");
}
