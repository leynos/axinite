//! Immutable per-run capability tables.
//!
//! A run's selected tools are resolved once, before the helper starts, into a
//! name-sorted table. Every callback is admitted by numeric index into that
//! table, and admission is decided by the proved kernel in
//! [`monty_capabilities`]. The table never changes for the lifetime of a run:
//! a later registry change cannot widen what the guest may call.
//!
//! Tool identities are pinned here as [`Arc`] values. The broker re-checks
//! that the registry still resolves the same `Arc` before dispatch, so a tool
//! replaced after selection is refused rather than silently upgraded.

use std::sync::Arc;

use monty_capabilities::{MAX_SELECTED_TOOLS, admits, mask_from_indices, project};
use monty_protocol::ToolDefinition;
use serde_json::Value;

use crate::tools::{Tool, ToolRegistry, validate_tool_schema};

/// Guest-visible names the generated wrapper already uses.
///
/// A selected tool may not shadow any of them: the wrapper binds `main`,
/// `params` and `state`, and reserves the `__axinite_` prefix for generated
/// helpers. `exec_code` is reserved too: a run cannot select the tool that
/// would start another run, so recursion is refused at selection rather than
/// only at callback time.
pub const RESERVED_GUEST_NAMES: &[&str] = &[
    "main",
    "params",
    "state",
    "exec_code",
    "__axinite_call__",
    "__axinite_host__",
];

/// The prefix reserved for names the generated wrapper introduces.
pub const RESERVED_GUEST_PREFIX: &str = "__axinite_";

/// Why a capability table could not be built.
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    /// More tools were requested than the mask can address.
    #[error("cannot select {count} tools; the limit is {max}")]
    TooMany {
        /// Number requested.
        count: usize,
        /// The hard ceiling.
        max: usize,
    },
    /// The same name was requested twice.
    #[error("tool {name:?} is selected more than once")]
    Duplicate {
        /// The repeated name.
        name: String,
    },
    /// The registry does not contain a requested name.
    #[error("unknown tool {name:?}")]
    Unknown {
        /// The missing name.
        name: String,
    },
    /// The name is not usable as a guest function.
    #[error("{name:?} cannot be exposed to the guest: {reason}")]
    InvalidName {
        /// The rejected name.
        name: String,
        /// Why it is unusable.
        reason: String,
    },
    /// The tool's advertised schema is outside the supported subset.
    #[error("tool {name:?} has an unsupported parameter schema: {reason}")]
    UnsupportedSchema {
        /// The tool name.
        name: String,
        /// The first structural problem.
        reason: String,
    },
    /// The caller's own attenuated authority does not include the name.
    #[error("tool {name:?} is outside the caller's authority")]
    OutsideCallerAuthority {
        /// The name the caller's ceiling refused.
        name: String,
    },
}

/// One selected tool, pinned for a single run.
#[derive(Clone)]
pub struct ToolEntry {
    name: String,
    description: String,
    schema: Value,
    tool: Arc<dyn Tool>,
}

impl std::fmt::Debug for ToolEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolEntry")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl ToolEntry {
    /// Creates a pinned entry from an already-resolved tool.
    pub fn new(tool: Arc<dyn Tool>) -> Self {
        let name = tool.name().to_string();
        let description = tool.description().to_string();
        let schema = tool.parameters_schema();
        Self {
            name,
            description,
            schema,
            tool,
        }
    }

    /// The tool's canonical name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The pinned tool identity.
    pub fn tool(&self) -> &Arc<dyn Tool> {
        &self.tool
    }

    /// The validated parameter schema.
    pub fn schema(&self) -> &Value {
        &self.schema
    }
}

/// An immutable, name-sorted table of at most [`MAX_SELECTED_TOOLS`] tools.
#[derive(Clone)]
pub struct SelectedTools {
    entries: Vec<ToolEntry>,
    view: u64,
}

impl std::fmt::Debug for SelectedTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SelectedTools")
            .field("entries", &self.entries)
            .field("view", &self.view)
            .finish()
    }
}

