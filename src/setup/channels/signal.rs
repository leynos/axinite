//! Signal channel setup flow and allow-list validation.

use url::Url;
use uuid::Uuid;

use crate::settings::Settings;
use crate::setup::prompts::{input, optional_input, print_error, print_info, print_success};

use super::secrets::ChannelSetupError;

/// Result of Signal channel setup.
#[derive(Debug, Clone)]
pub struct SignalSetupResult {
    pub enabled: bool,
    pub http_url: String,
    pub account: String,
    pub allow_from: String,
    pub allow_from_groups: String,
    pub dm_policy: String,
    pub group_policy: String,
    pub group_allow_from: String,
}

fn validate_e164(account: &str) -> Result<(), String> {
    if !account.starts_with('+') {
        return Err("E.164 account must start with '+'".to_string());
    }
    let digits = &account[1..];
    if digits.is_empty() {
        return Err("E.164 account must have digits after '+'".to_string());
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("E.164 account must contain only digits after '+'".to_string());
    }
    if digits.len() < 7 || digits.len() > 15 {
        return Err("E.164 account must be 7-15 digits after '+'".to_string());
    }
    Ok(())
}

fn validate_allow_from_list(list: &str) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    for (i, item) in list.split(',').enumerate() {
        validate_allow_from_entry(i, item.trim())?;
    }
    Ok(())
}

/// Validate a single trimmed `allow_from` entry, reporting its index in
/// error messages.
fn validate_allow_from_entry(index: usize, entry: &str) -> Result<(), String> {
    if entry.is_empty() || entry == "*" {
        return Ok(());
    }
    if let Some(uuid_part) = entry.strip_prefix("uuid:") {
        if Uuid::parse_str(uuid_part).is_err() {
            return Err(format!(
                "allow_from[{}]: '{}' is not a valid UUID (after 'uuid:' prefix)",
                index, entry
            ));
        }
        return Ok(());
    }
    if is_valid_sender_id(entry) {
        return Ok(());
    }
    Err(format!(
        "allow_from[{}]: '{}' must be '*', E.164 phone number, UUID, or 'uuid:<id>'",
        index, entry
    ))
}

/// Return `true` when the entry is a bare E.164 phone number or a bare UUID.
fn is_valid_sender_id(entry: &str) -> bool {
    validate_e164(entry).is_ok() || Uuid::parse_str(entry).is_ok()
}

fn validate_allow_from_groups_list(list: &str) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    for (i, item) in list.split(',').enumerate() {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "*" {
            continue;
        }
        if trimmed.is_empty() {
            return Err(format!(
                "allow_from_groups[{}]: group ID cannot be empty",
                i
            ));
        }
    }
    Ok(())
}

/// Set up Signal channel.
/// `Settings` is reserved for future use
pub async fn setup_signal(_settings: &Settings) -> Result<SignalSetupResult, ChannelSetupError> {
    println!("Signal Channel Setup:");
    println!();
    print_info("Signal channel connects to a signal-cli daemon running in HTTP mode.");
    println!();

    let http_url = prompt_http_url()?;
    let account = prompt_account()?;
    let policies = prompt_signal_policies(&account)?;

    validate_signal_policies(&policies)?;

    let result = SignalSetupResult {
        enabled: true,
        http_url,
        account,
        allow_from: policies.allow_from,
        allow_from_groups: policies.allow_from_groups,
        dm_policy: policies.dm_policy,
        group_policy: policies.group_policy,
        group_allow_from: policies.group_allow_from,
    };
    print_signal_summary(&result);
    Ok(result)
}

/// Allow-list and policy answers gathered from the user during setup.
struct SignalPolicyAnswers {
    allow_from: String,
    dm_policy: String,
    allow_from_groups: String,
    group_policy: String,
    group_allow_from: String,
}

