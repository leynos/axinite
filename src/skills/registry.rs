//! Skill registry for discovering, loading, and managing available skills.
//!
//! Skills are discovered from two filesystem locations:
//! 1. Workspace skills directory (`<workspace>/skills/`) -- Trusted
//! 2. User skills directory (`~/.ironclaw/skills/`) -- Trusted
//!
//! Both flat (`skills/SKILL.md`) and subdirectory (`skills/<name>/SKILL.md`)
//! layouts are supported. Earlier locations win on name collision (workspace
//! overrides user). Uses async I/O throughout to avoid blocking the tokio runtime.

mod discovery;
mod loading;
mod materialize;
mod removal;
mod staged_install;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub use loading::{check_gating, compute_hash};
pub use staged_install::{PreparedSkillInstall, SkillInstallPayload};

use crate::skills::bundle::SkillBundleError;
use crate::skills::{LoadedSkill, SkillSource, SkillTrust};

/// Maximum number of skills that can be discovered from a single directory.
/// Prevents resource exhaustion from a directory with thousands of entries.
const MAX_DISCOVERED_SKILLS: usize = 100;

fn to_lowercase_vec(items: &[String]) -> Vec<String> {
    items.iter().map(|s| s.to_lowercase()).collect()
}

/// Error type for skill registry operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillRegistryError {
    #[error("Skill not found: {0}")]
    NotFound(String),

    #[error("Failed to read skill file {path}: {reason}")]
    ReadError { path: String, reason: String },

    #[error("Failed to parse SKILL.md for '{name}': {reason}")]
    ParseError { name: String, reason: String },

    #[error("Skill file too large for '{name}': {size} bytes (max {max} bytes)")]
    FileTooLarge { name: String, size: u64, max: u64 },

    #[error("Symlink detected in skills directory: {path}")]
    SymlinkDetected { path: String },

    #[error("Skill '{name}' failed gating: {reason}")]
    GatingFailed { name: String, reason: String },

    #[error(
        "Skill '{name}' prompt exceeds token budget: ~{approx_tokens} tokens but declares max_context_tokens={declared}"
    )]
    TokenBudgetExceeded {
        name: String,
        approx_tokens: usize,
        declared: usize,
    },

    #[error("Skill '{name}' already exists")]
    AlreadyExists { name: String },

    #[error("Cannot remove skill '{name}': {reason}")]
    CannotRemove { name: String, reason: String },

    #[error("Failed to write skill file {path}: {reason}")]
    WriteError { path: String, reason: String },

    #[error("{0}")]
    InvalidBundle(#[from] SkillBundleError),

    #[error("Invalid skill content: {reason}")]
    InvalidContent { reason: String },
}

/// Registry of available skills.
pub struct SkillRegistry {
    /// All loaded skills.
    skills: Vec<LoadedSkill>,
    /// User skills directory (~/.ironclaw/skills/). Skills here are Trusted.
    user_dir: PathBuf,
    /// Registry-installed skills directory (~/.ironclaw/installed_skills/). Skills here are Installed.
    installed_dir: Option<PathBuf>,
    /// Optional workspace skills directory.
    workspace_dir: Option<PathBuf>,
}

impl SkillRegistry {
    /// Create a new skill registry.
    pub fn new(user_dir: PathBuf) -> Self {
        Self {
            skills: Vec::new(),
            user_dir,
            installed_dir: None,
            workspace_dir: None,
        }
    }

    /// Set the registry-installed skills directory.
    ///
    /// Skills installed via ClawHub or the skill tools are written here and
    /// loaded with `SkillTrust::Installed` (read-only tool access). This
    /// directory is separate from the user dir so that trust levels survive
    /// restarts correctly.
    pub fn with_installed_dir(mut self, dir: PathBuf) -> Self {
        self.installed_dir = Some(dir);
        self
    }

