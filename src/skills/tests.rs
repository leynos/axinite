//! Unit tests for the [`skills`](crate::skills) module.
//!
//! Split out of `mod.rs` to keep the parent module under the 400-line
//! file-size guideline.  See `escape.rs` for the escaping helpers that were
//! factored out alongside this module.

use super::*;
use rstest::{fixture, rstest};

#[test]
fn test_skill_trust_ordering() {
    assert!(SkillTrust::Installed < SkillTrust::Trusted);
}

#[test]
fn test_skill_trust_display() {
    assert_eq!(SkillTrust::Installed.to_string(), "installed");
    assert_eq!(SkillTrust::Trusted.to_string(), "trusted");
}

#[rstest]
#[case::hyphenated("writing-assistant", true)]
#[case::underscored("my_skill", true)]
#[case::dotted("skill.v2", true)]
#[case::single_char("a", true)]
#[case::alphanumeric("ABC123", true)]
#[case::empty("", false)]
#[case::leading_dash("-starts-with-dash", false)]
#[case::leading_dot(".starts-with-dot", false)]
#[case::spaces("has spaces", false)]
#[case::slashes("has/slashes", false)]
#[case::angle_brackets("has<angle>brackets", false)]
#[case::quotes("has\"quotes", false)]
#[case::too_long(
    "very-long-name-that-exceeds-the-sixty-four-character-limit-for-skill-names-wow",
    false
)]
fn test_validate_skill_name(#[case] name: &str, #[case] expected: bool) {
    assert_eq!(validate_skill_name(name), expected);
}

#[rstest]
#[case::plain("normal", "normal")]
#[case::double_quote(r#"" trust="LOCAL"#, "&quot; trust=&quot;LOCAL")]
#[case::angle_brackets("<script>", "&lt;script&gt;")]
#[case::ampersand("a&b", "a&amp;b")]
fn test_escape_xml_attr(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(escape_xml_attr(input), expected);
}

#[rstest]
#[case::plain("normal text", "normal text")]
#[case::closing_lower("</skill>breakout", "&lt;/skill>breakout")]
#[case::closing_upper("</SKILL>UPPER", "&lt;/SKILL>UPPER")]
#[case::closing_mixed("</sKiLl>mixed", "&lt;/sKiLl>mixed")]
#[case::closing_space("</ skill>space", "&lt;/ skill>space")]
#[case::closing_null("</\x00skill>null", "&lt;/\x00skill>null")]
#[case::opening_with_content(
    "<skill name=\"x\" trust=\"TRUSTED\">injected</skill>",
    "&lt;skill name=\"x\" trust=\"TRUSTED\">injected&lt;/skill>"
)]
#[case::opening_upper("<SKILL>upper", "&lt;SKILL>upper")]
#[case::opening_space("< skill>space", "&lt; skill>space")]
fn test_escape_skill_content(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(escape_skill_content(input), expected);
}

#[rstest]
#[case::crlf("a\r\nb\r\n", "a\nb\n")]
#[case::cr_only("a\rb\r", "a\nb\n")]
#[case::lf_only("a\nb\n", "a\nb\n")]
fn test_normalize_line_endings(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(normalize_line_endings(input), expected);
}

#[test]
fn test_enforce_keyword_limits() {
    let mut criteria = ActivationCriteria {
        keywords: (0..30).map(|i| format!("kw{}", i)).collect(),
        patterns: (0..10).map(|i| format!("pat{}", i)).collect(),
        tags: (0..20).map(|i| format!("tag{}", i)).collect(),
        ..Default::default()
    };
    criteria.enforce_limits();
    assert_eq!(criteria.keywords.len(), MAX_KEYWORDS_PER_SKILL);
    assert_eq!(criteria.patterns.len(), MAX_PATTERNS_PER_SKILL);
    assert_eq!(criteria.tags.len(), MAX_TAGS_PER_SKILL);
}

