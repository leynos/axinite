//! Curated in-memory catalog of known extensions with fuzzy search.
//!
//! The registry holds well-known channels, tools, and MCP servers that can be
//! installed via conversational commands. Online discoveries are cached here too.

use tokio::sync::RwLock;

use crate::extensions::{ExtensionKind, RegistryEntry, ResultSource, SearchResult};

/// Curated extension registry with fuzzy search.
pub struct ExtensionRegistry {
    /// Built-in curated entries.
    entries: Vec<RegistryEntry>,
    /// Cached entries from online discovery (session-lived).
    discovery_cache: RwLock<Vec<RegistryEntry>>,
}

impl ExtensionRegistry {
    /// Create a new registry populated with known extensions.
    pub fn new() -> Self {
        Self {
            entries: builtin_entries(),
            discovery_cache: RwLock::new(Vec::new()),
        }
    }

    /// Create a new registry merging builtin entries with catalog-provided entries.
    ///
    /// Deduplicates by `(name, kind)` pair -- a builtin MCP "slack" and a registry
    /// WASM "slack" can coexist since they're different kinds.
    pub fn new_with_catalog(catalog_entries: Vec<RegistryEntry>) -> Self {
        let mut entries = builtin_entries();
        for entry in catalog_entries {
            if !entries
                .iter()
                .any(|e| e.name == entry.name && e.kind == entry.kind)
            {
                entries.push(entry);
            }
        }
        Self {
            entries,
            discovery_cache: RwLock::new(Vec::new()),
        }
    }

    /// Search the registry by query string. Returns results sorted by relevance.
    ///
    /// Splits the query into lowercase tokens and scores each entry by matches
    /// in name, keywords, and description.
    pub async fn search(&self, query: &str) -> Vec<SearchResult> {
        let tokens: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        if tokens.is_empty() {
            // Return all entries when query is empty
            return self
                .entries
                .iter()
                .map(|e| SearchResult {
                    entry: e.clone(),
                    source: ResultSource::Registry,
                    validated: true,
                })
                .collect();
        }

        let mut scored: Vec<(SearchResult, u32)> = Vec::new();
        collect_scored(
            self.entries.as_slice(),
            ResultSource::Registry,
            &tokens,
            &mut scored,
        );

        let cache = self.discovery_cache.read().await;
        collect_scored(
            cache.as_slice(),
            ResultSource::Discovered,
            &tokens,
            &mut scored,
        );

        scored.sort_by_key(|b| std::cmp::Reverse(b.1));
        scored.into_iter().map(|(r, _)| r).collect()
    }

    /// Look up an entry by exact name.
    ///
    /// NOTE: Prefer [`get_with_kind`] when a kind hint is available, to avoid
    /// returning the wrong entry when two entries share a name but differ in kind.
    pub async fn get(&self, name: &str) -> Option<RegistryEntry> {
        if let Some(entry) = self.entries.iter().find(|e| e.name == name) {
            return Some(entry.clone());
        }
        let cache = self.discovery_cache.read().await;
        cache.iter().find(|e| e.name == name).cloned()
    }

    /// Look up an entry by exact name, filtering by kind when provided.
    ///
    /// When `kind` is `Some(...)`, only returns an entry matching both name and
    /// kind — never falls back to a different kind. When `kind` is `None`,
    /// returns the first name match (same as [`get`]).
    pub async fn get_with_kind(
        &self,
        name: &str,
        kind: Option<ExtensionKind>,
    ) -> Option<RegistryEntry> {
        if let Some(kind) = kind {
            if let Some(entry) = self
                .entries
                .iter()
                .find(|e| e.name == name && e.kind == kind)
            {
                return Some(entry.clone());
            }
            let cache = self.discovery_cache.read().await;
            if let Some(entry) = cache.iter().find(|e| e.name == name && e.kind == kind) {
                return Some(entry.clone());
            }
            // Kind was specified but no entry matches — don't fall back to a
            // different kind, as that would silently misroute the install.
            return None;
        }
        self.get(name).await
    }

    /// Return all registry entries (builtins + cached discoveries).
    pub async fn all_entries(&self) -> Vec<RegistryEntry> {
        let mut entries = self.entries.clone();
        let cache = self.discovery_cache.read().await;
        for entry in cache.iter() {
            if !entries
                .iter()
                .any(|e| e.name == entry.name && e.kind == entry.kind)
            {
                entries.push(entry.clone());
            }
        }
        entries
    }

    /// Add discovered entries to the cache.
    pub async fn cache_discovered(&self, entries: Vec<RegistryEntry>) {
        let mut cache = self.discovery_cache.write().await;
        for entry in entries {
            // Deduplicate by (name, kind) — same pair as new_with_catalog()
            if !cache
                .iter()
                .any(|e| e.name == entry.name && e.kind == entry.kind)
            {
                cache.push(entry);
            }
        }
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Score every entry in `entries` against `tokens`, appending the matches
/// (score > 0) to `scored` tagged with their `source`.
fn collect_scored(
    entries: &[RegistryEntry],
    source: ResultSource,
    tokens: &[String],
    scored: &mut Vec<(SearchResult, u32)>,
) {
    for entry in entries {
        let score = score_entry(entry, tokens);
        if score > 0 {
            scored.push((
                SearchResult {
                    entry: entry.clone(),
                    source,
                    validated: true,
                },
                score,
            ));
        }
    }
}

/// Score an entry against search tokens. Higher = better match.
fn score_entry(entry: &RegistryEntry, tokens: &[String]) -> u32 {
    let mut score = 0u32;
    let name_lower = entry.name.to_lowercase();
    let display_lower = entry.display_name.to_lowercase();
    let desc_lower = entry.description.to_lowercase();
    let keywords_lower: Vec<String> = entry.keywords.iter().map(|k| k.to_lowercase()).collect();

    for token in tokens {
        // Exact name match is the strongest signal
        if name_lower == *token {
            score += 100;
        } else if name_lower.contains(token.as_str()) {
            score += 50;
        }

        // Display name match
        if display_lower.contains(token.as_str()) {
            score += 30;
        }

        // Keyword match
        for kw in &keywords_lower {
            if kw == token {
                score += 40;
            } else if kw.contains(token.as_str()) {
                score += 20;
            }
        }

        // Description match (weakest signal)
        if desc_lower.contains(token.as_str()) {
            score += 10;
        }
    }

    score
}

mod builtin;

pub use builtin::{builtin_entries, builtin_entries_with_relay};

#[cfg(test)]
mod tests;
