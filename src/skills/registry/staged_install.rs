//! Staged install helpers for registry-managed skill installation.

use std::path::{Path, PathBuf};

use super::loading::load_and_validate_skill;
use super::materialize::{materialize_install_artifact, write_install_artifact};
use super::{LoadedSkill, SkillRegistry, SkillRegistryError, SkillSource, SkillTrust};
use uuid::Uuid;

/// Input payload for a staged skill install.
///
/// `SkillRegistry::prepare_install_to_disk` accepts one of these payloads,
/// materializes it into a staged install tree, and validates the staged
/// `SKILL.md` before returning a [`PreparedSkillInstall`].
///
/// Use [`Self::Markdown`] when the caller already has raw `SKILL.md` text for
/// a single-file skill install. Use [`Self::DownloadedBytes`] when the payload
/// came from a download and may represent either plain `SKILL.md` content or a
/// validated `.skill` bundle archive. The enum owns the staged payload bytes:
/// [`Self::Markdown`] stores the source text as an owned `String`, while
/// [`Self::DownloadedBytes`] stores the fetched bytes in an owned `Vec<u8>`.
pub enum SkillInstallPayload {
    /// Install from literal `SKILL.md` text.
    Markdown(String),
    /// Install from downloaded bytes, which may be markdown or a `.skill`
    /// archive.
    DownloadedBytes(Vec<u8>),
}

/// A failed staged install commit that preserves the prepared filesystem
/// state.
///
/// [`SkillRegistry::commit_install`] returns this when the final duplicate
/// check or rename step fails after preparation has already produced a
/// [`PreparedSkillInstall`]. The original [`SkillRegistryError`] is preserved
/// in `error`, and `prepared` is returned so callers can inspect or roll back
/// the staged directory with [`SkillRegistry::cleanup_prepared_install`]
/// without reconstructing install state.
#[derive(Debug)]
pub struct CommitPreparedInstallError {
    pub(super) error: SkillRegistryError,
    pub(super) prepared: Box<PreparedSkillInstall>,
}

impl CommitPreparedInstallError {
    /// Borrow the original registry error that caused the commit to fail.
    pub fn error(&self) -> &SkillRegistryError {
        &self.error
    }

    /// Borrow the prepared install state that remained staged after failure.
    pub fn prepared(&self) -> &PreparedSkillInstall {
        &self.prepared
    }

    /// Split the failure into its original error and prepared install state.
    pub fn into_parts(self) -> (SkillRegistryError, PreparedSkillInstall) {
        (self.error, *self.prepared)
    }
}

impl std::fmt::Display for CommitPreparedInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for CommitPreparedInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Prepared, validated install state that has not yet been committed.
///
/// A value of this type means the install payload has already been written into
/// `staged_dir`, the final destination has been reserved as `final_dir`, and
/// the staged `SKILL.md` has passed parsing, gating, and runtime validation
/// into `loaded_skill`. `name` is the validated manifest name that callers use
/// for duplicate checks and user-facing responses. The install is not visible
/// to normal skill discovery until [`SkillRegistry::commit_install`] renames
/// `staged_dir` into `final_dir` and inserts `loaded_skill` into the in-memory
/// registry.
///
/// Callers that abort after preparation must use
/// [`SkillRegistry::cleanup_prepared_install`] to remove the staged tree. The
/// lifecycle is `prepare_install_to_disk` followed by either
/// [`SkillRegistry::commit_install`] on success or
/// [`SkillRegistry::cleanup_prepared_install`] on failure.
#[derive(Debug)]
pub struct PreparedSkillInstall {
    pub(super) name: String,
    pub(super) staged_dir: PathBuf,
    pub(super) final_dir: PathBuf,
    pub(super) loaded_skill: LoadedSkill,
}

impl PreparedSkillInstall {
    /// Return the validated skill name that will be installed on commit.
    ///
    /// This matches the parsed manifest name and the final install directory
    /// name under the registry install root. The accessor borrows the prepared
    /// state, so callers remain responsible for later calling
    /// [`SkillRegistry::commit_install`] or
    /// [`SkillRegistry::cleanup_prepared_install`].
    pub fn name(&self) -> &str {
        &self.name
    }
}

async fn cleanup_staged_dir(staged_dir: &Path) {
    if let Err(cleanup_error) = tokio::fs::remove_dir_all(staged_dir).await
        && cleanup_error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            "failed to cleanup invalid staged skill install '{}': {}",
            staged_dir.display(),
            cleanup_error
        );
    }
}

pub(super) async fn prepare_install_to_disk(
    install_root: &Path,
    payload: SkillInstallPayload,
) -> Result<PreparedSkillInstall, SkillRegistryError> {
    tokio::fs::create_dir_all(install_root)
        .await
        .map_err(|e| SkillRegistryError::WriteError {
            path: install_root.display().to_string(),
            reason: e.to_string(),
        })?;

    let install_artifact = materialize_install_artifact(payload)?;
    let staged_dir = install_root.join(format!(".skill-install-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&staged_dir)
        .await
        .map_err(|e| SkillRegistryError::WriteError {
            path: staged_dir.display().to_string(),
            reason: e.to_string(),
        })?;

    let final_dir = install_root.join(&install_artifact.install_dir_name);
    if let Err(error) = write_install_artifact(&staged_dir, &install_artifact).await {
        cleanup_staged_dir(&staged_dir).await;
        return Err(error);
    }

    let skill_path = staged_dir.join("SKILL.md");
    let source = SkillSource::User(final_dir.clone());
    let (name, loaded_skill) =
        match load_and_validate_skill(&skill_path, SkillTrust::Installed, source).await {
            Ok(result) => result,
            Err(error) => {
                cleanup_staged_dir(&staged_dir).await;
                return Err(error);
            }
        };

    Ok(PreparedSkillInstall {
        name,
        staged_dir,
        final_dir,
        loaded_skill,
    })
}

pub(super) fn commit_install(
    registry: &mut SkillRegistry,
    prepared: PreparedSkillInstall,
) -> Result<(), CommitPreparedInstallError> {
    if registry.has(prepared.name()) || prepared.final_dir.exists() {
        return Err(CommitPreparedInstallError {
            error: SkillRegistryError::AlreadyExists {
                name: prepared.name().to_string(),
            },
            prepared: Box::new(prepared),
        });
    }

    let PreparedSkillInstall {
        name,
        staged_dir,
        final_dir,
        loaded_skill,
    } = prepared;

    if let Err(error) = std::fs::rename(&staged_dir, &final_dir) {
        return Err(CommitPreparedInstallError {
            error: SkillRegistryError::WriteError {
                path: final_dir.display().to_string(),
                reason: error.to_string(),
            },
            prepared: Box::new(PreparedSkillInstall {
                name,
                staged_dir,
                final_dir,
                loaded_skill,
            }),
        });
    }

    registry.skills.push(loaded_skill);
    tracing::info!("Installed skill: {}", name);
    Ok(())
}

pub(super) async fn cleanup_prepared_install(
    prepared: &PreparedSkillInstall,
) -> Result<(), SkillRegistryError> {
    match tokio::fs::remove_dir_all(&prepared.staged_dir).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SkillRegistryError::WriteError {
            path: prepared.staged_dir.display().to_string(),
            reason: e.to_string(),
        }),
    }
}
