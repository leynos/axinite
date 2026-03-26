//! Hybrid search combining full-text and semantic search.
//!
//! Uses Reciprocal Rank Fusion (RRF) to combine results from:
//! 1. PostgreSQL full-text search (ts_rank_cd)
//! 2. pgvector cosine similarity search
//!
//! RRF formula: score = sum(1 / (k + rank)) for each retrieval method
//! This is robust to different score scales and produces better results
//! than simple score averaging.

use std::collections::HashMap;

use uuid::Uuid;

/// Configuration for hybrid search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchConfig {
    /// Maximum number of results to return.
    pub limit: usize,
    /// RRF constant (typically 60). Higher values favour top results more.
    pub rrf_k: u32,
    /// Whether to include FTS results.
    pub use_fts: bool,
    /// Whether to include vector results.
    pub use_vector: bool,
    /// Minimum score threshold (0.0-1.0).
    pub min_score: f32,
    /// Maximum results to fetch from each method before fusion.
    pub pre_fusion_limit: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            limit: 10,
            rrf_k: 60,
            use_fts: true,
            use_vector: true,
            min_score: 0.0,
            pre_fusion_limit: 50,
        }
    }
}

impl SearchConfig {
    /// Set the result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set the RRF constant.
    pub fn with_rrf_k(mut self, k: u32) -> Self {
        self.rrf_k = k;
        self
    }

    /// Disable FTS (only use vector search).
    pub fn vector_only(mut self) -> Self {
        self.use_fts = false;
        self.use_vector = true;
        self
    }

    /// Disable vector search (only use FTS).
    pub fn fts_only(mut self) -> Self {
        self.use_fts = true;
        self.use_vector = false;
        self
    }

    /// Set minimum score threshold.
    pub fn with_min_score(mut self, score: f32) -> Self {
        self.min_score = score.clamp(0.0, 1.0);
        self
    }
}

/// A search result with hybrid scoring.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Document ID containing this chunk.
    pub document_id: Uuid,
    /// File path of the source document.
    pub document_path: String,
    /// Chunk ID.
    pub chunk_id: Uuid,
    /// Chunk content.
    pub content: String,
    /// Combined RRF score (0.0-1.0 normalized).
    pub score: f32,
    /// Rank in FTS results (1-based, None if not in FTS results).
    pub fts_rank: Option<u32>,
    /// Rank in vector results (1-based, None if not in vector results).
    pub vector_rank: Option<u32>,
}

impl SearchResult {
    /// Check if this result came from FTS.
    pub fn from_fts(&self) -> bool {
        self.fts_rank.is_some()
    }

    /// Check if this result came from vector search.
    pub fn from_vector(&self) -> bool {
        self.vector_rank.is_some()
    }

    /// Check if this result came from both methods (hybrid match).
    pub fn is_hybrid(&self) -> bool {
        self.fts_rank.is_some() && self.vector_rank.is_some()
    }
}

/// Raw result from a single search method.
#[derive(Debug, Clone)]
pub struct RankedResult {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    /// File path of the source document.
    pub document_path: String,
    pub content: String,
    pub rank: u32, // 1-based rank
}

/// Reciprocal Rank Fusion algorithm.
///
/// Combines ranked results from multiple retrieval methods using the formula:
/// score(d) = sum(1 / (k + rank(d))) for each method where d appears
///
/// # Arguments
///
/// * `fts_results` - Results from full-text search, ordered by relevance
/// * `vector_results` - Results from vector search, ordered by similarity
/// * `config` - Search configuration
///
/// # Returns
///
/// Combined results sorted by RRF score (descending).
pub fn reciprocal_rank_fusion(
    fts_results: Vec<RankedResult>,
    vector_results: Vec<RankedResult>,
    config: &SearchConfig,
) -> Vec<SearchResult> {
    let k = config.rrf_k as f32;

    // Track scores and metadata for each chunk
    struct ChunkInfo {
        document_id: Uuid,
        document_path: String,
        content: String,
        score: f32,
        fts_rank: Option<u32>,
        vector_rank: Option<u32>,
    }

    let mut chunk_scores: HashMap<Uuid, ChunkInfo> = HashMap::new();

    // Process FTS results
    for result in fts_results {
        let rrf_score = 1.0 / (k + result.rank as f32);
        chunk_scores
            .entry(result.chunk_id)
            .and_modify(|info| {
                info.score += rrf_score;
                info.fts_rank = Some(result.rank);
            })
            .or_insert(ChunkInfo {
                document_id: result.document_id,
                document_path: result.document_path,
                content: result.content,
                score: rrf_score,
                fts_rank: Some(result.rank),
                vector_rank: None,
            });
    }

    // Process vector results
    for result in vector_results {
        let rrf_score = 1.0 / (k + result.rank as f32);
        chunk_scores
            .entry(result.chunk_id)
            .and_modify(|info| {
                info.score += rrf_score;
                info.vector_rank = Some(result.rank);
            })
            .or_insert(ChunkInfo {
                document_id: result.document_id,
                document_path: result.document_path,
                content: result.content,
                score: rrf_score,
                fts_rank: None,
                vector_rank: Some(result.rank),
            });
    }

    // Convert to SearchResult and sort by score
    let mut results: Vec<SearchResult> = chunk_scores
        .into_iter()
        .map(|(chunk_id, info)| SearchResult {
            document_id: info.document_id,
            document_path: info.document_path,
            chunk_id,
            content: info.content,
            score: info.score,
            fts_rank: info.fts_rank,
            vector_rank: info.vector_rank,
        })
        .collect();

    // Normalize scores to 0-1 range
    if let Some(max_score) = results.iter().map(|r| r.score).reduce(f32::max)
        && max_score > 0.0
    {
        for result in &mut results {
            result.score /= max_score;
        }
    }

    // Filter by minimum score
    if config.min_score > 0.0 {
        results.retain(|r| r.score >= config.min_score);
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Limit results
    results.truncate(config.limit);

    results
}

/// Compute cosine similarity between two embedding vectors.
///
/// Returns a value in the range [-1.0, 1.0] where 1.0 means identical
/// direction, 0.0 means orthogonal, and -1.0 means opposite direction.
///
/// Returns 0.0 if either vector has zero magnitude to avoid NaN.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        tracing::warn!(
            a_len = a.len(),
            b_len = b.len(),
            "cosine_similarity called with vectors of differing lengths"
        );
        return 0.0;
    }

    let mut dot_product = 0.0;
    let mut magnitude_a = 0.0;
    let mut magnitude_b = 0.0;

    for i in 0..a.len() {
        dot_product += a[i] * b[i];
        magnitude_a += a[i] * a[i];
        magnitude_b += b[i] * b[i];
    }

    magnitude_a = magnitude_a.sqrt();
    magnitude_b = magnitude_b.sqrt();

    // Avoid division by zero
    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    let sim = dot_product / (magnitude_a * magnitude_b);
    if sim.is_nan() { 0.0 } else { sim }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "search_cosine_tests.rs"]
mod cosine_tests;
