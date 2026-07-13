//! Complexity tiers and their mapping to provider-level task complexity.

/// Complexity tier produced by the 13-dimension scorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Simple requests: greetings, quick lookups (score 0-15).
    Flash,
    /// Standard tasks: writing, comparisons (score 16-40).
    Standard,
    /// Complex work: multi-step analysis, code review (score 41-65).
    Pro,
    /// Critical tasks: security audits, high-stakes decisions (score 66+).
    Frontier,
}

impl Tier {
    /// Convert a complexity score to a tier.
    pub fn from_score(score: u32) -> Self {
        match score {
            0..=15 => Tier::Flash,
            16..=40 => Tier::Standard,
            41..=65 => Tier::Pro,
            _ => Tier::Frontier,
        }
    }

    /// Get a representative score for this tier (used when score is not computed).
    pub fn to_score(self) -> u32 {
        match self {
            Tier::Flash => 8,
            Tier::Standard => 28,
            Tier::Pro => 52,
            Tier::Frontier => 80,
        }
    }

    /// Tier name as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Flash => "flash",
            Tier::Standard => "standard",
            Tier::Pro => "pro",
            Tier::Frontier => "frontier",
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Classification of a request's complexity, determining which model handles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Short, simple queries -> cheap model (Flash + Standard tiers)
    Simple,
    /// Ambiguous complexity -> cheap model first, cascade to primary if uncertain (Pro tier)
    Moderate,
    /// Code generation, analysis, multi-step reasoning -> primary model (Frontier tier)
    Complex,
}

impl From<Tier> for TaskComplexity {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Flash | Tier::Standard => TaskComplexity::Simple,
            Tier::Pro => TaskComplexity::Moderate,
            Tier::Frontier => TaskComplexity::Complex,
        }
    }
}
