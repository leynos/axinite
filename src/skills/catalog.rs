//! Runtime skill catalog backed by ClawHub's public registry.
//!
//! Fetches skill listings from the ClawHub API (`/api/v1/search`) at runtime,
//! caching results in memory. No compile-time entries -- the catalog is always
//! up-to-date with the registry.
//!
//! Configuration:
//! - `CLAWHUB_REGISTRY` env var overrides the default base URL

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// Default ClawHub registry URL.
///
/// Points directly at the Convex backend, bypassing Vercel's edge which
/// rejects non-browser TLS fingerprints (JA3/JA4 filtering).
const DEFAULT_REGISTRY_URL: &str = "https://wry-manatee-359.convex.site";

/// How long cached search results remain valid (5 minutes).
const CACHE_TTL: Duration = Duration::from_secs(300);

/// Maximum number of results to return from a search.
const MAX_RESULTS: usize = 25;

/// HTTP request timeout for catalog queries.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

mod model;

#[cfg(test)]
mod tests;

pub use model::{CatalogEntry, CatalogSearchOutcome, SkillDetail, SkillOwner, SkillStats};
use model::{CatalogSearchEnvelope, CatalogSearchResult, SkillDetailResponse};

/// Cached search result with TTL.
struct CachedSearch {
    query: String,
    outcome: CatalogSearchOutcome,
    fetched_at: Instant,
}

/// Runtime skill catalog that queries ClawHub's API.
pub struct SkillCatalog {
    /// Base URL for the registry.
    registry_url: String,
    /// HTTP client (reused across requests).
    client: reqwest::Client,
    /// In-memory search cache keyed by query string.
    cache: RwLock<Vec<CachedSearch>>,
}

impl SkillCatalog {
    /// Create a new catalog.
    ///
    /// Reads `CLAWHUB_REGISTRY` (or legacy `CLAWDHUB_REGISTRY`) from the
    /// environment, falling back to the Convex backend.
    pub fn new() -> Self {
        let registry_url = std::env::var("CLAWHUB_REGISTRY")
            .or_else(|_| std::env::var("CLAWDHUB_REGISTRY"))
            .unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("ironclaw/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();

        Self {
            registry_url,
            client,
            cache: RwLock::new(Vec::new()),
        }
    }

    /// Create a catalog with a custom registry URL (for testing).
    #[cfg(test)]
    pub fn with_url(url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("ironclaw/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();

        Self {
            registry_url: url.to_string(),
            client,
            cache: RwLock::new(Vec::new()),
        }
    }

    /// Search for skills in the catalog.
    ///
    /// First checks the in-memory cache. If not cached or expired, fetches
    /// from the ClawHub API. Returns a [`CatalogSearchOutcome`] that carries
    /// both results and any error that occurred (catalog search is best-effort,
    /// never blocks the agent).
    pub async fn search(&self, query: &str) -> CatalogSearchOutcome {
        let query_lower = query.to_lowercase();

        // Check cache
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.iter().find(|c| c.query == query_lower)
                && cached.fetched_at.elapsed() < CACHE_TTL
            {
                return cached.outcome.clone();
            }
        }

        // Fetch from API
        let outcome = self.fetch_search(&query_lower).await;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            // Remove stale entry for this query
            cache.retain(|c| c.query != query_lower);
            // Limit cache size to prevent unbounded growth
            if cache.len() >= 50 {
                cache.remove(0);
            }
            cache.push(CachedSearch {
                query: query_lower,
                outcome: outcome.clone(),
                fetched_at: Instant::now(),
            });
        }

        outcome
    }

