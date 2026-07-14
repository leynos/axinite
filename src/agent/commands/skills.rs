//! Skill listing and ClawHub search commands (/skills).

use crate::agent::Agent;
use crate::agent::submission::SubmissionResult;
use crate::error::Error;

/// Format a count with a suffix, using K/M abbreviations for large numbers.
fn format_count(n: u64, suffix: &str) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M {}", n as f64 / 1_000_000.0, suffix)
    } else if n >= 1_000 {
        format!("{:.1}K {}", n as f64 / 1_000.0, suffix)
    } else {
        format!("{} {}", n, suffix)
    }
}

/// Formats a single ClawHub catalog entry (with optional owner and stats)
/// for the search output.
fn format_catalog_entry(entry: &crate::skills::catalog::CatalogEntry) -> String {
    let owner_str = entry
        .owner
        .as_deref()
        .map(|o| format!("  by {}", o))
        .unwrap_or_default();

    let stats_parts: Vec<String> = [
        entry.stars.map(|s| format!("{} stars", s)),
        entry.downloads.map(|d| format_count(d, "downloads")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let stats_str = if stats_parts.is_empty() {
        String::new()
    } else {
        format!("  {}", stats_parts.join("  "))
    };

    let mut line = format!(
        "  {:<24} v{:<10}{}{}\n",
        entry.name, entry.version, owner_str, stats_str,
    );
    if !entry.description.is_empty() {
        line.push_str(&format!("    {}\n\n", entry.description));
    }
    line
}

/// Appends the ClawHub result section: entries when present, otherwise the
/// registry error or a "no results" note.
fn append_catalog_results(
    out: &mut String,
    entries: &[crate::skills::catalog::CatalogEntry],
    error: Option<&str>,
) {
    if entries.is_empty() {
        match error {
            Some(err) => out.push_str(&format!("  (registry error: {})\n", err)),
            None => out.push_str("  No results found.\n"),
        }
        return;
    }
    for entry in entries {
        out.push_str(&format_catalog_entry(entry));
    }
}

impl Agent {
    /// Appends installed skills whose name or description matches the query.
    fn append_installed_matches(&self, out: &mut String, query: &str) {
        let Some(registry) = self.skill_registry() else {
            return;
        };
        let Ok(guard) = registry.read() else {
            return;
        };
        let query_lower = query.to_lowercase();
        let matches: Vec<_> = guard
            .skills()
            .iter()
            .filter(|s| {
                s.manifest.name.to_lowercase().contains(&query_lower)
                    || s.manifest.description.to_lowercase().contains(&query_lower)
            })
            .collect();

        if matches.is_empty() {
            return;
        }
        out.push_str(&format!("Installed skills matching \"{}\":\n", query));
        for s in &matches {
            out.push_str(&format!(
                "  {:<24} v{:<10} [{}]\n",
                s.manifest.name, s.manifest.version, s.trust,
            ));
        }
    }

    /// List installed skills.
    pub(super) async fn handle_skills_list(&self) -> Result<SubmissionResult, Error> {
        let Some(registry) = self.skill_registry() else {
            return Ok(SubmissionResult::error("Skills system not enabled."));
        };

        let guard = match registry.read() {
            Ok(g) => g,
            Err(e) => {
                return Ok(SubmissionResult::error(format!(
                    "Skill registry lock error: {}",
                    e
                )));
            }
        };

        let skills = guard.skills();
        if skills.is_empty() {
            return Ok(SubmissionResult::response(
                "No skills installed.\n\nUse /skills search <query> to find skills on ClawHub.",
            ));
        }

        let mut out = String::from("Installed skills:\n\n");
        for s in skills {
            let desc = if s.manifest.description.chars().count() > 60 {
                let truncated: String = s.manifest.description.chars().take(57).collect();
                format!("{}...", truncated)
            } else {
                s.manifest.description.clone()
            };
            out.push_str(&format!(
                "  {:<24} v{:<10} [{}]  {}\n",
                s.manifest.name, s.manifest.version, s.trust, desc,
            ));
        }
        out.push_str("\nUse /skills search <query> to find more on ClawHub.");

        Ok(SubmissionResult::response(out))
    }

    /// Search ClawHub for skills.
    pub(super) async fn handle_skills_search(
        &self,
        query: &str,
    ) -> Result<SubmissionResult, Error> {
        let catalog = match self.skill_catalog() {
            Some(c) => c,
            None => {
                return Ok(SubmissionResult::error("Skill catalog not available."));
            }
        };

        let outcome = catalog.search(query).await;

        // Enrich top results with detail data (stars, downloads, owner)
        let mut entries = outcome.results;
        catalog.enrich_search_results(&mut entries, 5).await;

        let mut out = format!("ClawHub results for \"{}\":\n\n", query);
        append_catalog_results(&mut out, &entries, outcome.error.as_deref());

        // Show matching installed skills
        self.append_installed_matches(&mut out, query);

        Ok(SubmissionResult::response(out))
    }
}
