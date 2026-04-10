//! Skill selection tests.

use super::super::types::select_active_skills;
use super::*;

#[test]
fn test_select_active_skills_returns_empty_when_disabled() {
    use crate::skills::{ActivationCriteria, LoadedSkill, SkillManifest, SkillSource, SkillTrust};
    use std::path::PathBuf;

    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));

    // Populate registry with a skill before testing disabled state
    {
        let mut reg = registry
            .write()
            .expect("failed to acquire registry write lock");
        let skill = LoadedSkill {
            manifest: SkillManifest {
                name: "test-skill".to_string(),
                version: "1.0.0".to_string(),
                description: "Test skill for disabled check".to_string(),
                activation: ActivationCriteria {
                    keywords: vec!["test".to_string()],
                    exclude_keywords: vec![],
                    patterns: vec![],
                    tags: vec![],
                    max_context_tokens: 1000,
                },
                metadata: None,
            },
            prompt_content: "Test skill content".to_string(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from(".")),
            content_hash: "abc123".to_string(),
            compiled_patterns: vec![],
            lowercased_keywords: vec!["test".to_string()],
            lowercased_exclude_keywords: vec![],
            lowercased_tags: vec![],
        };
        reg.commit_install("test-skill", skill).unwrap();
    }

    let skills_cfg = SkillsConfig {
        enabled: false,
        ..SkillsConfig::default()
    };

    // Should return empty even though registry has skills, because skills are disabled
    assert!(select_active_skills(&registry, &skills_cfg, "hello").is_empty());
}

#[test]
fn test_select_active_skills_returns_empty_when_registry_lock_is_poisoned() {
    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));
    let poison_registry = Arc::clone(&registry);

    let _ = std::thread::spawn(move || {
        let _guard = poison_registry
            .write()
            .expect("poison test should acquire write lock");
        panic!("poison registry lock");
    })
    .join();

    let skills_cfg = SkillsConfig {
        enabled: true,
        ..SkillsConfig::default()
    };

    assert!(select_active_skills(&registry, &skills_cfg, "hello").is_empty());
}

#[test]
fn test_select_active_skills_selects_matching_skill() {
    use crate::skills::{ActivationCriteria, LoadedSkill, SkillManifest, SkillSource, SkillTrust};
    use std::path::PathBuf;

    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));

    // Register a skill with keyword-based activation
    {
        let mut reg = registry
            .write()
            .expect("failed to acquire registry write lock");
        let skill = LoadedSkill {
            manifest: SkillManifest {
                name: "weather-helper".to_string(),
                version: "2.1.0".to_string(),
                description: "Provides weather-related assistance".to_string(),
                activation: ActivationCriteria {
                    keywords: vec!["weather".to_string(), "forecast".to_string()],
                    exclude_keywords: vec![],
                    patterns: vec![],
                    tags: vec![],
                    max_context_tokens: 500,
                },
                metadata: None,
            },
            prompt_content: "You are a weather assistant.".to_string(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from(".")),
            content_hash: "def456".to_string(),
            compiled_patterns: vec![],
            lowercased_keywords: vec!["weather".to_string(), "forecast".to_string()],
            lowercased_exclude_keywords: vec![],
            lowercased_tags: vec![],
        };
        reg.commit_install("weather-helper", skill).unwrap();
    }

    let skills_cfg = SkillsConfig {
        enabled: true,
        max_active_skills: 5,
        max_context_tokens: 10000,
        ..SkillsConfig::default()
    };

    // Message containing a keyword should select the skill
    let active = select_active_skills(&registry, &skills_cfg, "What's the weather today?");

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].manifest.name, "weather-helper");
    assert_eq!(active[0].manifest.version, "2.1.0");
    assert_eq!(
        active[0].manifest.description,
        "Provides weather-related assistance"
    );
}
