//! 13-dimension prompt complexity scorer producing tiered score breakdowns.

use std::collections::HashMap;

use regex::Regex;

use super::patterns::{
    RE_CODE, RE_CONJUNCTIONS, RE_CONTEXT, RE_CREATIVITY, RE_DOMAIN_DEFAULT, RE_MULTI_STEP,
    RE_OPEN_ENDED, RE_PRECISION, RE_REASONING, RE_SAFETY, RE_TIER_HINT, RE_TOOL, RE_VAGUE,
    build_domain_regex,
};
use super::tiers::Tier;

/// Weights for each of the 13 scoring dimensions.
#[derive(Debug, Clone)]
pub struct ScorerWeights {
    pub reasoning_words: f32,
    pub token_estimate: f32,
    pub code_indicators: f32,
    pub multi_step: f32,
    pub domain_specific: f32,
    pub ambiguity: f32,
    pub creativity: f32,
    pub precision: f32,
    pub context_dependency: f32,
    pub tool_likelihood: f32,
    pub safety_sensitivity: f32,
    pub question_complexity: f32,
    pub sentence_complexity: f32,
}

impl Default for ScorerWeights {
    fn default() -> Self {
        Self {
            reasoning_words: 0.14,
            token_estimate: 0.12,
            code_indicators: 0.10,
            multi_step: 0.10,
            domain_specific: 0.10,
            ambiguity: 0.05,
            creativity: 0.07,
            precision: 0.06,
            context_dependency: 0.05,
            tool_likelihood: 0.05,
            safety_sensitivity: 0.04,
            question_complexity: 0.07,
            sentence_complexity: 0.05,
        }
    }
}

/// Configuration for the complexity scorer.
#[derive(Debug, Clone, Default)]
pub struct ScorerConfig {
    /// Weights for each scoring dimension.
    pub weights: ScorerWeights,
    /// Custom domain-specific keywords (overrides defaults if provided).
    /// Each entry is a word or regex pattern fragment.
    pub domain_keywords: Option<Vec<String>>,
}

/// Breakdown of complexity score by dimension.
#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    /// Total complexity score (0-100).
    pub total: u32,
    /// Computed tier.
    pub tier: Tier,
    /// Per-dimension scores (0-100 each).
    pub components: HashMap<String, u32>,
    /// Human-readable hints about why this score.
    pub hints: Vec<String>,
}

/// Count regex matches in text.
fn count_matches(re: &Regex, text: &str) -> usize {
    re.find_iter(text).count()
}

/// Mutable per-dimension scores and hints accumulated while scoring a prompt.
///
/// Groups the two output collections that every dimension scorer writes to,
/// so helpers take one accumulator rather than a clump of string-keyed maps.
#[derive(Default)]
struct ScoreAccumulator {
    components: HashMap<String, u32>,
    hints: Vec<String>,
}

impl ScoreAccumulator {
    /// Record the score for one dimension.
    fn record(&mut self, dimension: &str, score: u32) {
        self.components.insert(dimension.to_string(), score);
    }

    /// Add a human-readable hint about why the prompt scored as it did.
    fn hint(&mut self, message: String) {
        self.hints.push(message);
    }
}

/// Score a prompt's complexity across 13 dimensions.
///
/// Returns a `ScoreBreakdown` with a total score (0-100) and per-dimension breakdown.
pub fn score_complexity(prompt: &str) -> ScoreBreakdown {
    score_complexity_with_config(prompt, &ScorerConfig::default())
}

/// Score with custom configuration (weights + domain keywords).
///
/// If you will call this repeatedly with the same config, prefer
/// [`score_complexity_with_regex`] and pre-build the domain regex once.
pub fn score_complexity_with_config(prompt: &str, config: &ScorerConfig) -> ScoreBreakdown {
    let domain_regex = match &config.domain_keywords {
        Some(custom) => {
            let refs: Vec<&str> = custom.iter().map(|s| s.as_str()).collect();
            build_domain_regex(&refs)
        }
        None => RE_DOMAIN_DEFAULT.clone(),
    };
    score_complexity_internal(prompt, &config.weights, &domain_regex)
}

