//! Edge-case coverage for `SearchConfig` builders and boundary behaviour in
//! `reciprocal_rank_fusion`.

use super::*;

#[test]
fn test_search_config_builders() {
    let config = SearchConfig::default()
        .with_limit(20)
        .with_rrf_k(30)
        .with_min_score(0.1);

    assert_config(
        &config,
        &ExpectedSearchConfig {
            limit: 20,
            rrf_k: 30,
            min_score: 0.1,
            use_fts: true,
            use_vector: true,
            pre_fusion_limit: 50,
        },
    );

    let fts_only = SearchConfig::default().fts_only();
    assert!(fts_only.use_fts);
    assert!(!fts_only.use_vector);

    let vector_only = SearchConfig::default().vector_only();
    assert!(!vector_only.use_fts);
    assert!(vector_only.use_vector);
}

#[test]
fn test_rrf_both_empty() {
    let config = SearchConfig::default();
    let results = reciprocal_rank_fusion(Vec::new(), Vec::new(), &config);
    assert!(results.is_empty());
}

#[test]
fn test_rrf_fts_only_no_vector() {
    let results = build_single_method_rrf_results(true);
    assert_eq!(results.len(), 3);
    assert_all_fts_only(&results);
    assert_scores_descending(&results);
}

#[test]
fn test_rrf_vector_only_no_fts() {
    let results = build_single_method_rrf_results(false);
    assert_eq!(results.len(), 3);
    assert_all_vector_only(&results);
    assert_scores_descending(&results);
}

#[test]
fn test_rrf_duplicate_chunks_merged() {
    let config = SearchConfig::default().with_limit(10);

    let shared_chunk = Uuid::new_v4();
    let fts_only_chunk = Uuid::new_v4();
    let vector_only_chunk = Uuid::new_v4();
    let doc = Uuid::new_v4();

    // shared_chunk appears at rank 2 in FTS and rank 3 in vector
    let fts_results = vec![
        make_result(fts_only_chunk, doc, 1),
        make_result(shared_chunk, doc, 2),
    ];
    let vector_results = vec![
        make_result(vector_only_chunk, doc, 1),
        make_result(shared_chunk, doc, 3),
    ];

    let results = reciprocal_rank_fusion(fts_results, vector_results, &config);

    // Should have 3 unique chunks (not 4)
    assert_eq!(results.len(), 3);

    // Find the shared chunk in results
    let shared = results
        .iter()
        .find(|r| r.chunk_id == shared_chunk)
        .expect("expected to find hybrid result for shared_chunk");
    assert_hybrid_chunk(shared, 2, 3);

    // The shared chunk's pre-normalization score is 1/(k+2) + 1/(k+3),
    // which is higher than either single-method chunk at rank 1: 1/(k+1).
    // After normalization the shared chunk should be the top result.
    assert_eq!(results[0].chunk_id, shared_chunk);
}

#[test]
fn test_rrf_limit_zero_returns_empty() {
    let config = SearchConfig::default().with_limit(0);

    let doc = Uuid::new_v4();
    let fts_results = vec![
        make_result(Uuid::new_v4(), doc, 1),
        make_result(Uuid::new_v4(), doc, 2),
    ];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    assert!(results.is_empty());
}

#[test]
fn test_rrf_min_score_one_keeps_only_top_result() {
    // RRF scores are always < 1.0 before normalization (1/(k+rank) where
    // k>=1, rank>=1). After normalization the top result gets score=1.0, so
    // min_score=1.0 should keep only the single top result. To truly filter
    // everything, we need min_score > 1.0 -- but with_min_score clamps to 1.0.
    // With a single result: normalized score = 1.0, so it passes
    // min_score=1.0. With multiple results: only the top (score=1.0)
    // survives. To filter ALL results we need to ensure none reach 1.0 -- but
    // normalization always makes the max = 1.0. So min_score=1.0 keeps
    // exactly 1 result (the top).
    //
    // Verified: the retain check is `score >= min_score` and the top score is
    // normalized to exactly 1.0, so one result survives.
    let config = SearchConfig::default().with_limit(10).with_min_score(1.0);

    let doc = Uuid::new_v4();
    let fts_results = vec![
        make_result(Uuid::new_v4(), doc, 1),
        make_result(Uuid::new_v4(), doc, 2),
        make_result(Uuid::new_v4(), doc, 3),
    ];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    // After normalization the top result has score 1.0, so exactly 1 survives
    assert_eq!(results.len(), 1);
    assert!((results[0].score - 1.0).abs() < 0.001);
}

#[test]
fn test_search_config_fts_only() {
    assert_single_method_config(true);
}

#[test]
fn test_search_config_vector_only() {
    assert_single_method_config(false);
}
