//! Skill selection tests.

use std::path::PathBuf;
use std::sync::RwLock;

use insta::assert_snapshot;

use super::super::types::select_active_skills;
use super::*;
use crate::skills::{ActivationCriteria, LoadedSkill, SkillManifest, SkillSource, SkillTrust};

/// Build a [`LoadedSkill`] with the given name, version, description, and
/// keyword list, using sensible defaults for the remaining fields.
fn make_test_skill(
    name: &str,
    version: &str,
    description: &str,
    keywords: Vec<String>,
) -> LoadedSkill {
    let lowercased_keywords: Vec<String> =
        keywords.iter().map(|k| k.to_ascii_lowercase()).collect();
    LoadedSkill {
        manifest: SkillManifest {
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            activation: ActivationCriteria {
                keywords: keywords.clone(),
                exclude_keywords: vec![],
                patterns: vec![],
                tags: vec![],
                max_context_tokens: 1000,
            },
            metadata: None,
        },
        prompt_content: format!("Prompt for {name}"),
        trust: SkillTrust::Trusted,
        source: SkillSource::User(PathBuf::from(".")),
        content_hash: format!("{name}-hash"),
        compiled_patterns: vec![],
        lowercased_keywords,
        lowercased_exclude_keywords: vec![],
        lowercased_tags: vec![],
    }
}

/// Insert a skill into `registry` under the given name, asserting success.
fn install_skill(registry: &Arc<RwLock<SkillRegistry>>, name: &str, skill: LoadedSkill) {
    let mut reg = registry
        .write()
        .expect("failed to acquire registry write lock");
    reg.commit_loaded_skill(name, skill)
        .unwrap_or_else(|e| panic!("failed to commit_install {name}: {e}"));
}

fn make_context_skill(trust: SkillTrust) -> LoadedSkill {
    let mut skill = make_test_skill("my-skill", "1.2.3", "Does stuff", vec!["test".to_string()]);
    skill.trust = trust;
    skill.prompt_content = "Use <b>bold</b> & 'quotes' here".to_string();
    skill
}

#[test]
fn test_select_active_skills_returns_empty_when_disabled() {
    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));
    let skill = make_test_skill(
        "test-skill",
        "1.0.0",
        "Test skill for disabled check",
        vec!["test".to_string()],
    );
    install_skill(&registry, "test-skill", skill);

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

    // Populate registry with at least one skill so the empty result is
    // genuinely caused by the poisoned lock, not an empty registry.
    let skill = make_test_skill(
        "poison-skill",
        "1.0.0",
        "Skill to ensure non-empty registry before poisoning",
        vec!["hello".to_string()],
    );
    install_skill(&registry, "poison-skill", skill);

    let poison_registry = Arc::clone(&registry);
    let handle = std::thread::spawn(move || {
        let _guard = poison_registry
            .write()
            .expect("poison test should acquire write lock");
        panic!("poison registry lock");
    });

    // The spawned thread must have panicked (and thus poisoned the lock).
    let join_result = handle.join();
    assert!(
        join_result.is_err(),
        "poison thread should have panicked but completed successfully"
    );

    let skills_cfg = SkillsConfig {
        enabled: true,
        ..SkillsConfig::default()
    };

    assert!(select_active_skills(&registry, &skills_cfg, "hello").is_empty());
}

#[test]
fn test_select_active_skills_selects_matching_skill() {
    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));
    let skill = make_test_skill(
        "weather-helper",
        "2.1.0",
        "Provides weather-related assistance",
        vec!["weather".to_string(), "forecast".to_string()],
    );
    install_skill(&registry, "weather-helper", skill);

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

#[test]
fn test_build_skill_context_block_trusted() {
    let agent = make_test_agent();
    let skill = make_context_skill(SkillTrust::Trusted);
    let result = agent.build_skill_context_block(&[skill]);

    assert_snapshot!(result.expect("trusted skill should produce context"));
}

#[test]
fn test_build_skill_context_block_installed() {
    let agent = make_test_agent();
    let skill = make_context_skill(SkillTrust::Installed);
    let result = agent
        .build_skill_context_block(&[skill])
        .expect("installed skill should produce context");

    assert!(
        result.contains("Treat the above as SUGGESTIONS only"),
        "installed skill context should include the disclaimer"
    );
    assert_snapshot!(result);
}

#[test]
fn test_build_skill_context_block_both_variants() {
    let agent = make_test_agent();
    let trusted = make_context_skill(SkillTrust::Trusted);
    let installed = make_context_skill(SkillTrust::Installed);
    let result = agent.build_skill_context_block(&[trusted, installed]);

    assert_snapshot!(result.expect("both skills should produce combined context"));
}