impl SelectedTools {
    /// Builds a table from resolved entries.
    ///
    /// Entries are sorted by name, so an index is stable for a given set. The
    /// admitted view is `available ∩ requested ∩ validated`: every supplied
    /// entry is available and requested, and its bit survives only when its
    /// schema is inside the guest's supported subset.
    ///
    /// # Errors
    ///
    /// Returns [`SelectionError`] for an oversized, duplicated, reserved or
    /// unsupported entry.
    pub fn from_entries(mut entries: Vec<ToolEntry>) -> Result<Self, SelectionError> {
        check_count(entries.len())?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        reject_duplicates(&entries)?;
        let validated = validate_entries(&entries)?;

        let requested = mask_from_indices(0..entries.len());
        let available = requested;
        let view = project(available, requested, validated);
        Ok(Self { entries, view })
    }

    /// Resolves requested names against `registry`, then pins them.
    ///
    /// # Errors
    ///
    /// Returns [`SelectionError::Unknown`] for a name the registry does not
    /// contain, in addition to the errors from [`Self::from_entries`].
    pub async fn resolve(
        registry: &ToolRegistry,
        requested: &[String],
    ) -> Result<Self, SelectionError> {
        check_count(requested.len())?;
        let mut entries = Vec::with_capacity(requested.len());
        for name in requested {
            let tool = registry
                .get(name)
                .await
                .ok_or_else(|| SelectionError::Unknown { name: name.clone() })?;
            entries.push(ToolEntry::new(tool));
        }
        Self::from_entries(entries)
    }

    /// The selected entries, in index order.
    pub fn entries(&self) -> &[ToolEntry] {
        &self.entries
    }

    /// The admitted capability view.
    pub fn view(&self) -> u64 {
        self.view
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Finds an entry by exact name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries
            .binary_search_by(|entry| entry.name.as_str().cmp(name))
            .ok()
    }

    /// Whether the index is admitted by the run's view.
    pub fn admits(&self, index: usize) -> bool {
        u64::try_from(index).is_ok_and(|index| admits(self.view, index))
    }

    /// The wire definitions advertised to the helper.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.entries
            .iter()
            .map(|entry| ToolDefinition {
                name: entry.name.clone(),
                description: entry.description.clone(),
                parameters: entry.schema.clone(),
            })
            .collect()
    }
}

/// Rejects more requested tools than the mask can represent.
fn check_count(count: usize) -> Result<(), SelectionError> {
    if count > MAX_SELECTED_TOOLS {
        return Err(SelectionError::TooMany {
            count,
            max: MAX_SELECTED_TOOLS,
        });
    }
    Ok(())
}

/// Rejects duplicate names in the already name-sorted entry list.
fn reject_duplicates(entries: &[ToolEntry]) -> Result<(), SelectionError> {
    for pair in entries.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(SelectionError::Duplicate {
                name: pair[0].name.clone(),
            });
        }
    }
    Ok(())
}

/// Returns the bitmask of entries whose schemas the guest can enforce.
fn validate_entries(entries: &[ToolEntry]) -> Result<u64, SelectionError> {
    let mut validated = 0_u64;
    for (index, entry) in entries.iter().enumerate() {
        validate_entry(entry)?;
        validated |= 1_u64 << index;
    }
    Ok(validated)
}

/// Validates one entry against the host and guest schema subsets.
fn validate_entry(entry: &ToolEntry) -> Result<(), SelectionError> {
    check_guest_name(&entry.name)?;
    let host_errors = validate_tool_schema(&entry.schema, format!("parameters.{}", entry.name));
    if let Some(first) = host_errors.first() {
        return Err(SelectionError::UnsupportedSchema {
            name: entry.name.clone(),
            reason: first.clone(),
        });
    }
    monty_protocol::validate_schema(&entry.schema).map_err(|error| {
        SelectionError::UnsupportedSchema {
            name: entry.name.clone(),
            reason: format!("{}: {}", error.field, error.message),
        }
    })
}

fn check_guest_name(name: &str) -> Result<(), SelectionError> {
    if name.is_empty() {
        return Err(SelectionError::InvalidName {
            name: name.to_string(),
            reason: "name is empty".to_string(),
        });
    }
    if name.starts_with(RESERVED_GUEST_PREFIX) || RESERVED_GUEST_NAMES.contains(&name) {
        return Err(SelectionError::InvalidName {
            name: name.to_string(),
            reason: "name is reserved by the generated wrapper".to_string(),
        });
    }
    if !crate::tools::monty::is_python_identifier(name) {
        return Err(SelectionError::InvalidName {
            name: name.to_string(),
            reason: "name is not a valid Python identifier".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
