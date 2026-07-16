//! Shared test helpers for constructing [`LoadedSkill`] instances and skill
//! bundle archives.

use std::io::Write;
use std::path::PathBuf;

use crate::skills::registry::{SkillInstallPayload, SkillRegistry};
use crate::skills::{
    ActivationCriteria, LoadedSkill, LoadedSkillLocation, LoadedSkillParts, SkillManifest,
    SkillPackageKind, SkillSource, SkillTrust,
};

/// Installed bundle state returned by [`installed_bundle_fixture`].
///
/// The fixture owns the temporary user and installed-skill directories so tests
/// can read files through the same registry and runtime paths used by real
/// installed bundles.  This mirrors the repository-level test support pattern
/// of returning owned setup state rather than hiding lifetimes in globals.
pub struct InstalledBundleFixture {
    /// Temporary user skill directory used as the registry's trusted source.
    pub _user_dir: tempfile::TempDir,
    /// Temporary installed-skill directory that receives the staged bundle.
    pub _installed_dir: tempfile::TempDir,
    /// Registry containing the committed bundle install.
    #[cfg(target_os = "linux")]
    pub registry: SkillRegistry,
    /// Loaded skill discovered after the bundle install is committed.
    pub loaded_skill: LoadedSkill,
}

/// File entry used by bundle archive builders in tests.
///
/// Use [`BundleArchiveEntry::file`] for ordinary bundle files and
/// [`BundleArchiveEntry::file_with_mode`] when a test needs to exercise ZIP
/// permission metadata.  Archive construction helpers consume these entries to
/// keep bundle layout setup close to the regression that uses it.
pub struct BundleArchiveEntry {
    name: String,
    data: Vec<u8>,
    unix_mode: Option<u32>,
}

impl BundleArchiveEntry {
    /// Build an archive entry with default ZIP file permissions.
    ///
    /// `name` is the bundle-relative path written into the archive and `data`
    /// is copied into the entry body.  Use this for the common
    /// `SKILL.md`, `references/`, and `assets/` fixtures.
    pub fn file(name: impl AsRef<str>, data: &[u8]) -> Self {
        Self {
            name: name.as_ref().to_string(),
            data: data.to_vec(),
            unix_mode: None,
        }
    }

    /// Build an archive entry with explicit Unix permission bits.
    ///
    /// `name` is the bundle-relative path, `data` is copied into the entry
    /// body, and `unix_mode` is written as ZIP Unix metadata.  This supports
    /// adapter-boundary tests that need malformed or unusual archive metadata.
    pub fn file_with_mode(name: impl AsRef<str>, data: &[u8], unix_mode: u32) -> Self {
        Self {
            name: name.as_ref().to_string(),
            data: data.to_vec(),
            unix_mode: Some(unix_mode),
        }
    }
}

/// Build a `.skill` archive from borrowed test entries.
///
/// Each tuple contains the bundle-relative file name and byte content.  This
/// convenience wrapper is used by deterministic install tests; property tests
/// that already own generated strings should prefer
/// [`build_bundle_archive_from_owned`] to avoid cloning the generated data.
pub fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, zip::result::ZipError> {
    let entries = entries
        .iter()
        .map(|(name, data)| BundleArchiveEntry::file(*name, data))
        .collect::<Vec<_>>();
    build_bundle_archive_from_entries(&entries)
}

/// Build a `.skill` archive from owned generated entries.
///
/// Each tuple contains the bundle-relative file name and byte content.  This
/// helper lets property tests transfer generated cases directly into archive
/// construction without retaining an extra copy.
pub fn build_bundle_archive_from_owned(
    entries: Vec<(String, Vec<u8>)>,
) -> Result<Vec<u8>, zip::result::ZipError> {
    let entries = entries
        .into_iter()
        .map(|(name, data)| BundleArchiveEntry::file(name, &data))
        .collect::<Vec<_>>();
    build_bundle_archive_from_entries(&entries)
}

/// Build a `.skill` archive from fully described archive entries.
///
/// Use this lower-level helper when a test needs entry metadata such as Unix
/// permissions.  The archive bytes can be passed directly to
/// [`SkillInstallPayload::ArchiveBytes`] or uploaded through channel adapter
/// tests.
pub fn build_bundle_archive_from_entries(
    entries: &[BundleArchiveEntry],
) -> Result<Vec<u8>, zip::result::ZipError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in entries {
        let mut entry_options = options;
        if let Some(unix_mode) = entry.unix_mode {
            entry_options = entry_options.unix_permissions(unix_mode);
        }
        writer.start_file(&entry.name, entry_options)?;
        writer.write_all(&entry.data)?;
    }

    Ok(writer.finish()?.into_inner())
}

