//! Tests for carrying onboarding credentials into the wizard's snapshot.

use crate::bootstrap::merge_axinite_env_into_context;
use crate::config::{Config, EnvContext};
use crate::settings::Settings;
use crate::setup::wizard::{SetupConfig, SetupWizard};
use secrecy::ExposeSecret;

/// A name that cannot collide with a real provider key.
const ONBOARDED_KEY_VAR: &str = "AXINITE_TEST_ONBOARDED_API_KEY";
const ONBOARDED_KEY_VALUE: &str = "sk-onboarded-test-key";

#[test]
fn record_api_key_updates_the_wizard_snapshot() {
    let mut wizard =
        SetupWizard::with_config_and_env_context(SetupConfig::default(), EnvContext::default());

    wizard.record_api_key(ONBOARDED_KEY_VAR, ONBOARDED_KEY_VALUE);

    assert_eq!(
        wizard.env_context().get(ONBOARDED_KEY_VAR),
        Some(ONBOARDED_KEY_VALUE),
        "startup resolves config from the wizard's snapshot, so a key entered \
         during onboarding must be recorded there"
    );

    // `record_api_key` also publishes the value through the legacy ambient
    // overlay; remove it so this test cannot influence others in the process.
    crate::config::remove_single_var(ONBOARDED_KEY_VAR);
}

/// With no secrets database, a provider credential entered during onboarding
/// must reach the configuration startup resolves from the handoff snapshot.
#[cfg(any(feature = "postgres", feature = "libsql"))]
#[tokio::test]
async fn onboarding_credentials_reach_startup_configuration() {
    const API_KEY_VAR: &str = "ANTHROPIC_API_KEY";
    const API_KEY_VALUE: &str = "sk-onboarded-test-key";

    let dir = tempfile::tempdir().expect("create tempdir");
    let base_dir = dir.path().join("axinite-home");
    let pre_onboarding = EnvContext::default()
        .with_env("AXINITE_BASE_DIR", base_dir.display().to_string())
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test")
        .with_env("LLM_BACKEND", "anthropic");

    let mut wizard =
        SetupWizard::with_config_and_env_context(SetupConfig::default(), pre_onboarding.clone());
    wizard.record_api_key(API_KEY_VAR, API_KEY_VALUE);

    let startup_context =
        merge_axinite_env_into_context(wizard.env_context()).expect("handoff should merge");
    let config = Config::from_context(&startup_context, &Settings::default())
        .await
        .expect("handoff snapshot should resolve a configuration");

    let api_key = config
        .llm
        .provider
        .as_ref()
        .and_then(|provider| provider.api_key.as_ref())
        .map(ExposeSecret::expose_secret);
    assert_eq!(
        api_key,
        Some(API_KEY_VALUE),
        "startup resolves configuration from the merged snapshot, so an \
         entered credential must reach it"
    );
    assert_eq!(
        pre_onboarding.get(API_KEY_VAR),
        None,
        "control: the pre-onboarding snapshot must not contain the credential, \
         so this test fails if startup uses the original snapshot"
    );

    // `record_api_key` also publishes through the legacy ambient overlay;
    // clear it so this test cannot influence others in the process.
    crate::config::remove_single_var(API_KEY_VAR);
}