/// Prompt for the signal-cli HTTP URL and validate its scheme.
fn prompt_http_url() -> Result<String, ChannelSetupError> {
    let http_url = input("Signal-cli HTTP URL")?;
    match Url::parse(&http_url) {
        Ok(url) if url.scheme() == "http" || url.scheme() == "https" => Ok(http_url),
        Ok(_) => {
            print_error("URL must use http or https scheme");
            Err(ChannelSetupError::Validation(
                "Invalid HTTP URL: must use http or https scheme".to_string(),
            ))
        }
        Err(e) => {
            print_error(&format!("Invalid URL: {}", e));
            Err(ChannelSetupError::Validation(format!(
                "Invalid HTTP URL: {}",
                e
            )))
        }
    }
}

/// Prompt for the Signal account and validate it as an E.164 number.
fn prompt_account() -> Result<String, ChannelSetupError> {
    let account = input("Signal account (E.164)")?;
    if let Err(e) = validate_e164(&account) {
        print_error(&e);
        return Err(ChannelSetupError::Validation(e));
    }
    Ok(account)
}

/// Prompt for the allow-list and policy settings, applying defaults.
fn prompt_signal_policies(account: &str) -> Result<SignalPolicyAnswers, ChannelSetupError> {
    let allow_from = optional_input(
        "Allow from (comma-separated: E.164 numbers, '*' for anyone, UUIDs or 'uuid:<id>'; empty for self-only)",
        Some(&format!("default: {} (self-only)", account)),
    )?
    .unwrap_or_else(|| account.to_string());

    let dm_policy = optional_input(
        "DM policy (open, allowlist, pairing)",
        Some("default: pairing"),
    )?
    .unwrap_or_else(|| "pairing".to_string());

    let allow_from_groups = optional_input(
        "Allow from groups (comma-separated group IDs, '*' for any group; empty for none)",
        Some("default: (none)"),
    )?
    .unwrap_or_default();

    let group_policy = optional_input(
        "Group policy (allowlist, open, disabled)",
        Some("default: allowlist"),
    )?
    .unwrap_or_else(|| "allowlist".to_string());

    let group_allow_from = optional_input(
        "Group allow from (comma-separated member IDs; empty to inherit from allow_from)",
        Some("default: (inherit from allow_from)"),
    )?
    .unwrap_or_default();

    Ok(SignalPolicyAnswers {
        allow_from,
        dm_policy,
        allow_from_groups,
        group_policy,
        group_allow_from,
    })
}

/// Validate the gathered allow-lists, echoing errors to the user.
fn validate_signal_policies(policies: &SignalPolicyAnswers) -> Result<(), ChannelSetupError> {
    if let Err(e) = validate_allow_from_list(&policies.allow_from) {
        print_error(&e);
        return Err(ChannelSetupError::Validation(e));
    }
    if let Err(e) = validate_allow_from_groups_list(&policies.allow_from_groups) {
        print_error(&e);
        return Err(ChannelSetupError::Validation(e));
    }
    Ok(())
}

/// Print a summary of the configured Signal channel.
fn print_signal_summary(result: &SignalSetupResult) {
    println!();
    print_success(&format!(
        "Signal channel configured for account: {}",
        result.account
    ));
    print_info(&format!("HTTP URL: {}", result.http_url));
    if result.allow_from == result.account {
        print_info("Allow from: self-only");
    } else {
        print_info(&format!("Allow from: {}", result.allow_from));
    }
    print_info(&format!("DM policy: {}", result.dm_policy));
    if result.allow_from_groups.is_empty() {
        print_info("Allow from groups: (none)");
    } else {
        print_info(&format!("Allow from groups: {}", result.allow_from_groups));
    }
    print_info(&format!("Group policy: {}", result.group_policy));
    if result.group_allow_from.is_empty() {
        print_info("Group allow from: (inherits from allow_from)");
    } else {
        print_info(&format!("Group allow from: {}", result.group_allow_from));
    }
}
