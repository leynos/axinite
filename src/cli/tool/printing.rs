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
    if let Some(ref http) = caps.http {
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

        if !http.credentials.is_empty() {
            println!("  Credentials:");
            for (key, cred) in &http.credentials {
                println!("    {}: {} -> {:?}", key, cred.secret_name, cred.location);
            }
        }

        if let Some(ref rate) = http.rate_limit {
            println!(
                "  Rate limit: {}/min, {}/hour",
                rate.requests_per_minute, rate.requests_per_hour
            );
        }
    }

    if let Some(ref secrets) = caps.secrets
        && !secrets.allowed_names.is_empty()
    {
        println!("  Secrets (existence check only):");
        for name in &secrets.allowed_names {
            println!("    {}", name);
        }
    }

    if let Some(ref tool_invoke) = caps.tool_invoke
        && !tool_invoke.aliases.is_empty()
    {
        println!("  Tool aliases:");
        for (alias, real_name) in &tool_invoke.aliases {
            println!("    {} -> {}", alias, real_name);
        }
    }

    if let Some(ref workspace) = caps.workspace
        && !workspace.allowed_prefixes.is_empty()
    {
        println!("  Workspace read prefixes:");
        for prefix in &workspace.allowed_prefixes {
            println!("    {}", prefix);
        }
    }
}

/// Validate a tool name to prevent path traversal.
pub(super) fn validate_tool_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        anyhow::bail!(
            "Invalid tool name '{}': must not contain path separators or '..'",
            name
        );
    }
    Ok(())
}
