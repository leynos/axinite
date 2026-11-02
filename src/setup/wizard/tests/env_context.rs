//! Tests for carrying onboarding credentials into the wizard's snapshot.

use crate::config::EnvContext;
use crate::setup::wizard::{SetupConfig, SetupWizard};

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