/// Score with a pre-compiled domain regex (avoids rebuilding per call).
pub fn score_complexity_with_regex(
    prompt: &str,
    weights: &ScorerWeights,
    domain_regex: &Regex,
) -> ScoreBreakdown {
    score_complexity_internal(prompt, weights, domain_regex)
}

/// Internal scoring implementation.
fn score_complexity_internal(
    prompt: &str,
    weights: &ScorerWeights,
    domain_regex: &Regex,
) -> ScoreBreakdown {
    // Check for explicit tier hint (e.g. "[tier:flash]")
    if let Some(breakdown) = explicit_tier_breakdown(prompt) {
        return breakdown;
    }

    let mut acc = ScoreAccumulator::default();

    score_token_estimate(prompt, &mut acc);
    score_keyword_dimensions(prompt, domain_regex, &mut acc);
    score_ambiguity(prompt, &mut acc);
    score_question_complexity(prompt, &mut acc);
    score_sentence_complexity(prompt, &mut acc);

    let total = weighted_total(&acc.components, weights);
    let total = apply_dimension_boost(total, &mut acc);

    // Clamp to 0-100
    let total = (total as u32).clamp(0, 100);
    let tier = Tier::from_score(total);

    ScoreBreakdown {
        total,
        tier,
        components: acc.components,
        hints: acc.hints,
    }
}

/// Score token estimate from char count: <20 chars = 0, >=520 chars = 100.
fn score_token_estimate(prompt: &str, acc: &mut ScoreAccumulator) {
    let char_count = prompt.len();
    let token_score = ((char_count as i32 - 20).max(0) as f32 / 5.0).min(100.0) as u32;
    acc.record("token_estimate", token_score);
    if char_count > 200 {
        acc.hint(format!("Long prompt ({char_count} chars)"));
    }
}

/// A keyword-based scoring dimension: its name, matching regex, and the
/// optional match count above which a hint is emitted.
struct KeywordDimension<'a> {
    name: &'a str,
    regex: &'a Regex,
    hint_threshold: Option<usize>,
}

/// Score all keyword-based dimensions: 50 points per match, hint at the
/// dimension's threshold (dimensions without a threshold never hint).
fn score_keyword_dimensions(prompt: &str, domain_regex: &Regex, acc: &mut ScoreAccumulator) {
    let keyword_dimensions: [KeywordDimension; 9] = [
        KeywordDimension {
            name: "reasoning_words",
            regex: &RE_REASONING,
            hint_threshold: Some(2),
        },
        KeywordDimension {
            name: "multi_step",
            regex: &RE_MULTI_STEP,
            hint_threshold: Some(2),
        },
        KeywordDimension {
            name: "creativity",
            regex: &RE_CREATIVITY,
            hint_threshold: Some(2),
        },
        KeywordDimension {
            name: "precision",
            regex: &RE_PRECISION,
            hint_threshold: None,
        },
        KeywordDimension {
            name: "code_indicators",
            regex: &RE_CODE,
            hint_threshold: Some(2),
        },
        KeywordDimension {
            name: "tool_likelihood",
            regex: &RE_TOOL,
            hint_threshold: None,
        },
        KeywordDimension {
            name: "safety_sensitivity",
            regex: &RE_SAFETY,
            hint_threshold: Some(1),
        },
        KeywordDimension {
            name: "context_dependency",
            regex: &RE_CONTEXT,
            hint_threshold: None,
        },
        KeywordDimension {
            name: "domain_specific",
            regex: domain_regex,
            hint_threshold: Some(2),
        },
    ];
    for dimension in keyword_dimensions {
        score_keyword_dimension(prompt, &dimension, acc);
    }
}

/// Score ambiguity from vague-pronoun density.
fn score_ambiguity(prompt: &str, acc: &mut ScoreAccumulator) {
    let vague_count = count_matches(&RE_VAGUE, prompt);
    let ambiguity_score = (vague_count * 25).min(100) as u32;
    acc.record("ambiguity", ambiguity_score);
}

