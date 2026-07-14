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
        // Check if this secret already exists
        if secrets.secret_exists(&secret_config.name).await {
            print_info(&format!(
                "Existing {} found in database.",
                secret_config.name
            ));
            if !confirm("Replace existing value?", false)? {
                continue;
            }
        }

        // Get the value from user or auto-generate
        let value = if secret_config.optional {
            let input_value =
                optional_input(&secret_config.prompt, Some("leave empty to auto-generate"))?;

            if let Some(v) = input_value {
                if !v.is_empty() {
                    SecretString::from(v)
                } else if let Some(ref auto_gen) = secret_config.auto_generate {
                    let generated = generate_secret_with_length(auto_gen.length);
                    print_info(&format!(
                        "Auto-generated {} ({} bytes)",
                        secret_config.name, auto_gen.length
                    ));
                    SecretString::from(generated)
                } else {
                    continue; // Skip optional secret with no auto-generate
                }
            } else if let Some(ref auto_gen) = secret_config.auto_generate {
                let generated = generate_secret_with_length(auto_gen.length);
                print_info(&format!(
                    "Auto-generated {} ({} bytes)",
                    secret_config.name, auto_gen.length
                ));
                SecretString::from(generated)
            } else {
                continue; // Skip optional secret with no auto-generate
            }
        } else {
            // Required secret
            let input_value = secret_input(&secret_config.prompt)?;

            // Validate if pattern is provided
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

            input_value
        };

        // Save the secret
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