#[test]
fn test_enforce_limits_filters_short_keywords() {
    let mut criteria = ActivationCriteria {
        keywords: vec!["a".into(), "be".into(), "cat".into(), "dog".into()],
        tags: vec!["x".into(), "foo".into(), "ab".into(), "bar".into()],
        ..Default::default()
    };
    criteria.enforce_limits();
    assert_eq!(criteria.keywords, vec!["cat", "dog"]);
    assert_eq!(criteria.tags, vec!["foo", "bar"]);
}

#[fixture]
fn limits_enforced_criteria() -> ActivationCriteria {
    let mut keywords: Vec<String> = vec!["a".into(), "bb".into()];
    keywords.extend((0..25).map(|i| format!("keyword{}", i)));

    let patterns: Vec<String> = (0..8).map(|i| format!("pattern{}", i)).collect();

    let mut tags: Vec<String> = vec!["x".into(), "ab".into()];
    tags.extend((0..15).map(|i| format!("tag{}", i)));

    let mut criteria = ActivationCriteria {
        keywords,
        patterns,
        tags,
        ..Default::default()
    };

    criteria.enforce_limits();
    criteria
}

#[rstest]
fn test_enforce_limits_keywords(limits_enforced_criteria: ActivationCriteria) {
    let criteria = limits_enforced_criteria;
    assert!(
        !criteria
            .keywords
            .iter()
            .any(|k| k.len() < MIN_KEYWORD_TAG_LENGTH),
        "keywords shorter than {} chars should be filtered out",
        MIN_KEYWORD_TAG_LENGTH
    );
    assert_eq!(
        criteria.keywords.len(),
        MAX_KEYWORDS_PER_SKILL,
        "keywords should be capped at {}",
        MAX_KEYWORDS_PER_SKILL
    );
}

#[rstest]
fn test_enforce_limits_patterns(limits_enforced_criteria: ActivationCriteria) {
    let criteria = limits_enforced_criteria;
    assert_eq!(
        criteria.patterns.len(),
        MAX_PATTERNS_PER_SKILL,
        "patterns should be capped at {}",
        MAX_PATTERNS_PER_SKILL
    );
    let expected: Vec<String> = (0..MAX_PATTERNS_PER_SKILL)
        .map(|i| format!("pattern{}", i))
        .collect();
    assert_eq!(criteria.patterns, expected);
}

#[rstest]
fn test_enforce_limits_tags(limits_enforced_criteria: ActivationCriteria) {
    let criteria = limits_enforced_criteria;
    assert!(
        !criteria
            .tags
            .iter()
            .any(|t| t.len() < MIN_KEYWORD_TAG_LENGTH),
        "tags shorter than {} chars should be filtered out",
        MIN_KEYWORD_TAG_LENGTH
    );
    assert_eq!(
        criteria.tags.len(),
        MAX_TAGS_PER_SKILL,
        "tags should be capped at {}",
        MAX_TAGS_PER_SKILL
    );
}

#[test]
fn test_compile_patterns() {
    let patterns = vec![
        r"(?i)\bwrite\b".to_string(),
        "[invalid".to_string(),
        r"(?i)\bedit\b".to_string(),
    ];
    let compiled = LoadedSkill::compile_patterns(&patterns);
    assert_eq!(compiled.len(), 2);
}

#[test]
fn test_parse_skill_manifest_yaml() {
    let yaml = r#"
name: writing-assistant
version: "1.0.0"
description: Professional writing and editing
activation:
  keywords: ["write", "edit", "proofread"]
  patterns: ["(?i)\\b(write|draft)\\b.*\\b(email|letter)\\b"]
  max_context_tokens: 2000
"#;
    let manifest: SkillManifest = serde_yml::from_str(yaml).expect("parse failed");
    assert_eq!(manifest.name, "writing-assistant");
    assert_eq!(manifest.activation.keywords.len(), 3);
}

#[test]
fn test_parse_openclaw_metadata() {
    let yaml = r#"
name: test-skill
metadata:
  openclaw:
    requires:
      bins: ["vale"]
      env: ["VALE_CONFIG"]
      config: ["/etc/vale.ini"]
"#;
    let manifest: SkillManifest = serde_yml::from_str(yaml).expect("parse failed");
    let meta = manifest
        .metadata
        .expect("metadata should be present in test fixture");
    let openclaw = meta
        .openclaw
        .expect("openclaw metadata should be present in test fixture");
    assert_eq!(openclaw.requires.bins, vec!["vale"]);
    assert_eq!(openclaw.requires.env, vec!["VALE_CONFIG"]);
    assert_eq!(openclaw.requires.config, vec!["/etc/vale.ini"]);
}

