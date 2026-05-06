//! Shared test helpers for constructing [`LoadedSkill`] instances.

use std::path::PathBuf;

use crate::skills::{
    ActivationCriteria, LoadedSkill, LoadedSkillLocation, LoadedSkillParts, SkillManifest,
    SkillPackageKind, SkillSource, SkillTrust,
};

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

    pub fn build(self) -> LoadedSkill {
        let root = self.root;
        let location = self.location.unwrap_or_else(|| {
            LoadedSkillLocation::new(
                &self.name,
                root,
                PathBuf::from("SKILL.md"),
                SkillPackageKind::SingleFile,
            )
            .expect("test skill builder produces bundle-relative entrypoint")
        });
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
        .expect("test skill builder produced inconsistent location metadata")
    }
}
