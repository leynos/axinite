//! Data models for the ClawHub skill catalog.
//!
//! Wire types for the ClawHub search and skill-detail APIs, plus the
//! public catalog entry and search-outcome types returned to callers.

use serde::{Deserialize, Serialize};

/// Result of a catalog search, carrying both results and any error that occurred.
#[derive(Debug, Clone)]
pub struct CatalogSearchOutcome {
    /// Skill entries returned by the search (empty on error).
    pub results: Vec<CatalogEntry>,
    /// If the registry was unreachable or returned an error, a human-readable message.
    pub error: Option<String>,
}

/// A skill entry from the ClawHub catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Skill slug (unique identifier, e.g. "owner/skill-name").
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Short description.
    #[serde(default)]
    pub description: String,
    /// Skill version (semver).
    #[serde(default)]
    pub version: String,
    /// Relevance score from the search API.
    #[serde(default)]
    pub score: f64,
    /// Last updated timestamp (epoch milliseconds from registry).
    #[serde(default)]
    pub updated_at: Option<u64>,
    /// Star count (populated via detail enrichment).
    #[serde(default)]
    pub stars: Option<u64>,
    /// Total download count (populated via detail enrichment).
    #[serde(default)]
    pub downloads: Option<u64>,
    /// Current install count (populated via detail enrichment).
    #[serde(default)]
    pub installs_current: Option<u64>,
    /// Owner handle (populated via detail enrichment).
    #[serde(default)]
    pub owner: Option<String>,
}

/// Top-level wrapper from the ClawHub `/api/v1/skills/{slug}` response.
///
/// The API returns `{"skill": {...}, "owner": {...}, "latestVersion": {...}}`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct SkillDetailResponse {
    pub(super) skill: SkillDetailInner,
    #[serde(default)]
    pub(super) owner: Option<SkillOwner>,
}

/// Inner `skill` object within `SkillDetailResponse`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SkillDetailInner {
    pub slug: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub stats: Option<SkillStats>,
    #[serde(default)]
    pub updated_at: Option<u64>,
}

/// Detailed skill information from the ClawHub `/api/v1/skills/{slug}` endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    /// Skill slug in `owner/skill-name` form.
    pub slug: String,
    /// Human-readable skill name, when the endpoint supplies one.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Short description of the skill, when present.
    #[serde(default)]
    pub summary: Option<String>,
    /// Version string, when the endpoint supplies one. `None` when absent.
    #[serde(default)]
    pub version: Option<String>,
    /// Popularity and install counters for the skill.
    #[serde(default)]
    pub stats: Option<SkillStats>,
    /// The publishing account.
    #[serde(default)]
    pub owner: Option<SkillOwner>,
    /// Last update time as Unix epoch milliseconds.
    #[serde(default)]
    pub updated_at: Option<u64>,
}

/// Statistics for a skill from the ClawHub detail endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStats {
    /// Number of stars the skill has received.
    #[serde(default)]
    pub stars: Option<u64>,
    /// Total number of downloads.
    #[serde(default)]
    pub downloads: Option<u64>,
    /// Current install count, as reported by ClawHub.
    #[serde(default)]
    pub installs_current: Option<u64>,
    /// Installs counted across the skill's whole history.
    #[serde(default)]
    pub installs_all_time: Option<u64>,
    /// Number of published versions.
    #[serde(default)]
    pub versions: Option<u64>,
}

/// Owner information for a skill.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillOwner {
    /// Account handle of the owner, the preferred identifier for display.
    #[serde(default)]
    pub handle: Option<String>,
    /// Owner's display name, wire key `displayName`.
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
}

/// Wrapper for ClawHub's `{"results": [...]}` envelope.
#[derive(Debug, Deserialize)]
pub(super) struct CatalogSearchEnvelope {
    pub(super) results: Vec<CatalogSearchResult>,
}

/// Internal type matching ClawHub's `/api/v1/search` response items.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogSearchResult {
    pub(super) slug: String,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) score: Option<f64>,
    #[serde(default)]
    pub(super) updated_at: Option<u64>,
}
