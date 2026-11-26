//! Interactive setup helpers for tool-required secrets.

use std::io::Write;
use std::path::PathBuf;

use tokio::fs;

use crate::secrets::CreateSecretParams;
use crate::tools::wasm::CapabilitiesFile;

use super::auth::read_hidden_input;
use super::default_tools_dir;
use super::init_secrets_store;
use super::printing::validate_tool_name;

async fn load_tool_setup(
    tools_dir: &std::path::Path,
    name: &str,
) -> anyhow::Result<(CapabilitiesFile, PathBuf)> {
    let caps_path = tools_dir.join(format!("{name}.capabilities.json"));
    if !fs::try_exists(&caps_path).await? {
        anyhow::bail!(
            "Tool '{}' not found or has no capabilities file at {}",
            name,
            caps_path.display()
        );
    }

    let content = fs::read_to_string(&caps_path).await?;
    let caps = CapabilitiesFile::from_json(&content)
        .map_err(|e| anyhow::anyhow!("Invalid capabilities file: {e}"))?;
    Ok((caps, caps_path))
}

fn print_setup_banner(display_name: &str) {
    let title: String = format!("{display_name} Setup").chars().take(62).collect();
    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  {:^62}║", title);
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
}

fn should_prompt_for_secret_blocking(prompt: &str, already_exists: bool) -> anyhow::Result<bool> {
    if already_exists {
        println!("  ✓ {} (already configured)", prompt);

        print!("    Replace? [y/N]: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok(input.trim().eq_ignore_ascii_case("y"))
    } else {
        Ok(true)
    }
}

fn prompt_secret_value_blocking(
    prompt: &str,
    optional: bool,
    secret_name: &str,
) -> anyhow::Result<Option<String>> {
    if optional {
        print!("  {} (optional, Enter to skip): ", prompt);
    } else {
        print!("  {}: ", prompt);
    }
    std::io::stdout().flush()?;
    let value = read_hidden_input()?;
    println!();

    if value.is_empty() {
        if optional {
            println!("    Skipped.");
            return Ok(None);
        } else {
            anyhow::bail!("Required secret '{}' cannot be empty.", secret_name);
        }
    }

    Ok(Some(value))
}

async fn should_prompt_for_secret(prompt: &str, already_exists: bool) -> anyhow::Result<bool> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || should_prompt_for_secret_blocking(&prompt, already_exists))
        .await
        .map_err(|e| anyhow::anyhow!("interactive prompt task failed: {e}"))?
}

async fn prompt_secret_value(
    prompt: &str,
    optional: bool,
    secret_name: &str,
) -> anyhow::Result<Option<String>> {
    let prompt = prompt.to_string();
    let secret_name = secret_name.to_string();
    tokio::task::spawn_blocking(move || {
        prompt_secret_value_blocking(&prompt, optional, &secret_name)
    })
    .await
    .map_err(|e| anyhow::anyhow!("interactive secret input task failed: {e}"))?
}

/// One secret to request during tool setup.
struct SecretRequest<'a> {
    name: &'a str,
    prompt: &'a str,
    optional: bool,
}

/// Prompt for one secret, unless the user keeps an existing value or skips it,
/// and save it for `tool_name`. Returns whether a value was saved.
async fn prompt_and_save_secret(
    secrets_store: &(dyn crate::secrets::SecretsStore + Send + Sync),
    user_id: &str,
    tool_name: &str,
    secret: SecretRequest<'_>,
) -> anyhow::Result<bool> {
    let already_exists = secrets_store
        .exists(user_id, secret.name)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to check whether secret '{}' exists: {e}",
                secret.name
            )
        })?;

    if !should_prompt_for_secret(secret.prompt, already_exists).await? {
        return Ok(false);
    }
    let Some(value) = prompt_secret_value(secret.prompt, secret.optional, secret.name).await?
    else {
        return Ok(false);
    };

    let params = CreateSecretParams::new(secret.name, &value).with_provider(tool_name.to_string());
    secrets_store
        .create(user_id, params)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to save secret '{}': {}", secret.name, e))?;

    println!("    ✓ Saved.");
    Ok(true)
}

/// Configure required secrets for a tool via its `setup.required_secrets` schema.
pub(super) async fn setup_tool(
    name: String,
    dir: Option<PathBuf>,
    user_id: String,
) -> anyhow::Result<()> {
    validate_tool_name(&name)?;
    let tools_dir = dir.unwrap_or_else(default_tools_dir);
    let (caps, _caps_path) = load_tool_setup(&tools_dir, &name).await?;

    let setup = caps.setup.ok_or_else(|| {
        anyhow::anyhow!(
            "Tool '{}' has no setup configuration.\n\
             The tool may not require setup, or setup is not defined.\n\
             Try 'axinite tool auth {}' for OAuth-based authentication.",
            name,
            name
        )
    })?;

    if setup.required_secrets.is_empty() {
        println!("Tool '{}' has no required secrets.", name);
        return Ok(());
    }

    let display_name = caps
        .auth
        .as_ref()
        .and_then(|auth| auth.display_name.as_deref())
        .unwrap_or(&name);

    print_setup_banner(display_name);

    let secrets_store = init_secrets_store().await?;
    let mut any_saved = false;

    for secret in &setup.required_secrets {
        let request = SecretRequest {
            name: &secret.name,
            prompt: &secret.prompt,
            optional: secret.optional,
        };
        any_saved |=
            prompt_and_save_secret(secrets_store.as_ref(), &user_id, &name, request).await?;
    }

    println!();
    if any_saved {
        println!("  ✓ {} setup complete!", display_name);
    } else {
        println!("  No changes made.");
    }
    println!();

    Ok(())
}
