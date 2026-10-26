//! Non-mutating parsing and precedence for Axinite `.env` files.
//!
//! Keep the `dotenvy` dependency behind this module: iterator parsing retains
//! its format support without calling the mutating `Iter::load` methods.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};

use crate::bootstrap::migrate_bootstrap_json_to_env;

const AXINITE_BASE_DIR_ENV: &str = "AXINITE_BASE_DIR";

/// Replaceable, immutable snapshots of parsed `.env` layers.
pub(super) static DOTENV_LAYERS: LazyLock<RwLock<Arc<DotenvLayers>>> =
    LazyLock::new(|| RwLock::new(Arc::new(DotenvLayers::empty())));

/// Parsed current-directory and bootstrap environment files.
#[derive(Clone, Debug, Default)]
pub(crate) struct DotenvLayers {
    cwd: HashMap<String, String>,
    bootstrap: HashMap<String, String>,
    base_dir: PathBuf,
    bootstrap_path: PathBuf,
}

impl DotenvLayers {
    /// Construct an empty layer set for the initial process snapshot.
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Load the nearest ancestor `.env` file and the selected bootstrap file.
    pub(crate) fn load(
        start_dir: &Path,
        process: &HashMap<String, String>,
        home_dir_fallback: Option<&Path>,
    ) -> Self {
        Self::load_with_base_dir(start_dir, process, None, home_dir_fallback)
    }

    /// Load layers while keeping a base directory that was already cached.
    pub(crate) fn load_with_base_dir(
        start_dir: &Path,
        process: &HashMap<String, String>,
        cached_base_dir: Option<&Path>,
        home_dir_fallback: Option<&Path>,
    ) -> Self {
        let cwd_path = find_ancestor_env_file(start_dir);
        let cwd = cwd_path.as_deref().map(read_env_file).unwrap_or_default();
        let base_dir = cached_base_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| resolve_base_dir(start_dir, process, &cwd, home_dir_fallback));
        let bootstrap_path = base_dir.join(".env");

        if !path_exists(&bootstrap_path) {
            migrate_bootstrap_json_to_env(&bootstrap_path);
        }

        let bootstrap = read_env_file(&bootstrap_path);
        Self {
            cwd,
            bootstrap,
            base_dir,
            bootstrap_path,
        }
    }

    /// Parse in-memory file contents against an explicit base directory.
    ///
    /// This constructor keeps precedence tests independent of the real
    /// environment, working directory, and filesystem.
    #[cfg(test)]
    pub(crate) fn from_contents(
        cwd: Option<&[u8]>,
        bootstrap: Option<&[u8]>,
        base_dir: &Path,
    ) -> Self {
        let bootstrap_path = base_dir.join(".env");
        Self {
            cwd: cwd
                .map(|bytes| parse_bytes(bytes, "in-memory current-directory .env"))
                .unwrap_or_default(),
            bootstrap: bootstrap
                .map(|bytes| parse_bytes(bytes, "in-memory bootstrap .env"))
                .unwrap_or_default(),
            base_dir: base_dir.to_path_buf(),
            bootstrap_path,
        }
    }

    pub(crate) fn cwd_values(&self) -> &HashMap<String, String> {
        &self.cwd
    }

    pub(crate) fn bootstrap_values(&self) -> &HashMap<String, String> {
        &self.bootstrap
    }

    pub(crate) fn base_dir(&self) -> Option<&Path> {
        (!self.base_dir.as_os_str().is_empty()).then_some(self.base_dir.as_path())
    }

    pub(crate) fn bootstrap_path(&self) -> Option<&Path> {
        (!self.bootstrap_path.as_os_str().is_empty()).then_some(self.bootstrap_path.as_path())
    }
}

fn find_ancestor_env_file(start_dir: &Path) -> Option<PathBuf> {
    let mut directory = Some(start_dir);
    while let Some(current) = directory {
        let candidate = current.join(".env");
        match ambient_fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Some(candidate),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                path = %candidate.display(),
                %error,
                "Failed to inspect current-directory dotenv file"
            ),
        }
        directory = current.parent();
    }
    None
}

fn path_exists(path: &Path) -> bool {
    match ambient_fs::try_exists(path) {
        Ok(exists) => exists,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "Failed to inspect bootstrap dotenv path"
            );
            true
        }
    }
}

fn read_env_file(path: &Path) -> HashMap<String, String> {
    match ambient_fs::read(path) {
        Ok(bytes) => parse_bytes(&bytes, &path.display().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "Failed to read dotenv file");
            HashMap::new()
        }
    }
}

fn parse_bytes(bytes: &[u8], source: &str) -> HashMap<String, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut values = HashMap::new();

    for item in dotenvy::from_read_iter(Cursor::new(bytes)) {
        match item {
            Ok((key, value)) => {
                values.entry(key).or_insert(value);
            }
            Err(error) => {
                let (error_kind, error_index) = match &error {
                    dotenvy::Error::LineParse(_, index) => ("line parse error", Some(*index)),
                    dotenvy::Error::Io(_) => ("I/O error", None),
                    dotenvy::Error::EnvVar(_) => ("environment lookup error", None),
                    _ => ("unknown dotenv error", None),
                };
                tracing::warn!(
                    %source,
                    error_kind,
                    error_index = ?error_index,
                    "Stopped parsing dotenv file after an error"
                );
                break;
            }
        }
    }

    values
}

fn resolve_base_dir(
    start_dir: &Path,
    process: &HashMap<String, String>,
    cwd: &HashMap<String, String>,
    home_dir_fallback: Option<&Path>,
) -> PathBuf {
    let configured = process
        .get(AXINITE_BASE_DIR_ENV)
        .or_else(|| cwd.get(AXINITE_BASE_DIR_ENV));
    if let Some(configured) = configured
        && !configured.is_empty()
    {
        let path = PathBuf::from(configured);
        return if path.is_absolute() {
            path
        } else {
            start_dir.join(path)
        };
    }

    default_base_dir(start_dir, process, cwd, home_dir_fallback)
}

fn default_base_dir(
    start_dir: &Path,
    process: &HashMap<String, String>,
    cwd: &HashMap<String, String>,
    home_dir_fallback: Option<&Path>,
) -> PathBuf {
    effective_value(process, cwd, "HOME")
        .or_else(|| effective_value(process, cwd, "USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            effective_value(process, cwd, "HOMEDRIVE")
                .zip(effective_value(process, cwd, "HOMEPATH"))
                .map(|(drive, path)| PathBuf::from(format!("{drive}{path}")))
        })
        .or_else(|| home_dir_fallback.map(PathBuf::from))
        .unwrap_or_else(|| start_dir.to_path_buf())
        .join(".axinite")
}

fn effective_value<'a>(
    process: &'a HashMap<String, String>,
    cwd: &'a HashMap<String, String>,
    key: &str,
) -> Option<&'a String> {
    process.get(key).or_else(|| cwd.get(key))
}

#[cfg(test)]
#[path = "dotenv_layers/tests.rs"]
mod tests;
