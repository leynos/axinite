//! WASM channel setup flow driven by a capabilities setup schema.

use secrecy::{ExposeSecret, SecretString};

use crate::setup::prompts::{
    confirm, optional_input, print_error, print_info, print_success, secret_input,
};

use super::secrets::{ChannelSetupError, SecretsContext, generate_secret_with_length};

/// Result of WASM channel setup.
#[derive(Debug, Clone)]
pub struct WasmChannelSetupResult {
    pub enabled: bool,
    pub channel_name: String,
}

/// Set up a WASM channel using its capabilities file setup schema.
///
/// Reads setup requirements from the channel's capabilities file and
/// prompts the user for each required secret.
pub async fn setup_wasm_channel(
    secrets: &SecretsContext,
    channel_name: &str,
    setup: &crate::channels::wasm::SetupSchema,
) -> Result<WasmChannelSetupResult, ChannelSetupError> {
    println!("{} Setup:", channel_name);
    println!();

    for secret_config in &setup.required_secrets {
        if should_keep_existing_secret(secrets, &secret_config.name).await? {
            continue;
        }

        // A `None` value means the user skipped an optional secret.
        let Some(value) = resolve_secret_value(secret_config)? else {
            continue;
        };

        secrets.save_secret(&secret_config.name, &value).await?;
        print_success(&format!("{} saved to database", secret_config.name));
    }

    // TODO: Substitute secrets into the validation URL and make a
    // GET request to verify the configured credentials actually work.
    if let Some(ref validation_endpoint) = setup.validation_endpoint {
        print_info(&format!(
            "Validation endpoint configured: {} (validation not yet implemented)",
            validation_endpoint
        ));
    }

    print_success(&format!("{} channel configured", channel_name));

    Ok(WasmChannelSetupResult {
        enabled: true,
        channel_name: channel_name.to_string(),
    })
}

/// Return `true` when the secret already exists and the user declines to
/// replace it.
async fn should_keep_existing_secret(
    secrets: &SecretsContext,
    name: &str,
) -> Result<bool, ChannelSetupError> {
    if !secrets.secret_exists(name).await {
        return Ok(false);
    }
    print_info(&format!("Existing {} found in database.", name));
    Ok(!confirm("Replace existing value?", false)?)
}

/// Obtain a secret value from the user, auto-generation, or validation.
///
/// Returns `Ok(None)` when an optional secret is skipped (no input and no
/// auto-generate configuration).
fn resolve_secret_value(
    secret_config: &crate::channels::wasm::SecretSetupSchema,
) -> Result<Option<SecretString>, ChannelSetupError> {
    if secret_config.optional {
        resolve_optional_secret(secret_config)
    } else {
        prompt_required_secret(secret_config).map(Some)
    }
}

/// Prompt for an optional secret, falling back to auto-generation when the
/// user provides no value.
fn resolve_optional_secret(
    secret_config: &crate::channels::wasm::SecretSetupSchema,
) -> Result<Option<SecretString>, ChannelSetupError> {
    let input_value = optional_input(&secret_config.prompt, Some("leave empty to auto-generate"))?;
    if let Some(v) = input_value
        && !v.is_empty()
    {
        return Ok(Some(SecretString::from(v)));
    }
    Ok(auto_generate_secret(secret_config))
}

/// Auto-generate a secret when the schema configures generation; otherwise
/// return `None` so the caller skips the secret.
fn auto_generate_secret(
    secret_config: &crate::channels::wasm::SecretSetupSchema,
) -> Option<SecretString> {
    let auto_gen = secret_config.auto_generate.as_ref()?;
    let generated = generate_secret_with_length(auto_gen.length);
    print_info(&format!(
        "Auto-generated {} ({} bytes)",
        secret_config.name, auto_gen.length
    ));
    Some(SecretString::from(generated))
}

/// Prompt for a required secret and check it against the schema's
/// validation pattern, when one is configured.
fn prompt_required_secret(
    secret_config: &crate::channels::wasm::SecretSetupSchema,
) -> Result<SecretString, ChannelSetupError> {
    let input_value = secret_input(&secret_config.prompt)?;

    if let Some(ref pattern) = secret_config.validation {
        let re = regex::Regex::new(pattern).map_err(|e| {
            ChannelSetupError::Validation(format!("Invalid validation pattern: {}", e))
        })?;
        if !re.is_match(input_value.expose_secret()) {
            print_error(&format!(
                "Value does not match expected format: {}",
                pattern
            ));
            return Err(ChannelSetupError::Validation(
                "Validation failed".to_string(),
            ));
        }
    }

    Ok(input_value)
}
