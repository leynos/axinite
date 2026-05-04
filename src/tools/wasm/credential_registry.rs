//! Shared credential registry for host-side WASM HTTP credential injection.

use std::collections::HashSet;
use std::sync::RwLock;

use crate::secrets::CredentialMapping;
use crate::tools::wasm::credential_injector::host_matches_pattern;

/// Thread-safe registry of credential mappings from all installed tools.
///
/// Aggregates credential mappings from WASM tools so the built-in HTTP tool can
/// auto-inject credentials for matching hosts. Uses `std::sync::RwLock` so
/// `requires_approval` can query it from synchronous code.
pub struct SharedCredentialRegistry {
    mappings: RwLock<Vec<OwnedCredentialMapping>>,
}

#[derive(Clone)]
struct OwnedCredentialMapping {
    owner: Option<String>,
    mapping: CredentialMapping,
}

impl SharedCredentialRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            mappings: RwLock::new(Vec::new()),
        }
    }

    /// Add credential mappings without assigning them to a specific tool owner.
    pub fn add_mappings(&self, mappings: impl IntoIterator<Item = CredentialMapping>) {
        self.add_mappings_for_owner(None, mappings);
    }

    /// Replace credential mappings owned by a specific tool.
    ///
    /// Re-registration for the same tool removes that tool's earlier mappings
    /// before adding the new set, without clobbering other tools that reference
    /// the same secret.
    pub fn add_mappings_for_tool(
        &self,
        tool_name: &str,
        mappings: impl IntoIterator<Item = CredentialMapping>,
    ) {
        self.add_mappings_for_owner(Some(tool_name), mappings);
    }

    fn add_mappings_for_owner(
        &self,
        owner: Option<&str>,
        mappings: impl IntoIterator<Item = CredentialMapping>,
    ) {
        let mappings = dedupe_mappings_by_secret_name(mappings);
        let owner = owner.map(str::to_string);

        match self.mappings.write() {
            Ok(mut guard) => {
                replace_mappings_by_secret_name(&mut guard, owner, mappings);
            }
            Err(poisoned) => {
                tracing::warn!(
                    "SharedCredentialRegistry RwLock poisoned during add_mappings; recovering"
                );
                let mut guard = poisoned.into_inner();
                replace_mappings_by_secret_name(&mut guard, owner, mappings);
            }
        }
    }

    /// Remove credential mappings whose `secret_name` matches any of the given names
    /// and whose owner matches `owner_id`.
    ///
    /// Called when an extension is unregistered/deactivated so its credential
    /// injection authority does not outlive the extension. Only removes mappings
    /// owned by the specified owner, leaving other tools' mappings for the same
    /// secret intact.
    pub fn remove_mappings_for_secrets(&self, owner_id: &str, secret_names: &[String]) {
        let secret_names = secret_names
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut guard = match self.mappings.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!(
                    "SharedCredentialRegistry RwLock poisoned during remove_mappings_for_secrets; recovering"
                );
                poisoned.into_inner()
            }
        };
        guard.retain(|m| {
            !(secret_names.contains(m.mapping.secret_name.as_str())
                && m.owner.as_deref() == Some(owner_id))
        });
    }

    /// Check if any credential mapping matches this host.
    pub fn has_credentials_for_host(&self, host: &str) -> bool {
        let guard = match self.mappings.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!(
                    "SharedCredentialRegistry RwLock poisoned during has_credentials_for_host; recovering"
                );
                poisoned.into_inner()
            }
        };
        guard.iter().any(|m| mapping_matches_host(m, host))
    }

    /// Get all credential mappings matching a host.
    pub fn find_for_host(&self, host: &str) -> Vec<CredentialMapping> {
        let guard = match self.mappings.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!(
                    "SharedCredentialRegistry RwLock poisoned during find_for_host; recovering"
                );
                poisoned.into_inner()
            }
        };
        guard
            .iter()
            .filter(|mapping| mapping_matches_host(mapping, host))
            .map(|owned| owned.mapping.clone())
            .collect()
    }
}

fn mapping_matches_host(mapping: &OwnedCredentialMapping, host: &str) -> bool {
    mapping
        .mapping
        .host_patterns
        .iter()
        .any(|pattern| host_matches_pattern(host, pattern))
}

fn dedupe_mappings_by_secret_name(
    mappings: impl IntoIterator<Item = CredentialMapping>,
) -> Vec<CredentialMapping> {
    let mappings = mappings.into_iter().collect::<Vec<_>>();
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for mapping in mappings.into_iter().rev() {
        if seen.insert(mapping.secret_name.clone()) {
            deduped.push(mapping);
        }
    }
    deduped.reverse();
    deduped
}

fn merge_host_patterns_from_existing(
    guard: &[OwnedCredentialMapping],
    mapping: &mut CredentialMapping,
) {
    let mut seen = mapping
        .host_patterns
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    for existing in guard
        .iter()
        .filter(|e| e.owner.is_none() && e.mapping.secret_name == mapping.secret_name)
    {
        for host_pattern in &existing.mapping.host_patterns {
            if seen.insert(host_pattern.clone()) {
                mapping.host_patterns.push(host_pattern.clone());
            }
        }
    }
}

fn replace_ownerless_mappings(
    guard: &mut Vec<OwnedCredentialMapping>,
    secret_names: &HashSet<String>,
    mut mappings: Vec<CredentialMapping>,
) {
    for mapping in &mut mappings {
        merge_host_patterns_from_existing(guard, mapping);
    }
    guard.retain(|m| m.owner.is_some() || !secret_names.contains(&m.mapping.secret_name));
    guard.extend(mappings.into_iter().map(|mapping| OwnedCredentialMapping {
        owner: None,
        mapping,
    }));
}

fn replace_owned_mappings(
    guard: &mut Vec<OwnedCredentialMapping>,
    owner: String,
    mappings: Vec<CredentialMapping>,
) {
    guard.retain(|m| m.owner.as_deref() != Some(owner.as_str()));
    guard.extend(mappings.into_iter().map(|mapping| OwnedCredentialMapping {
        owner: Some(owner.clone()),
        mapping,
    }));
}

fn replace_mappings_by_secret_name(
    guard: &mut Vec<OwnedCredentialMapping>,
    owner: Option<String>,
    mappings: Vec<CredentialMapping>,
) {
    let secret_names = mappings
        .iter()
        .map(|m| m.secret_name.clone())
        .collect::<HashSet<_>>();

    match owner {
        Some(owner) => replace_owned_mappings(guard, owner, mappings),
        None => replace_ownerless_mappings(guard, &secret_names, mappings),
    }
}

impl Default for SharedCredentialRegistry {
    fn default() -> Self {
        Self::new()
    }
}
