//! Installing pgvector into the embedded cluster's tree.
//!
//! Theseus builds PostgreSQL with its in-tree contrib modules, and pgvector is
//! out of tree, so the server arrives without it. The archive is prebuilt, so
//! nothing is compiled here and the no-source-builds rule holds.

use pg_embedded_setup_unpriv::ClusterHandle;

use super::TOKEN_GUIDANCE;
use crate::error::DatabaseError;

/// Points `postgresql_extensions` at the cluster's own installation.
///
/// The extension crate derives the library and share directories by running
/// `pg_config` from the binary directory, so this only has to say where the
/// binaries are. The connection fields are unused for an install but the trait
/// requires them.
struct ExtensionTarget {
    binary_dir: std::path::PathBuf,
}

impl postgresql_commands::Settings for ExtensionTarget {
    fn get_binary_dir(&self) -> std::path::PathBuf {
        self.binary_dir.clone()
    }
    fn get_host(&self) -> std::ffi::OsString {
        "localhost".into()
    }
    fn get_port(&self) -> u16 {
        0
    }
    fn get_username(&self) -> std::ffi::OsString {
        "postgres".into()
    }
    fn get_password(&self) -> std::ffi::OsString {
        String::new().into()
    }
}

/// Installs pgvector into the cluster's tree if it is not already there.
///
/// Theseus builds PostgreSQL with its in-tree contrib modules, and pgvector is
/// out of tree, so the server arrives without it. The archive is prebuilt, so
/// nothing is compiled here and the estate's no-source-builds rule holds.
///
/// Idempotent: the extension crate records what it installed and skips a second
/// install, so every test can call this and only the first does work.
///
/// # Errors
/// Returns [`DatabaseError::Pool`] carrying [`TOKEN_GUIDANCE`] when the archive
/// cannot be fetched, because the overwhelmingly likely cause is the GitHub API
/// rate limit and the underlying error says only `403 Forbidden`.
pub(super) async fn install_extension(cluster: &ClusterHandle) -> Result<(), DatabaseError> {
    let target = ExtensionTarget {
        binary_dir: cluster.settings().binary_dir(),
    };
    let installed = postgresql_extensions::get_installed_extensions(&target)
        .await
        .unwrap_or_default();
    if installed
        .iter()
        .any(|extension| extension.name() == "pgvector_compiled")
    {
        return Ok(());
    }
    postgresql_extensions::install(
        &target,
        "portal-corp",
        "pgvector_compiled",
        &semver::VersionReq::STAR,
    )
    .await
    .map_err(|error| DatabaseError::Pool(format!("{TOKEN_GUIDANCE}\n\nUnderlying error: {error}")))
}
