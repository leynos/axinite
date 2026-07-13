//! Credential grant parsing and env var validation for sandbox jobs.
//!
//! Sandbox jobs may request secrets from the encrypted store to be injected
//! into the container as environment variables. Env var names are validated
//! against a denylist of variables that could hijack process behaviour.

use crate::orchestrator::auth::CredentialGrant;
use crate::tools::tool::ToolError;

use super::CreateJobTool;

/// Env var names that could be abused to hijack process behavior.
const DANGEROUS_ENV_VARS: &[&str] = &[
    // Dynamic linker hijacking
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    // Shell behavior
    "BASH_ENV",
    "ENV",
    "CDPATH",
    "IFS",
    "PATH",
    "HOME",
    // Language runtime library path hijacking
    "PYTHONPATH",
    "NODE_PATH",
    "PERL5LIB",
    "RUBYLIB",
    "CLASSPATH",
    // JVM injection
    "JAVA_TOOL_OPTIONS",
    "MAVEN_OPTS",
    "USER",
    "SHELL",
    "RUST_LOG",
];

/// Validate that an env var name is safe for container injection.
fn validate_env_var_name(name: &str) -> Result<(), ToolError> {
    if name.is_empty() {
        return Err(ToolError::InvalidParameters(
            "env var name cannot be empty".into(),
        ));
    }

    // Must match ^[A-Z_][A-Z0-9_]*$
    let valid = name
        .bytes()
        .enumerate()
        .all(|(i, b)| matches!(b, b'A'..=b'Z' | b'_') || (i > 0 && b.is_ascii_digit()));

    if !valid {
        return Err(ToolError::InvalidParameters(format!(
            "env var '{}' must match [A-Z_][A-Z0-9_]* (uppercase, underscores, digits)",
            name
        )));
    }

    if DANGEROUS_ENV_VARS.contains(&name) {
        return Err(ToolError::InvalidParameters(format!(
            "env var '{}' is on the denylist (could hijack process behavior)",
            name
        )));
    }

    Ok(())
}

impl CreateJobTool {
    /// Parse and validate the `credentials` parameter.
    ///
    /// Each key is a secret name (must exist in SecretsStore), each value is the
    /// env var name the container should receive it as. Returns an empty vec if
    /// no credentials were requested.
    pub(super) async fn parse_credentials(
        &self,
        params: &serde_json::Value,
        user_id: &str,
    ) -> Result<Vec<CredentialGrant>, ToolError> {
        let creds_obj = match params.get("credentials").and_then(|v| v.as_object()) {
            Some(obj) if !obj.is_empty() => obj,
            _ => return Ok(vec![]),
        };

        const MAX_CREDENTIAL_GRANTS: usize = 20;
        if creds_obj.len() > MAX_CREDENTIAL_GRANTS {
            return Err(ToolError::InvalidParameters(format!(
                "too many credential grants ({}, max {})",
                creds_obj.len(),
                MAX_CREDENTIAL_GRANTS
            )));
        }

        let secrets = match &self.secrets_store {
            Some(s) => s,
            None => {
                return Err(ToolError::ExecutionFailed(
                    "credentials requested but no secrets store is configured. \
                     Set SECRETS_MASTER_KEY to enable credential management."
                        .to_string(),
                ));
            }
        };

        let mut grants = Vec::with_capacity(creds_obj.len());
        for (secret_name, env_var_value) in creds_obj {
            let env_var = env_var_value.as_str().ok_or_else(|| {
                ToolError::InvalidParameters(format!(
                    "credential env var for '{}' must be a string",
                    secret_name
                ))
            })?;

            validate_env_var_name(env_var)?;

            // Validate the secret actually exists
            let exists = secrets.exists(user_id, secret_name).await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to check secret '{}': {}",
                    secret_name, e
                ))
            })?;

            if !exists {
                return Err(ToolError::ExecutionFailed(format!(
                    "secret '{}' not found. Store it first via 'ironclaw tool auth' or the web UI.",
                    secret_name
                )));
            }

            grants.push(CredentialGrant {
                secret_name: secret_name.clone(),
                env_var: env_var.to_string(),
            });
        }

        Ok(grants)
    }
}