/// Install a bundle archive into an isolated temporary registry.
///
/// `entries` are bundle-relative file paths and byte contents passed through
/// [`build_bundle_archive`].  The returned fixture owns the temporary
/// directories, registry, and loaded skill so tests can exercise the same
/// install-to-read path used by runtime skill file access.  Setup failures are
/// returned to the calling test instead of panicking inside shared test support.
pub async fn installed_bundle_fixture(
    entries: &[(&str, &[u8])],
) -> Result<InstalledBundleFixture, Box<dyn std::error::Error>> {
    let user_dir = tempfile::tempdir()?;
    let installed_dir = tempfile::tempdir()?;
    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_dir.path().to_path_buf());
    let archive = build_bundle_archive(entries)?;

    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::ArchiveBytes(archive),
    )
    .await?;
    let name = prepared.name().to_string();
    registry.commit_install(prepared)?;
    let loaded_skill = registry
        .find_by_name(&name)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("installed fixture skill `{name}` should be loaded"),
            )
        })?
        .clone();

    Ok(InstalledBundleFixture {
        _user_dir: user_dir,
        _installed_dir: installed_dir,
        #[cfg(target_os = "linux")]
        registry,
        loaded_skill,
    })
}

pub struct TestSkillBuilder {
    name: String,
    version: String,
    description: String,
    trust: SkillTrust,
    source: SkillSource,
    root: PathBuf,
    location: Option<LoadedSkillLocation>,
    keywords: Vec<String>,
    exclude_keywords: Vec<String>,
    tags: Vec<String>,
    patterns: Vec<String>,
    max_context_tokens: usize,
    prompt_content: String,
    content_hash: String,
}

impl TestSkillBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            name,
            version: "1.0.0".to_string(),
            description: String::new(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from("/tmp")),
            root: PathBuf::from("/tmp"),
            location: None,
            keywords: vec![],
            exclude_keywords: vec![],
            tags: vec![],
            patterns: vec![],
            max_context_tokens: 1000,
            prompt_content: "Test prompt".to_string(),
            content_hash: "sha256:000".to_string(),
        }
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn trust(mut self, trust: SkillTrust) -> Self {
        self.trust = trust;
        self
    }

    pub fn source(mut self, source: SkillSource) -> Self {
        self.source = source;
        self
    }

    /// Override the default runtime root used when no explicit
    /// [`Self::location`] is supplied.  Defaults to `/tmp`.
    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self
    }

    pub fn location(mut self, location: LoadedSkillLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub fn keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords = keywords.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn exclude_keywords(mut self, exclude_keywords: &[&str]) -> Self {
        self.exclude_keywords = exclude_keywords.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn patterns(mut self, patterns: &[&str]) -> Self {
        self.patterns = patterns.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn max_context_tokens(mut self, max_context_tokens: usize) -> Self {
        self.max_context_tokens = max_context_tokens;
        self
    }

    pub fn prompt_content(mut self, prompt_content: impl Into<String>) -> Self {
        self.prompt_content = prompt_content.into();
        self
    }

    pub fn content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = content_hash.into();
        self
    }

    /// Build the [`LoadedSkill`] described by this builder.
    ///
    /// # Errors
    ///
    /// Returns an error when the default location cannot be constructed or
    /// when the assembled skill parts are inconsistent.
    pub fn build(self) -> anyhow::Result<LoadedSkill> {
        use anyhow::Context as _;

        let root = self.root;
        let location = match self.location {
            Some(location) => location,
            None => LoadedSkillLocation::new(
                &self.name,
                root,
                PathBuf::from("SKILL.md"),
                SkillPackageKind::SingleFile,
            )
            .context("test skill builder produces bundle-relative entrypoint")?,
        };
        let compiled = LoadedSkill::compile_patterns(&self.patterns);
        let lowercased_keywords = self.keywords.iter().map(|k| k.to_lowercase()).collect();
        let lowercased_exclude_keywords = self
            .exclude_keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();
        let lowercased_tags = self.tags.iter().map(|t| t.to_lowercase()).collect();
        LoadedSkill::new(LoadedSkillParts {
            manifest: SkillManifest {
                name: self.name,
                version: self.version,
                description: self.description,
                activation: ActivationCriteria {
                    keywords: self.keywords,
                    exclude_keywords: self.exclude_keywords,
                    tags: self.tags,
                    patterns: self.patterns,
                    max_context_tokens: self.max_context_tokens,
                },
                metadata: None,
            },
            prompt_content: self.prompt_content,
            trust: self.trust,
            source: self.source,
            location,
            content_hash: self.content_hash,
            compiled_patterns: compiled,
            lowercased_keywords,
            lowercased_exclude_keywords,
            lowercased_tags,
        })
        .context("test skill builder produced inconsistent location metadata")
    }
}
