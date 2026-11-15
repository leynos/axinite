//! Shared CLI formatting helpers for tool output.

use crate::tools::wasm::CapabilitiesFile;

/// Print a brief capabilities summary.
pub(super) fn print_capabilities_summary(caps: &CapabilitiesFile) {
    let mut parts = Vec::new();

    if let Some(ref http) = caps.http {
        let hosts: Vec<_> = http
            .allowlist
            .iter()
            .map(|entry| entry.host.as_str())
            .collect();
        if !hosts.is_empty() {
            parts.push(format!("http: {}", hosts.join(", ")));
        }
    }

    if let Some(ref secrets) = caps.secrets
        && !secrets.allowed_names.is_empty()
    {
        parts.push(format!("secrets: {}", secrets.allowed_names.len()));
    }

    if let Some(ref workspace) = caps.workspace
        && !workspace.allowed_prefixes.is_empty()
    {
        parts.push("workspace: read".to_string());
    }

    if !parts.is_empty() {
        println!("    Perms: {}", parts.join(", "));
    }
}

/// Print detailed capabilities.
pub(super) fn print_capabilities_detail(caps: &CapabilitiesFile) {
    print_http_allowlist(caps);
    print_http_credentials(caps);
    print_http_rate_limit(caps);
    print_secret_names(caps);
    print_tool_aliases(caps);
    print_workspace_prefixes(caps);
}

/// Print the HTTP allowlist entries declared by `caps`.
fn print_http_allowlist(caps: &CapabilitiesFile) {
    let Some(ref http) = caps.http else {
        return;
    };
    println!("  HTTP:");
    for endpoint in &http.allowlist {
        let methods = if endpoint.methods.is_empty() {
            "*".to_string()
        } else {
            endpoint.methods.join(", ")
        };
        let path = endpoint.path_prefix.as_deref().unwrap_or("/*");
        println!("    {} {} {}", methods, endpoint.host, path);
    }
}

/// Print the HTTP credential mappings declared by `caps`.
fn print_http_credentials(caps: &CapabilitiesFile) {
    let Some(ref http) = caps.http else {
        return;
    };
    if http.credentials.is_empty() {
        return;
    }
    println!("  Credentials:");
    for (key, cred) in &http.credentials {
        println!("    {}: {} -> {:?}", key, cred.secret_name, cred.location);
    }
}

/// Print the HTTP rate limit declared by `caps`, when one is configured.
fn print_http_rate_limit(caps: &CapabilitiesFile) {
    let Some(ref http) = caps.http else {
        return;
    };
    let Some(ref rate) = http.rate_limit else {
        return;
    };
    println!(
        "  Rate limit: {}/min, {}/hour",
        rate.requests_per_minute, rate.requests_per_hour
    );
}

/// Print the secret names `caps` may check for.
fn print_secret_names(caps: &CapabilitiesFile) {
    let Some(ref secrets) = caps.secrets else {
        return;
    };
    if secrets.allowed_names.is_empty() {
        return;
    }
    println!("  Secrets (existence check only):");
    for name in &secrets.allowed_names {
        println!("    {}", name);
    }
}

/// Print the tool aliases declared by `caps`.
fn print_tool_aliases(caps: &CapabilitiesFile) {
    let Some(ref tool_invoke) = caps.tool_invoke else {
        return;
    };
    if tool_invoke.aliases.is_empty() {
        return;
    }
    println!("  Tool aliases:");
    for (alias, real_name) in &tool_invoke.aliases {
        println!("    {} -> {}", alias, real_name);
    }
}

/// Print the workspace read prefixes declared by `caps`.
fn print_workspace_prefixes(caps: &CapabilitiesFile) {
    let Some(ref workspace) = caps.workspace else {
        return;
    };
    if workspace.allowed_prefixes.is_empty() {
        return;
    }
    println!("  Workspace read prefixes:");
    for prefix in &workspace.allowed_prefixes {
        println!("    {}", prefix);
    }
}

/// Validate a tool name to prevent path traversal.
pub(super) fn validate_tool_name(name: &str) -> anyhow::Result<()> {
    if is_invalid_tool_name(name) {
        anyhow::bail!(
            "Invalid tool name '{}': must not contain path separators or '..'",
            name
        );
    }
    Ok(())
}

fn is_invalid_tool_name(name: &str) -> bool {
    name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
}
