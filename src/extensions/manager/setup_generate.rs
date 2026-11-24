//! Auto-generation of channel-declared secrets.
//!
//! Channels may declare a secret that the host generates when the user leaves
//! it blank. Generation preserves the manifest's byte length and runs only for
//! secrets that are still absent after the submission's own writes.

use rand::RngCore;
use rand::rngs::OsRng;

use crate::extensions::setup::{CredentialName, FieldKey};
use crate::secrets::CreateSecretParams;

use super::ExtensionManager;
use super::setup_apply::{Execution, FieldOutcome};

impl ExtensionManager {
    /// Generate channel-defined secrets that are still absent.
    pub(super) async fn auto_generate_missing(&self, name: &str, execution: &mut Execution) {
        let Ok(Some(cap_file)) = self.load_channel_capabilities(name).await else {
            return;
        };
        let Ok(snapshot) = self.setup_snapshot(name).await else {
            return;
        };
        for secret_def in &cap_file.setup.required_secrets {
            let Some(auto) = &secret_def.auto_generate else {
                continue;
            };
            if execution.stopped {
                return;
            }
            let Ok(field) = CredentialName::parse(secret_def.name.clone()) else {
                continue;
            };
            if snapshot.secret_provided(&field) != Some(false) {
                continue;
            }
            self.generate_secret(name, &field, auto.length, execution)
                .await;
        }
    }

    async fn generate_secret(
        &self,
        provider: &str,
        field: &CredentialName,
        length: usize,
        execution: &mut Execution,
    ) {
        let value = random_hex(length);
        let params =
            CreateSecretParams::new(field.as_str(), &value).with_provider(provider.to_string());
        let outcome = match self.secrets.create(&self.user_id, params).await {
            Ok(_) => FieldOutcome::Stored,
            Err(_) => FieldOutcome::Indeterminate,
        };
        execution.record(FieldKey::Secret(field.clone()), outcome);
        tracing::info!(
            "Auto-generated secret '{}' for channel '{}'",
            field.as_str(),
            provider
        );
    }
}

/// Generate `length` random bytes, hex-encoded.
fn random_hex(length: usize) -> String {
    let mut bytes = vec![0u8; length];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