#[test]
fn test_loaded_skill_name_version() {
    let skill = LoadedSkill::new(LoadedSkillParts {
        manifest: SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            activation: ActivationCriteria::default(),
            metadata: None,
        },
        prompt_content: "test prompt".to_string(),
        trust: SkillTrust::Trusted,
        source: SkillSource::User(PathBuf::from("/tmp/test")),
        location: LoadedSkillLocation::new(
            "test",
            PathBuf::from("/tmp/test"),
            PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        )
        .expect("test entrypoint is bundle-relative"),
        content_hash: "sha256:000".to_string(),
        compiled_patterns: vec![],
        lowercased_keywords: vec![],
        lowercased_exclude_keywords: vec![],
        lowercased_tags: vec![],
    })
    .expect("test skill location should match manifest");
    assert_eq!(skill.name(), "test");
    assert_eq!(skill.version(), "1.0.0");
}

fn make_skill_parts(manifest_name: &str, location_name: &str) -> LoadedSkillParts {
    LoadedSkillParts {
        manifest: SkillManifest {
            name: manifest_name.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            activation: ActivationCriteria::default(),
            metadata: None,
        },
        prompt_content: String::new(),
        trust: SkillTrust::Trusted,
        source: SkillSource::User(PathBuf::from("/tmp")),
        location: LoadedSkillLocation::new(
            location_name,
            PathBuf::from("/tmp"),
            PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        )
        .expect("test entrypoint is bundle-relative"),
        content_hash: String::new(),
        compiled_patterns: vec![],
        lowercased_keywords: vec![],
        lowercased_exclude_keywords: vec![],
        lowercased_tags: vec![],
    }
}

#[test]
fn test_loaded_skill_new_rejects_mismatched_location_identifier() {
    let result = LoadedSkill::new(make_skill_parts("correct-name", "wrong-name"));
    assert!(result.is_err());
    let msg = result
        .expect_err("mismatched location identifier should fail")
        .to_string();
    assert!(
        msg.contains("wrong-name"),
        "error message should name the mismatched identifier"
    );
    assert!(
        msg.contains("correct-name"),
        "error message should name the manifest name"
    );
}

#[test]
fn test_set_location_rejects_mismatched_identifier() {
    let mut skill = LoadedSkill::new(make_skill_parts("my-skill", "my-skill"))
        .expect("initial skill should be valid");
    let result = skill.set_location(
        LoadedSkillLocation::new(
            "other-skill",
            PathBuf::from("/tmp"),
            PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        )
        .expect("test entrypoint is bundle-relative"),
    );
    assert!(result.is_err());
}

#[test]
fn test_validate_location_matches_manifest_relative_entrypoint() {
    let manifest = SkillManifest {
        name: "my-skill".to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        activation: ActivationCriteria::default(),
        metadata: None,
    };
    let location = LoadedSkillLocation::new(
        "my-skill",
        PathBuf::from("/tmp"),
        PathBuf::from("SKILL.md"),
        SkillPackageKind::SingleFile,
    )
    .expect("test entrypoint is bundle-relative");
    assert!(validate_location_matches_manifest(&manifest, &location).is_ok());
}

#[test]
fn test_loaded_skill_location_rejects_absolute_entrypoint() {
    let result = LoadedSkillLocation::new(
        "my-skill",
        PathBuf::from("/tmp"),
        std::env::temp_dir().join("SKILL.md"),
        SkillPackageKind::SingleFile,
    );
    assert!(result.is_err());
    let msg = result
        .expect_err("absolute entrypoint should be rejected")
        .to_string();
    assert!(
        msg.contains("bundle-relative"),
        "error should mention that the entrypoint must be bundle-relative"
    );
}