    /// Set a workspace skills directory.
    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    /// Discover and load skills from all configured directories.
    ///
    /// Discovery order (earlier wins on name collision):
    /// 1. Workspace skills directory (if set) -- Trusted
    /// 2. User skills directory -- Trusted
    /// 3. Installed skills directory (if set) -- Installed
    pub async fn discover_all(&mut self) -> Vec<String> {
        let mut loaded_names: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // 1. Workspace skills (highest priority)
        if let Some(ws_dir) = self.workspace_dir.clone() {
            let ws_skills =
                discovery::discover_from_dir(&ws_dir, SkillTrust::Trusted, SkillSource::Workspace)
                    .await;
            for (name, skill) in ws_skills {
                if seen.contains(&name) {
                    continue;
                }
                seen.insert(name.clone());
                loaded_names.push(name);
                self.skills.push(skill);
            }
        }

        // 2. User skills
        let user_dir = self.user_dir.clone();
        let user_skills =
            discovery::discover_from_dir(&user_dir, SkillTrust::Trusted, SkillSource::User).await;
        for (name, skill) in user_skills {
            if seen.contains(&name) {
                tracing::debug!("Skipping user skill '{}' (overridden by workspace)", name);
                continue;
            }
            seen.insert(name.clone());
            loaded_names.push(name);
            self.skills.push(skill);
        }

        // 3. Installed skills (registry-installed, lowest priority)
        if let Some(inst_dir) = self.installed_dir.clone() {
            let inst_skills =
                discovery::discover_from_dir(&inst_dir, SkillTrust::Installed, SkillSource::User)
                    .await;
            for (name, skill) in inst_skills {
                if seen.contains(&name) {
                    tracing::debug!(
                        "Skipping installed skill '{}' (overridden by user/workspace)",
                        name
                    );
                    continue;
                }
                seen.insert(name.clone());
                loaded_names.push(name);
                self.skills.push(skill);
            }
        }

        loaded_names
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[LoadedSkill] {
        &self.skills
    }

    /// Get the number of loaded skills.
    pub fn count(&self) -> usize {
        self.skills.len()
    }

    /// Retain only skills whose names are in the given allowlist.
    ///
    /// If `names` is empty, this is a no-op (all skills are kept).
    pub fn retain_only(&mut self, names: &[&str]) {
        if names.is_empty() {
            return;
        }
        let names_set: HashSet<&str> = names.iter().copied().collect();
        self.skills
            .retain(|s| names_set.contains(s.manifest.name.as_str()));
    }

    /// Check if a skill with the given name is loaded.
    pub fn has(&self, name: &str) -> bool {
        self.skills.iter().any(|s| s.manifest.name == name)
    }

    /// Find a skill by name.
    pub fn find_by_name(&self, name: &str) -> Option<&LoadedSkill> {
        self.skills.iter().find(|s| s.manifest.name == name)
    }

    /// Perform the disk I/O and loading for a skill install.
    ///
    /// This is a static method so it doesn't borrow `&self`, allowing callers
    /// to drop their registry lock before awaiting.
    pub async fn prepare_install_to_disk(
        install_root: &Path,
        payload: SkillInstallPayload,
    ) -> Result<PreparedSkillInstall, SkillRegistryError> {
        staged_install::prepare_install_to_disk(install_root, payload).await
    }

    /// Finalize a previously prepared install.
    ///
    /// This moves the prepared install from `prepared.staged_dir` to
    /// `prepared.final_dir` with a same-filesystem rename, then inserts the
    /// prevalidated `prepared.loaded_skill` into the in-memory registry.
    ///
    /// Call this only after [`SkillRegistry::prepare_install_to_disk`] has
    /// returned successfully. On success, the caller must treat
    /// `prepared.staged_dir` as consumed. On failure, the staged directory is
    /// left in place so the caller can decide whether to inspect it or roll it
    /// back with [`SkillRegistry::cleanup_prepared_install`].
    ///
    /// The function borrows `prepared` rather than consuming it, so callers are
    /// still responsible for cleanup on failure. This keeps the registry lock
    /// held only for duplicate checks, the final rename, and the in-memory
    /// insert.
    pub fn commit_install(
        &mut self,
        prepared: &PreparedSkillInstall,
    ) -> Result<(), SkillRegistryError> {
        staged_install::commit_install(self, prepared)
    }
    /// Insert an already loaded skill into the registry without filesystem I/O.
    ///
    /// This is the lower-level helper used by [`Self::commit_install`] after
    /// the staged directory has been renamed into place. Callers are
    /// responsible for ensuring any on-disk lifecycle work has already
    /// completed before using this helper.
    pub fn commit_loaded_skill(
        &mut self,
        name: &str,
        skill: LoadedSkill,
    ) -> Result<(), SkillRegistryError> {
        if self.has(name) {
            return Err(SkillRegistryError::AlreadyExists {
                name: name.to_string(),
            });
        }

        self.skills.push(skill);
        tracing::info!("Installed skill: {}", name);
        Ok(())
    }
    /// Remove the staged directory for a prepared install.
    ///
    /// Use this to roll back a [`PreparedSkillInstall`] that will not be
    /// committed. The function is idempotent with respect to missing
    /// directories: if `prepared.staged_dir` is already gone, cleanup succeeds.
    ///
    /// This does not touch `prepared.final_dir` or mutate the in-memory
    /// registry. Callers should continue to return or log their original
    /// install failure if cleanup itself also errors, and should call this
    /// after `prepare_install_to_disk` whenever `commit_install` is not going
    /// to succeed.
    pub async fn cleanup_prepared_install(
        prepared: &PreparedSkillInstall,
    ) -> Result<(), SkillRegistryError> {
        staged_install::cleanup_prepared_install(prepared).await
    }

    /// Install a skill at runtime from SKILL.md content.
    ///
    /// Convenience method that parses, writes to disk, and commits in-memory.
    /// When called through tool execution where a lock is involved, prefer using
    /// `prepare_install_to_disk` + `commit_install` separately to minimize lock
    /// hold time.
    pub async fn install_skill(&mut self, content: &str) -> Result<String, SkillRegistryError> {
        let install_dir = self.install_target_dir().to_path_buf();
        let prepared = Self::prepare_install_to_disk(
            &install_dir,
            SkillInstallPayload::Markdown(content.into()),
        )
        .await?;

        match self.commit_install(&prepared) {
            Ok(()) => Ok(prepared.name().to_string()),
            Err(commit_error) => {
                if let Err(cleanup_error) = Self::cleanup_prepared_install(&prepared).await {
                    tracing::warn!(
                        "failed to cleanup prepared skill install '{}': {}",
                        prepared.name(),
                        cleanup_error
                    );
                }

                Err(commit_error)
            }
        }
    }

    /// Validate that a skill can be removed and return its filesystem path.
    ///
    /// Performs validation without modifying state. Callers can then do async
    /// filesystem cleanup without holding the registry lock, and call
    /// `commit_remove` afterward.
    pub fn validate_remove(&self, name: &str) -> Result<PathBuf, SkillRegistryError> {
        removal::validate_remove(self, name)
    }

    /// Remove a skill's files from disk (async I/O).
    ///
    /// Call after `validate_remove` and before `commit_remove`.
    pub async fn delete_skill_files(path: &Path) -> Result<(), SkillRegistryError> {
        removal::delete_skill_files(path).await
    }

    /// Remove a skill from the in-memory registry.
    ///
    /// Fast synchronous operation. Call after filesystem cleanup.
    pub fn commit_remove(&mut self, name: &str) -> Result<(), SkillRegistryError> {
        removal::commit_remove(self, name)
    }

    /// Remove a skill by name.
    ///
    /// Convenience method that combines validation, file deletion, and in-memory
    /// removal. When called through tool execution, prefer using the split
    /// validate/delete/commit methods to minimize lock hold time.
    pub async fn remove_skill(&mut self, name: &str) -> Result<(), SkillRegistryError> {
        let path = self.validate_remove(name)?;
        Self::delete_skill_files(&path).await?;
        self.commit_remove(name)
    }

    /// Clear all loaded skills and re-discover from disk.
    pub async fn reload(&mut self) -> Vec<String> {
        self.skills.clear();
        self.discover_all().await
    }

    /// Get the user skills directory path.
    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    /// Get the installed skills directory path, if configured.
    pub fn installed_dir(&self) -> Option<&Path> {
        self.installed_dir.as_deref()
    }

    /// Get the directory where new registry installs should be written.
    ///
    /// Returns the installed_dir if configured (preferred), otherwise falls
    /// back to user_dir. In practice, the installed_dir is always set when
    /// the app is running; the fallback exists for test registries.
    pub fn install_target_dir(&self) -> &Path {
        self.installed_dir.as_deref().unwrap_or(&self.user_dir)
    }
}
