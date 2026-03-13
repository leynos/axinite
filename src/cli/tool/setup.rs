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
    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  {:^62}║", format!("{display_name} Setup"));
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
}

fn should_prompt_for_secret(prompt: &str, already_exists: bool) -> anyhow::Result<bool> {
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

fn prompt_secret_value(prompt: &str, optional: bool, secret_name: &str) -> anyhow::Result<String> {
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
        } else {
            anyhow::bail!("Required secret '{}' cannot be empty.", secret_name);
        }
    }

    Ok(value)
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
             Try 'ironclaw tool auth {}' for OAuth-based authentication.",
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
        let already_exists = secrets_store
            .exists(&user_id, &secret.name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check whether secret exists: {e}"))?;

        if !should_prompt_for_secret(&secret.prompt, already_exists)? {
            continue;
        }
        let value = prompt_secret_value(&secret.prompt, secret.optional, &secret.name)?;

        let params = CreateSecretParams::new(&secret.name, &value).with_provider(name.to_string());
        secrets_store
            .create(&user_id, params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to save secret: {}", e))?;

        println!("    ✓ Saved.");
        any_saved = true;
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