    /// Fetch search results from the ClawHub API.
    async fn fetch_search(&self, query: &str) -> CatalogSearchOutcome {
        let url = format!("{}/api/v1/search", self.registry_url);

        let response = match self.client.get(&url).query(&[("q", query)]).send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!("Catalog search failed (network): {}", e);
                return CatalogSearchOutcome {
                    results: Vec::new(),
                    error: Some("Registry unreachable".to_string()),
                };
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            tracing::debug!(
                "Catalog search returned status {}: {}",
                status,
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "(no body)".to_string())
            );
            return CatalogSearchOutcome {
                results: Vec::new(),
                error: Some(format!("Registry returned status {status}")),
            };
        }

        // Parse the response body as text first so we can try multiple formats.
        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!("Catalog search: failed to read response body: {}", e);
                return CatalogSearchOutcome {
                    results: Vec::new(),
                    error: Some("Failed to read registry response".to_string()),
                };
            }
        };

        // Try wrapped format first: {"results": [...]}
        // Then fall back to bare array: [...]
        let raw_results = if let Ok(envelope) = serde_json::from_str::<CatalogSearchEnvelope>(&body)
        {
            envelope.results
        } else if let Ok(arr) = serde_json::from_str::<Vec<CatalogSearchResult>>(&body) {
            arr
        } else {
            let preview = body.get(..200).unwrap_or(&body);
            tracing::debug!("Catalog search: failed to parse response: {}", preview);
            return CatalogSearchOutcome {
                results: Vec::new(),
                error: Some("Invalid response from registry".to_string()),
            };
        };

        CatalogSearchOutcome {
            results: raw_results
                .into_iter()
                .take(MAX_RESULTS)
                .map(|r| CatalogEntry {
                    slug: r.slug,
                    name: r.display_name.unwrap_or_default(),
                    description: r.summary.unwrap_or_default(),
                    version: r.version.unwrap_or_default(),
                    score: r.score.unwrap_or(0.0),
                    updated_at: r.updated_at,
                    stars: None,
                    downloads: None,
                    installs_current: None,
                    owner: None,
                })
                .collect(),
            error: None,
        }
    }

    /// Fetch detailed information for a single skill by slug.
    ///
    /// Calls `GET /api/v1/skills/{slug}` and returns the detail if available.
    /// Returns `None` on any network or parse error (best-effort).
    pub async fn fetch_skill_detail(&self, slug: &str) -> Option<SkillDetail> {
        let url = format!(
            "{}/api/v1/skills/{}",
            self.registry_url,
            urlencoding::encode(slug)
        );

        let response = self.client.get(&url).send().await.ok()?;
        if !response.status().is_success() {
            tracing::debug!(
                "Skill detail for '{}' returned status {}",
                slug,
                response.status()
            );
            return None;
        }

        let wrapper = response.json::<SkillDetailResponse>().await.ok()?;
        let inner = wrapper.skill;
        Some(SkillDetail {
            slug: inner.slug,
            display_name: inner.display_name,
            summary: inner.summary,
            version: None, // not returned in detail response
            stats: inner.stats,
            owner: wrapper.owner,
            updated_at: inner.updated_at,
        })
    }

    /// Enrich catalog entries with detail data (stars, downloads, owner).
    ///
    /// Fetches detail for up to `max` entries in parallel. Best-effort: entries
    /// that fail to enrich keep their `None` values.
    pub async fn enrich_search_results(&self, entries: &mut [CatalogEntry], max: usize) {
        let count = entries.len().min(max);
        if count == 0 {
            return;
        }

        let futures: Vec<_> = entries[..count]
            .iter()
            .map(|e| self.fetch_skill_detail(&e.slug))
            .collect();

        let details = futures::future::join_all(futures).await;

        for (entry, detail) in entries[..count].iter_mut().zip(details) {
            if let Some(detail) = detail {
                if let Some(ref stats) = detail.stats {
                    entry.stars = stats.stars;
                    entry.downloads = stats.downloads;
                    entry.installs_current = stats.installs_current;
                }
                if let Some(ref owner) = detail.owner {
                    entry.owner = owner.handle.clone().or_else(|| owner.display_name.clone());
                }
            }
        }
    }

    /// Get the registry base URL.
    pub fn registry_url(&self) -> &str {
        &self.registry_url
    }

    /// Clear the search cache.
    pub async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }
}

impl Default for SkillCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// Construct the download URL for a skill's SKILL.md from the registry.
///
/// The slug is URL-encoded to prevent query string injection via special
/// characters like `&` or `#`.
pub fn skill_download_url(registry_url: &str, slug: &str) -> String {
    format!(
        "{}/api/v1/download?slug={}",
        registry_url,
        urlencoding::encode(slug)
    )
}

/// Convenience wrapper for creating a shared catalog.
pub fn shared_catalog() -> Arc<SkillCatalog> {
    Arc::new(SkillCatalog::new())
}