/// Combine per-dimension scores into a weighted total.
fn weighted_total(components: &HashMap<String, u32>, weights: &ScorerWeights) -> f32 {
    [
        ("reasoning_words", weights.reasoning_words),
        ("token_estimate", weights.token_estimate),
        ("code_indicators", weights.code_indicators),
        ("multi_step", weights.multi_step),
        ("domain_specific", weights.domain_specific),
        ("ambiguity", weights.ambiguity),
        ("creativity", weights.creativity),
        ("precision", weights.precision),
        ("context_dependency", weights.context_dependency),
        ("tool_likelihood", weights.tool_likelihood),
        ("safety_sensitivity", weights.safety_sensitivity),
        ("question_complexity", weights.question_complexity),
        ("sentence_complexity", weights.sentence_complexity),
    ]
    .iter()
    .map(|(name, weight)| components.get(*name).copied().unwrap_or(0) as f32 * weight)
    .sum()
}

/// Build a breakdown directly from an explicit "[tier:...]" hint, if present.
fn explicit_tier_breakdown(prompt: &str) -> Option<ScoreBreakdown> {
    let caps = RE_TIER_HINT.captures(prompt)?;
    // Group 1 always exists when the regex matches; an empty string
    // falls through to the defensive branch below.
    let tier_str = caps.get(1).map_or("", |m| m.as_str());
    let tier = match tier_str.to_lowercase().as_str() {
        "flash" => Tier::Flash,
        "standard" => Tier::Standard,
        "pro" => Tier::Pro,
        "frontier" => Tier::Frontier,
        // The regex only captures valid tiers, so this is defensive.
        other => {
            tracing::error!(tier = %other, "Unexpected tier in hint despite regex constraint");
            Tier::Standard
        }
    };
    Some(ScoreBreakdown {
        total: tier.to_score(),
        tier,
        components: HashMap::new(),
        hints: vec![format!("Explicit tier hint: {tier}")],
    })
}

/// Score one keyword dimension: 50 points per regex match, capped at 100.
///
/// Records a "{name}: {count} matches" hint once `hint_threshold` matches
/// fire; dimensions with no threshold never hint.
fn score_keyword_dimension(prompt: &str, dimension: &KeywordDimension, acc: &mut ScoreAccumulator) {
    let count = count_matches(dimension.regex, prompt);
    let score = (count * 50).min(100) as u32;
    acc.record(dimension.name, score);
    if dimension
        .hint_threshold
        .is_some_and(|threshold| count >= threshold)
    {
        acc.hint(format!("{}: {count} matches", dimension.name));
    }
}

/// Score question complexity from '?' density and open-ended phrasing.
fn score_question_complexity(prompt: &str, acc: &mut ScoreAccumulator) {
    let question_marks = prompt.matches('?').count();
    let open_ended_count = count_matches(&RE_OPEN_ENDED, prompt);
    let question_score = ((question_marks * 20) + (open_ended_count * 25)).min(100) as u32;
    acc.record("question_complexity", question_score);
    if question_marks >= 2 {
        acc.hint(format!("Multiple questions: {question_marks}"));
    }
}

/// Score sentence complexity from commas, semicolons, and conjunctions.
fn score_sentence_complexity(prompt: &str, acc: &mut ScoreAccumulator) {
    let commas = prompt.matches(',').count();
    let semicolons = prompt.matches(';').count();
    let conjunctions = count_matches(&RE_CONJUNCTIONS, prompt);
    let clauses = commas + (semicolons * 2) + conjunctions;
    let sentence_score = (clauses * 12).min(100) as u32;
    acc.record("sentence_complexity", sentence_score);
    if clauses >= 5 {
        acc.hint(format!("Complex structure: {clauses} clauses"));
    }
}

/// Apply the multi-dimensional boost: +30% when 3+ dimensions fire above
/// threshold, +15% when exactly 2 fire.
fn apply_dimension_boost(total: f32, acc: &mut ScoreAccumulator) -> f32 {
    let triggered_dimensions = acc.components.values().filter(|&&v| v > 20).count();
    if triggered_dimensions >= 3 {
        acc.hint(format!(
            "Multi-dimensional ({triggered_dimensions} triggers)"
        ));
        total * 1.3
    } else if triggered_dimensions >= 2 {
        total * 1.15
    } else {
        total
    }
}
