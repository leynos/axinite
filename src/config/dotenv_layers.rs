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
pub(super) static DOTENV_LAYERS: LazyLock<DotenvLayerStore> =
    LazyLock::new(DotenvLayerStore::empty);

/// Inputs needed to reload dotenv files at an explicit composition boundary.
pub(crate) struct DotenvRefreshInputs<'a> {
    pub(crate) start_dir: &'a Path,
    pub(crate) process: &'a HashMap<String, String>,
    pub(crate) cached_base_dir: Option<&'a Path>,
    pub(crate) home_dir_fallback: Option<&'a Path>,
}

/// Publishes immutable dotenv snapshots while readers retain stable versions.
///
/// Once a valid snapshot exists, a failed read or parse does not replace it.
/// The parser still retains entries before the first error for initial loads.
pub(crate) struct DotenvLayerStore {
    layers: RwLock<PublishedDotenvLayers>,
}

struct PublishedDotenvLayers {
    layers: Arc<DotenvLayers>,
    has_valid_snapshot: bool,
}

impl DotenvLayerStore {
    /// Start with an empty snapshot.
    pub(crate) fn empty() -> Self {
        Self {
            layers: RwLock::new(PublishedDotenvLayers {
                layers: Arc::new(DotenvLayers::empty()),
                has_valid_snapshot: false,
            }),
        }
    }

    /// Load files from explicit inputs and publish a fresh immutable snapshot.
    pub(crate) fn refresh(&self, inputs: DotenvRefreshInputs<'_>) -> Arc<DotenvLayers> {
        let layers = match inputs.cached_base_dir {
            Some(base_dir) => DotenvLayers::load_with_base_dir(
                inputs.start_dir,
                inputs.process,
                Some(base_dir),
                inputs.home_dir_fallback,
            ),
            None => DotenvLayers::load(inputs.start_dir, inputs.process, inputs.home_dir_fallback),
        };
        let layers = Arc::new(layers);
        let mut published = self
            .layers
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if layers.has_load_errors && published.has_valid_snapshot {
            tracing::warn!(
                "Rejected dotenv refresh after a file error; retaining the last valid snapshot"
            );
            return Arc::clone(&published.layers);
        }
        if !layers.has_load_errors {
            published.has_valid_snapshot = true;
        }
        published.layers = Arc::clone(&layers);
        layers
    }

    /// Get the current immutable snapshot.
    pub(crate) fn snapshot(&self) -> Arc<DotenvLayers> {
        self.layers
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layers
            .clone()
    }
}

/// Parsed current-directory and bootstrap environment files.
#[derive(Clone, Debug, Default)]
pub(crate) struct DotenvLayers {
    cwd: HashMap<String, String>,
    bootstrap: HashMap<String, String>,
    base_dir: PathBuf,
    bootstrap_path: PathBuf,
    has_load_errors: bool,
}

#[derive(Default)]
struct ParsedEnvFile {
    values: HashMap<String, String>,
    has_load_errors: bool,
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
        let base_dir = cached_base_dir.map(Path::to_path_buf).unwrap_or_else(|| {
            resolve_base_dir(start_dir, process, &cwd.values, home_dir_fallback)
        });
        let bootstrap_path = base_dir.join(".env");

        if !path_exists(&bootstrap_path) {
            migrate_bootstrap_json_to_env(&bootstrap_path);
        }

        let bootstrap = read_env_file(&bootstrap_path);
        let has_load_errors = cwd.has_load_errors || bootstrap.has_load_errors;
        Self {
            cwd: cwd.values,
            bootstrap: bootstrap.values,
            base_dir,
            bootstrap_path,
            has_load_errors,
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
        let cwd = cwd
            .map(|bytes| parse_bytes(bytes, "in-memory current-directory .env"))
            .unwrap_or_default();
        let bootstrap = bootstrap
            .map(|bytes| parse_bytes(bytes, "in-memory bootstrap .env"))
            .unwrap_or_default();
        Self {
            has_load_errors: cwd.has_load_errors || bootstrap.has_load_errors,
            cwd: cwd.values,
            bootstrap: bootstrap.values,
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

fn read_env_file(path: &Path) -> ParsedEnvFile {
    match ambient_fs::read(path) {
        Ok(bytes) => parse_bytes(&bytes, &path.display().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ParsedEnvFile::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "Failed to read dotenv file");
            ParsedEnvFile {
                has_load_errors: true,
                ..ParsedEnvFile::default()
            }
        }
    }
}

fn parse_bytes(bytes: &[u8], source: &str) -> ParsedEnvFile {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut parsed = ParsedEnvFile::default();

    for item in dotenvy::from_read_iter(Cursor::new(bytes)) {
        match item {
            Ok((key, value)) => {
                parsed
                    .values
                    .entry(canonical_env_key(&key))
                    .or_insert(value);
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
                parsed.has_load_errors = true;
                break;
            }
        }
    }

    parsed
}

fn canonical_env_key(key: &str) -> String {
    #[cfg(windows)]
    {
        key.to_ascii_uppercase()
    }
    #[cfg(not(windows))]
    {
        key.to_string()
    }
}

fn resolve_base_dir(
    start_dir: &Path,
    process: &HashMap<String, String>,
    cwd: &HashMap<String, String>,
    home_dir_fallback: Option<&Path>,
) -> PathBuf {
    let configured = effective_non_empty_value(process, cwd, AXINITE_BASE_DIR_ENV);
    if let Some(configured) = configured {
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
    effective_non_empty_value(process, cwd, "HOME")
        .or_else(|| effective_non_empty_value(process, cwd, "USERPROFILE"))
        .map(PathBuf::from)
        .or_else(|| {
            effective_non_empty_value(process, cwd, "HOMEDRIVE")
                .zip(effective_non_empty_value(process, cwd, "HOMEPATH"))
                .map(|(drive, path)| PathBuf::from(format!("{drive}{path}")))
        })
        .or_else(|| home_dir_fallback.map(PathBuf::from))
        .unwrap_or_else(|| start_dir.to_path_buf())
        .join(".axinite")
}

fn effective_non_empty_value<'a>(
    process: &'a HashMap<String, String>,
    cwd: &'a HashMap<String, String>,
    key: &str,
) -> Option<&'a String> {
    lookup_env_value(process, key)
        .filter(|value| !value.is_empty())
        .or_else(|| lookup_env_value(cwd, key).filter(|value| !value.is_empty()))
}

fn lookup_env_value<'a>(map: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
    let value = map.get(key);
    #[cfg(windows)]
    {
        value.or_else(|| {
            map.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value)
        })
    }
    #[cfg(not(windows))]
    {
        value
    }
}

#[cfg(test)]
#[path = "dotenv_layers/tests.rs"]
mod tests;
