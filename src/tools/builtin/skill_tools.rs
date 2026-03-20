//! Agent-callable tools for managing skills (prompt-level extensions).
//!
//! Four tools for discovering, installing, listing, and removing skills
//! entirely through conversation, following the extension_tools pattern.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::skills::catalog::{CatalogEntry, SkillCatalog};
use crate::skills::registry::SkillRegistry;
use crate::tools::tool::{Tool, ToolError, ToolOutput, require_str};

mod install;
mod remove;
#[cfg(test)]
mod tests;

pub use install::SkillInstallTool;
pub use remove::SkillRemoveTool;

/// Build a minimal JSON Schema object descriptor.
///
/// `required` may be empty, in which case the key is omitted from the schema.
fn object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    let mut schema = serde_json::json!({
        "type": "object",
        "properties": properties,
    });
    if !required.is_empty() {
        schema["required"] = serde_json::json!(required);
    }
    schema
}

// ── skill_list ──────────────────────────────────────────────────────────

pub struct SkillListTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

impl SkillListTool {
    pub fn new(registry: Arc<std::sync::RwLock<SkillRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        "skill_list"
    }

    fn description(&self) -> &str {
        "List all loaded skills with their trust level, source, and activation keywords."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "verbose": {
                    "type": "boolean",
                    "description": "Include extra detail (tags, content_hash, version)",
                    "default": false
                }
            }),
            &[],
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let verbose = params
            .get("verbose")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let guard = self
            .registry
            .read()
            .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;

        let skills: Vec<serde_json::Value> = guard
            .skills()
            .iter()
            .map(|s| {
                let mut entry = serde_json::json!({
                    "name": s.manifest.name,
                    "description": s.manifest.description,
                    "trust": s.trust.to_string(),
                    "source": format!("{:?}", s.source),
                    "keywords": s.manifest.activation.keywords,
                });

                if verbose && let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "version".to_string(),
                        serde_json::Value::String(s.manifest.version.clone()),
                    );
                    obj.insert(
                        "tags".to_string(),
                        serde_json::json!(s.manifest.activation.tags),
                    );
                    obj.insert(
                        "content_hash".to_string(),
                        serde_json::Value::String(s.content_hash.clone()),
                    );
                    obj.insert(
                        "max_context_tokens".to_string(),
                        serde_json::json!(s.manifest.activation.max_context_tokens),
                    );
                }

                entry
            })
            .collect();

        let output = serde_json::json!({
            "skills": skills,
            "count": skills.len(),
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}

// ── skill_search ────────────────────────────────────────────────────────

pub struct SkillSearchTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
    catalog: Arc<SkillCatalog>,
}

impl SkillSearchTool {
    pub fn new(
        registry: Arc<std::sync::RwLock<SkillRegistry>>,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }

    fn parse_search_query(params: &serde_json::Value) -> Result<String, ToolError> {
        Ok(require_str(params, "query")?.to_string())
    }

    async fn compute_installed_index(
        registry: &Arc<std::sync::RwLock<SkillRegistry>>,
    ) -> Result<HashSet<String>, ToolError> {
        let guard = registry
            .read()
            .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
        Ok(guard
            .skills()
            .iter()
            .map(|s| s.manifest.name.clone())
            .collect())
    }

    fn is_entry_installed(installed: &HashSet<String>, name: &str, slug_opt: Option<&str>) -> bool {
        installed.contains(name)
            || slug_opt.is_some_and(|slug| {
                slug.rsplit('/')
                    .next()
                    .is_some_and(|segment| installed.contains(segment))
            })
    }

    fn format_search_results(
        entries: Vec<CatalogEntry>,
        installed: &HashSet<String>,
    ) -> Result<serde_json::Value, ToolError> {
        Ok(serde_json::Value::Array(
            entries
                .into_iter()
                .map(|entry| {
                    let is_installed =
                        Self::is_entry_installed(installed, &entry.name, Some(&entry.slug));
                    serde_json::json!({
                        "slug": entry.slug,
                        "name": entry.name,
                        "description": entry.description,
                        "version": entry.version,
                        "score": entry.score,
                        "installed": is_installed,
                        "stars": entry.stars,
                        "downloads": entry.downloads,
                        "owner": entry.owner,
                    })
                })
                .collect(),
        ))
    }

    fn collect_local_matches(&self, query: &str) -> Result<Vec<serde_json::Value>, ToolError> {
        let query_lower = query.to_lowercase();
        let guard = self
            .registry
            .read()
            .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
        Ok(guard
            .skills()
            .iter()
            .filter(|s| {
                s.manifest.name.to_lowercase().contains(&query_lower)
                    || s.manifest.description.to_lowercase().contains(&query_lower)
                    || s.manifest
                        .activation
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query_lower))
            })
            .map(|s| {
                serde_json::json!({
                    "name": s.manifest.name,
                    "description": s.manifest.description,
                    "trust": s.trust.to_string(),
                })
            })
            .collect())
    }
}

#[async_trait]
impl Tool for SkillSearchTool {
    fn name(&self) -> &str {
        "skill_search"
    }

    fn description(&self) -> &str {
        "Search for skills in the ClawHub catalog and among locally loaded skills."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "Search query (name, keyword, or description fragment)"
                }
            }),
            &["query"],
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let query = Self::parse_search_query(&params)?;

        let catalog_outcome = self.catalog.search(&query).await;
        let catalog_error = catalog_outcome.error.clone();

        let mut catalog_entries = catalog_outcome.results;
        self.catalog
            .enrich_search_results(&mut catalog_entries, 5)
            .await;

        let installed = Self::compute_installed_index(&self.registry).await?;
        let catalog_json = Self::format_search_results(catalog_entries, &installed)?;
        let local_matches = self.collect_local_matches(&query)?;

        let mut output = serde_json::json!({
            "catalog": catalog_json,
            "catalog_count": catalog_json.as_array().map_or(0, Vec::len),
            "installed": local_matches,
            "installed_count": local_matches.len(),
            "registry_url": self.catalog.registry_url(),
        });
        if let Some(err) = catalog_error {
            output["catalog_error"] = serde_json::Value::String(err);
        }

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}
