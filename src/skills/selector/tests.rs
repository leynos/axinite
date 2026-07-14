//! Unit tests for skill prefiltering and relevance scoring.

use super::*;
use crate::skills::{LoadedSkill, SkillSource};
use std::path::PathBuf;

fn make_skill(
    name: &str,
    keywords: &[&str],
    tags: &[&str],
    patterns: &[&str],
) -> anyhow::Result<LoadedSkill> {
    crate::skills::test_support::TestSkillBuilder::new(name)
        .description(format!("{name} skill"))
        .source(SkillSource::User(PathBuf::from("/tmp/test")))
        .root("/tmp/test")
        .keywords(keywords)
        .tags(tags)
        .patterns(patterns)
        .build()
}

#[test]
fn test_empty_message_returns_nothing() {
    let skills = vec![make_skill("test", &["write"], &[], &[]).expect("test skill should build")];
    let result = prefilter_skills("", &skills, 3, MAX_SKILL_CONTEXT_TOKENS);
    assert!(result.is_empty());
}

#[test]
fn test_no_matching_skills() {
    let skills = vec![
        make_skill("cooking", &["recipe", "cook", "bake"], &[], &[])
            .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "Help me write an email",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert!(result.is_empty());
}

#[test]
fn test_keyword_exact_match() {
    let skills =
        vec![make_skill("writing", &["write", "edit"], &[], &[]).expect("test skill should build")];
    let result = prefilter_skills(
        "Please write an email",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name(), "writing");
}

#[test]
fn test_keyword_substring_match() {
    let skills =
        vec![make_skill("writing", &["writing"], &[], &[]).expect("test skill should build")];
    let result = prefilter_skills(
        "I need help with rewriting this text",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_tag_match() {
    let skills = vec![
        make_skill("writing", &[], &["prose", "email"], &[]).expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "Draft an email for me",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_regex_pattern_match() {
    let skills = vec![
        make_skill(
            "writing",
            &[],
            &[],
            &[r"(?i)\b(write|draft)\b.*\b(email|letter)\b"],
        )
        .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "Please draft an email to my boss",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_scoring_priority() {
    let skills = vec![
        make_skill("cooking", &["cook"], &[], &[]).expect("test skill should build"),
        make_skill(
            "writing",
            &["write", "draft"],
            &["email"],
            &[r"(?i)\b(write|draft)\b.*\bemail\b"],
        )
        .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "Write and draft an email",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name(), "writing");
}

#[test]
fn test_max_candidates_limit() {
    let skills = vec![
        make_skill("a", &["test"], &[], &[]).expect("test skill should build"),
        make_skill("b", &["test"], &[], &[]).expect("test skill should build"),
        make_skill("c", &["test"], &[], &[]).expect("test skill should build"),
    ];
    let result = prefilter_skills("test", &skills, 2, MAX_SKILL_CONTEXT_TOKENS);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_context_budget_limit() {
    let mut skill = make_skill("big", &["test"], &[], &[]).expect("test skill should build");
    skill.manifest.activation.max_context_tokens = 3000;
    let mut skill2 = make_skill("also_big", &["test"], &[], &[]).expect("test skill should build");
    skill2.manifest.activation.max_context_tokens = 3000;

    let skills = vec![skill, skill2];
    // Budget of 4000 can only fit one 3000-token skill
    let result = prefilter_skills("test", &skills, 5, 4000);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_invalid_regex_handled_gracefully() {
    let skills = vec![
        make_skill("bad", &["test"], &[], &["[invalid regex"]).expect("test skill should build"),
    ];
    let result = prefilter_skills("test", &skills, 3, MAX_SKILL_CONTEXT_TOKENS);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_keyword_score_capped() {
    let many_keywords: Vec<&str> = vec![
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ];
    let skill = make_skill("spammer", &many_keywords, &[], &[]).expect("test skill should build");
    let skills = vec![skill];
    let result = prefilter_skills(
        "a b c d e f g h i j k l m n o p",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_tag_score_capped() {
    let many_tags: Vec<&str> = vec![
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
    ];
    let skill = make_skill("tag-spammer", &[], &many_tags, &[]).expect("test skill should build");
    let skills = vec![skill];
    let result = prefilter_skills(
        "alpha bravo charlie delta echo foxtrot golf hotel",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_regex_score_capped() {
    let skill = make_skill(
        "regex-spammer",
        &[],
        &[],
        &[
            r"(?i)\bwrite\b",
            r"(?i)\bdraft\b",
            r"(?i)\bedit\b",
            r"(?i)\bcompose\b",
            r"(?i)\bauthor\b",
        ],
    )
    .expect("test skill should build");
    let skills = vec![skill];
    let result = prefilter_skills(
        "write draft edit compose author",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(result.len(), 1);
}

#[test]
fn test_zero_context_tokens_still_costs_budget() {
    let mut skill = make_skill("free", &["test"], &[], &[]).expect("test skill should build");
    skill.manifest.activation.max_context_tokens = 0;
    skill.prompt_content = String::new();
    let mut skill2 = make_skill("also_free", &["test"], &[], &[]).expect("test skill should build");
    skill2.manifest.activation.max_context_tokens = 0;
    skill2.prompt_content = String::new();

    let skills = vec![skill, skill2];
    let result = prefilter_skills("test", &skills, 5, 1);
    assert_eq!(result.len(), 1);
}

fn make_skill_with_excludes(
    name: &str,
    keywords: &[&str],
    exclude_keywords: &[&str],
    tags: &[&str],
    patterns: &[&str],
) -> anyhow::Result<LoadedSkill> {
    let mut skill = make_skill(name, keywords, tags, patterns)?;
    let excl_vec: Vec<String> = exclude_keywords.iter().map(|s| s.to_string()).collect();
    skill.lowercased_exclude_keywords = excl_vec.iter().map(|k| k.to_lowercase()).collect();
    skill.manifest.activation.exclude_keywords = excl_vec;
    Ok(skill)
}

// --- exclude_keywords tests ---

#[test]
fn test_exclude_keyword_vetos_match() {
    // Skill matches on "write" but exclude_keywords: ["route"] — message contains "route"
    // so the skill should score 0 and be excluded.
    let skills = vec![
        make_skill_with_excludes("writer", &["write"], &["route"], &[], &[])
            .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "route this write request to another agent",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert!(
        result.is_empty(),
        "skill with matching exclude_keyword should score 0"
    );
}

#[test]
fn test_exclude_keyword_absent_does_not_block() {
    // Same skill, message does NOT contain the exclude keyword — should activate normally.
    let skills = vec![
        make_skill_with_excludes("writer", &["write"], &["route"], &[], &[])
            .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "help me write an email",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert_eq!(
        result.len(),
        1,
        "skill should activate when no exclude_keyword is present"
    );
}

#[test]
fn test_exclude_keyword_veto_wins_over_positive_match() {
    // Both a keyword match AND an exclude_keyword match are present.
    // The veto must win regardless of how high the positive score is.
    let skills = vec![
        make_skill_with_excludes(
            "writer",
            &["write", "draft", "compose"],
            &["redirect"],
            &[],
            &[],
        )
        .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "write and draft and compose — but redirect this somewhere else",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert!(
        result.is_empty(),
        "exclude_keyword veto must win even when multiple positive keywords match"
    );
}

#[test]
fn test_exclude_keyword_case_insensitive() {
    // exclude_keywords are pre-lowercased; the veto must fire regardless of case in the message.
    let skills = vec![
        make_skill_with_excludes("writer", &["write"], &["Route"], &[], &[])
            .expect("test skill should build"),
    ];
    let result = prefilter_skills(
        "please ROUTE this write request",
        &skills,
        3,
        MAX_SKILL_CONTEXT_TOKENS,
    );
    assert!(
        result.is_empty(),
        "exclude_keyword veto should be case-insensitive"
    );
}
