//! Compiled regex patterns for the complexity scorer and the fast-path
//! pattern overrides applied before full scoring.

use std::sync::LazyLock;

use lazy_regex::{Lazy, lazy_regex, regex};
use regex::Regex;

use super::keywords::DEFAULT_DOMAIN_KEYWORDS;
use super::tiers::Tier;

/// Build a domain regex from a keyword list, with fallback on invalid patterns.
///
/// An empty keyword list falls back to the default keywords so scoring
/// doesn't break when `domain_keywords: Some(vec![])` is configured.
pub(super) fn build_domain_regex(keywords: &[&str]) -> Regex {
    if keywords.is_empty() {
        return RE_DOMAIN_DEFAULT.clone();
    }
    let pattern = format!(r"(?i)\b({})\b", keywords.join("|"));
    Regex::new(&pattern).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Invalid domain keywords pattern, using minimal fallback");
        Regex::clone(regex!(r"(?i)\b(api|code|deploy)\b"))
    })
}

pub(super) static RE_REASONING: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(why|how|explain|analyze|analyse|compare|contrast|evaluate|assess|reason|think|consider|implications?|consequences?|trade-?offs?|pros?\s*(and|&)\s*cons?|advantages?|disadvantages?|benefits?|drawbacks?|differs?|difference|versus|vs\.?|better|worse|optimal|best|worst)\b"
);

pub(super) static RE_MULTI_STEP: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(first|then|next|after|before|finally|step|steps|phase|stages?|process|workflow|sequence|procedure|pipeline|chain|series|order|followed by)\b"
);

pub(super) static RE_CREATIVITY: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(write|create|generate|compose|design|imagine|brainstorm|ideate|draft|invent|story|poem|essay|article|blog|content|narrative|script|summarize|summarise|rewrite|paraphrase|translate|adapt|tweet|post|thread|outline|structure|format|style|tone|voice)\b"
);

pub(super) static RE_PRECISION: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(\d{4}|\d+\.\d+|exactly|precisely|specific|accurate|correct|verify|confirm|date|time|number|calculate|compute|measure|count)\b"
);

pub(super) static RE_CODE: Lazy<Regex> = lazy_regex!(
    r"(?i)(`{1,3}|```|function|const|let|var|import|export|class|def |async|await|=>|\.ts|\.js|\.py|\.rs|\.go|\.sol|\(\)|\[\]|\{\}|<[A-Z][a-z]+>|useState|useEffect|npm|yarn|pnpm|cargo|pip|implement|rebase|merge|commit|branch|PR|pull.?request|columns?|migrations?|module|refactor|debug|fix|bug|error|schema|database|query)"
);

pub(super) static RE_TOOL: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(file|read|write|search|fetch|run|execute|check|look up|find|open|save|send|post|get|download|upload|install|deploy|build|compile|test|add|update|remove|delete|modify|change|edit|create|resolve|push|pull|clone)\b"
);

pub(super) static RE_SAFETY: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(password|secret|private|confidential|medical|legal|financial|personal|sensitive|ssn|credit.?card|auth|token|key|encrypt|decrypt|hash|vulnerability|exploit|attack|breach)\b"
);

pub(super) static RE_CONTEXT: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(previous|earlier|above|before|last|that|those|it|they|we discussed|you said|mentioned|remember|recall|as I said|like I mentioned)\b"
);

pub(super) static RE_VAGUE: Lazy<Regex> =
    lazy_regex!(r"(?i)\b(it|this|that|something|stuff|thing|things)\b");

pub(super) static RE_OPEN_ENDED: Lazy<Regex> =
    lazy_regex!(r"(?i)\b(why|how|what if|explain|describe|elaborate|discuss)\b");

pub(super) static RE_CONJUNCTIONS: Lazy<Regex> = lazy_regex!(
    r"(?i)\b(and|but|or|however|therefore|because|although|while|whereas|moreover|furthermore)\b"
);

pub(super) static RE_TIER_HINT: Lazy<Regex> =
    lazy_regex!(r"(?i)\[tier:(flash|standard|pro|frontier)\]");

/// Default domain regex, compiled once from `DEFAULT_DOMAIN_KEYWORDS`.
pub(super) static RE_DOMAIN_DEFAULT: LazyLock<Regex> =
    LazyLock::new(|| build_domain_regex(DEFAULT_DOMAIN_KEYWORDS));

// ---------------------------------------------------------------------------
// Pattern overrides (fast-path before scoring)
// ---------------------------------------------------------------------------

/// A compiled pattern override entry.
pub(super) struct PatternOverride {
    pub(super) regex: Regex,
    pub(super) tier: Tier,
}

/// Default pattern overrides, compiled once.
pub(super) static DEFAULT_OVERRIDES: LazyLock<Vec<PatternOverride>> = LazyLock::new(|| {
    vec![
        // Flash tier: greetings and acknowledgements
        PatternOverride {
            regex: Regex::clone(regex!(
                r"(?i)^(hi|hello|hey|thanks|ok|sure|yes|no|yep|nope|cool|nice|great|got it)$"
            )),
            tier: Tier::Flash,
        },
        // Flash tier: quick lookups (end-anchored to avoid matching complex questions
        // like "What time complexity is merge sort?")
        PatternOverride {
            regex: Regex::clone(regex!(
                r"(?i)^what(?:'s|\s+is)?\s+(?:the\s+)?(time|date|day|weather)\b(?:\s+(?:is\s+it|today|now|in\s+\S+))?[?.!]*$"
            )),
            tier: Tier::Flash,
        },
        // Frontier tier: security audits
        PatternOverride {
            regex: Regex::clone(regex!(r"(?i)security.*(audit|review|scan)")),
            tier: Tier::Frontier,
        },
        PatternOverride {
            regex: Regex::clone(regex!(
                r"(?i)vulnerability(y|ies).*(review|scan|check|audit)"
            )),
            tier: Tier::Frontier,
        },
        // Pro tier: production deployments
        PatternOverride {
            regex: Regex::clone(regex!(r"(?i)deploy.*(mainnet|production)")),
            tier: Tier::Pro,
        },
        PatternOverride {
            regex: Regex::clone(regex!(r"(?i)production.*(deploy|release|push)")),
            tier: Tier::Pro,
        },
    ]
});
