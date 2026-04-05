//! Shared fixtures and assertion helpers for the workspace search test suite.

use super::*;

/// Creates a [`RankedResult`] with the given chunk ID, document ID, and rank.
/// The document path is auto-generated as `docs/{doc_id}.md`.
pub(super) fn make_result(chunk_id: Uuid, doc_id: Uuid, rank: u32) -> RankedResult {
    make_result_with_path(chunk_id, doc_id, &format!("docs/{}.md", doc_id), rank)
}

/// Creates a [`RankedResult`] with the given chunk ID, document ID, path, and rank.
/// Use this when you need to specify an explicit document path.
pub(super) fn make_result_with_path(
    chunk_id: Uuid,
    doc_id: Uuid,
    path: &str,
    rank: u32,
) -> RankedResult {
    RankedResult {
        chunk_id,
        document_id: doc_id,
        document_path: path.to_string(),
        content: format!("content for chunk {}", chunk_id),
        rank,
    }
}

/// Core implementation for single-method result assertions.
/// `is_valid` returns `true` for a result that satisfies the expected modality;
/// `label` is used verbatim in the assertion failure message.
fn assert_all_single_method(
    results: &[SearchResult],
    is_valid: impl Fn(&SearchResult) -> bool,
    label: &str,
) {
    let failure = results
        .iter()
        .enumerate()
        .find(|(_, result)| !is_valid(result));
    assert!(
        failure.is_none(),
        "expected all {label} results, found violation: {:?}; full results: {results:#?}",
        failure
    );
}

pub(super) fn assert_all_fts_only(results: &[SearchResult]) {
    assert_all_single_method(
        results,
        |r| r.from_fts() && !r.from_vector() && !r.is_hybrid(),
        "FTS-only",
    );
}

pub(super) fn assert_all_vector_only(results: &[SearchResult]) {
    assert_all_single_method(
        results,
        |r| r.from_vector() && !r.from_fts() && !r.is_hybrid(),
        "vector-only",
    );
}

pub(super) fn assert_scores_descending(results: &[SearchResult]) {
    for (index, window) in results.windows(2).enumerate() {
        assert!(
            window[0].score >= window[1].score,
            "scores not descending at pair {index}/{next}: {} < {}; left={:#?}; right={:#?}",
            window[0].score,
            window[1].score,
            window[0],
            window[1],
            next = index + 1
        );
    }
}

/// Expected field values when asserting a [`SearchConfig`] in tests.
/// Expected values for [`SearchConfig`] fields in test assertions.
/// Used by [`assert_config`] to verify search configuration settings.
pub(super) struct ExpectedSearchConfig {
    /// Maximum number of results to return
    pub(super) limit: usize,
    /// RRF constant k parameter for score normalization
    pub(super) rrf_k: u32,
    /// Minimum score threshold for inclusion
    pub(super) min_score: f32,
    /// Whether full-text search is enabled
    pub(super) use_fts: bool,
    /// Whether vector search is enabled
    pub(super) use_vector: bool,
    /// Pre-fusion limit for individual search methods
    pub(super) pre_fusion_limit: usize,
}

pub(super) fn assert_config(config: &SearchConfig, expected: &ExpectedSearchConfig) {
    assert_eq!(config.limit, expected.limit, "config.limit");
    assert_eq!(config.rrf_k, expected.rrf_k, "config.rrf_k");
    assert_eq!(
        config.pre_fusion_limit, expected.pre_fusion_limit,
        "config.pre_fusion_limit"
    );
    assert!(
        (config.min_score - expected.min_score).abs() < f32::EPSILON,
        "expected min_score {}, got {}",
        expected.min_score,
        config.min_score
    );
    assert_eq!(config.use_fts, expected.use_fts, "config.use_fts");
    assert_eq!(config.use_vector, expected.use_vector, "config.use_vector");
}

pub(super) fn assert_hybrid_chunk(result: &SearchResult, fts_rank: u32, vector_rank: u32) {
    assert!(
        result.is_hybrid(),
        "expected hybrid chunk but got fts_rank={:?}, vector_rank={:?}",
        result.fts_rank,
        result.vector_rank
    );
    assert_eq!(
        result.fts_rank,
        Some(fts_rank),
        "fts_rank mismatch for chunk_id={}: expected {}, got {:?}",
        result.chunk_id,
        fts_rank,
        result.fts_rank
    );
    assert_eq!(
        result.vector_rank,
        Some(vector_rank),
        "vector_rank mismatch for chunk_id={}: expected {}, got {:?}",
        result.chunk_id,
        vector_rank,
        result.vector_rank
    );
}

/// Runs RRF with three ranked inputs fed through one method slot only.
/// Pass `use_fts = true` to supply through the FTS argument; `false` for
/// the vector argument.
pub(super) fn build_single_method_rrf_results(use_fts: bool) -> Vec<SearchResult> {
    let config = SearchConfig::default().with_limit(10);
    let doc = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let inputs = vec![
        make_result(
            Uuid::parse_str("10000000-0000-0000-0000-000000000001").unwrap(),
            doc,
            1,
        ),
        make_result(
            Uuid::parse_str("20000000-0000-0000-0000-000000000001").unwrap(),
            doc,
            2,
        ),
        make_result(
            Uuid::parse_str("30000000-0000-0000-0000-000000000001").unwrap(),
            doc,
            3,
        ),
    ];
    if use_fts {
        reciprocal_rank_fusion(inputs, Vec::new(), &config)
    } else {
        reciprocal_rank_fusion(Vec::new(), inputs, &config)
    }
}

/// Asserts that a single-method [`SearchConfig`] (FTS-only or vector-only)
/// has the expected default field values.
pub(super) fn assert_single_method_config(use_fts: bool) {
    let use_vector = !use_fts;
    let config = if use_fts {
        SearchConfig::default().fts_only()
    } else {
        SearchConfig::default().vector_only()
    };
    assert_config(
        &config,
        &ExpectedSearchConfig {
            limit: 10,
            rrf_k: 60,
            min_score: 0.0,
            use_fts,
            use_vector,
            pre_fusion_limit: 50,
        },
    );
}
