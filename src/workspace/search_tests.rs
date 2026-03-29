use super::*;

fn make_result(chunk_id: Uuid, doc_id: Uuid, rank: u32) -> RankedResult {
    RankedResult {
        chunk_id,
        document_id: doc_id,
        document_path: format!("docs/{}.md", doc_id),
        content: format!("content for chunk {}", chunk_id),
        rank,
    }
}

fn make_result_with_path(chunk_id: Uuid, doc_id: Uuid, path: &str, rank: u32) -> RankedResult {
    RankedResult {
        chunk_id,
        document_id: doc_id,
        document_path: path.to_string(),
        content: format!("content for chunk {}", chunk_id),
        rank,
    }
}

fn assert_all_fts_only(results: &[SearchResult]) {
    assert!(
        results
            .iter()
            .all(|r| r.from_fts() && !r.from_vector() && !r.is_hybrid())
    );
}

fn assert_all_vector_only(results: &[SearchResult]) {
    assert!(
        results
            .iter()
            .all(|r| r.from_vector() && !r.from_fts() && !r.is_hybrid())
    );
}

fn assert_scores_descending(results: &[SearchResult]) {
    for w in results.windows(2) {
        assert!(w[0].score >= w[1].score);
    }
}

/// Expected field values when asserting a [`SearchConfig`] in tests.
struct ExpectedSearchConfig {
    limit: usize,
    rrf_k: u32,
    min_score: f32,
    use_fts: bool,
    use_vector: bool,
}

fn assert_config(config: &SearchConfig, expected: &ExpectedSearchConfig) {
    assert_eq!(config.limit, expected.limit, "config.limit");
    assert_eq!(config.rrf_k, expected.rrf_k, "config.rrf_k");
    assert!(
        (config.min_score - expected.min_score).abs() < f32::EPSILON,
        "expected min_score {}, got {}",
        expected.min_score,
        config.min_score
    );
    assert_eq!(config.use_fts, expected.use_fts, "config.use_fts");
    assert_eq!(config.use_vector, expected.use_vector, "config.use_vector");
}

fn assert_hybrid_chunk(result: &SearchResult, fts_rank: u32, vector_rank: u32) {
    assert!(result.is_hybrid());
    assert_eq!(result.fts_rank, Some(fts_rank));
    assert_eq!(result.vector_rank, Some(vector_rank));
}

#[test]
fn test_rrf_propagates_document_path() {
    // Regression test: search results must carry the source document's file
    // path, not the document UUID. See PR #503 / issue #481.
    let config = SearchConfig::default().with_limit(10);

    let doc_a = Uuid::new_v4();
    let doc_b = Uuid::new_v4();
    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let chunk3 = Uuid::new_v4();

    let fts_results = vec![
        make_result_with_path(chunk1, doc_a, "notes/todo.md", 1),
        make_result_with_path(chunk2, doc_b, "journal/2024-01-15.md", 2),
    ];
    let vector_results = vec![
        make_result_with_path(chunk1, doc_a, "notes/todo.md", 1),
        make_result_with_path(chunk3, doc_b, "journal/2024-01-15.md", 2),
    ];

    let results = reciprocal_rank_fusion(fts_results, vector_results, &config);

    for result in &results {
        // The path must be a real file path, never a UUID string
        assert!(
            Uuid::parse_str(&result.document_path).is_err(),
            "document_path looks like a UUID ('{}'), expected a file path",
            result.document_path
        );
    }

    // Verify exact paths are preserved
    let paths: Vec<&str> = results.iter().map(|r| r.document_path.as_str()).collect();
    assert!(
        paths.contains(&"notes/todo.md"),
        "missing notes/todo.md in {:?}",
        paths
    );
    assert!(
        paths.contains(&"journal/2024-01-15.md"),
        "missing journal/2024-01-15.md in {:?}",
        paths
    );

    // Hybrid match (chunk1) should preserve the correct path
    let hybrid = results
        .iter()
        .find(|r| r.chunk_id == chunk1)
        .expect("expected to find hybrid result for chunk1");
    assert_eq!(hybrid.document_path, "notes/todo.md");
    assert!(hybrid.is_hybrid());
}

#[test]
fn test_rrf_single_method() {
    let config = SearchConfig::default().with_limit(10);

    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let fts_results = vec![make_result(chunk1, doc, 1), make_result(chunk2, doc, 2)];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    assert_eq!(results.len(), 2);
    assert!(results[0].score > results[1].score);
    assert_all_fts_only(&results);
}

#[test]
fn test_rrf_hybrid_match_boosted() {
    let config = SearchConfig::default().with_limit(10);

    let chunk1 = Uuid::new_v4(); // In both
    let chunk2 = Uuid::new_v4(); // FTS only
    let chunk3 = Uuid::new_v4(); // Vector only
    let doc = Uuid::new_v4();

    let fts_results = vec![make_result(chunk1, doc, 1), make_result(chunk2, doc, 2)];

    let vector_results = vec![make_result(chunk1, doc, 1), make_result(chunk3, doc, 2)];

    let results = reciprocal_rank_fusion(fts_results, vector_results, &config);

    assert_eq!(results.len(), 3);
    let top = &results[0];
    assert_eq!(top.chunk_id, chunk1);
    assert!(top.score > results[1].score);
    assert!(top.is_hybrid());

    let remaining = &results[1..];
    assert!(remaining.iter().all(|r| !r.is_hybrid()));
}

#[test]
fn test_rrf_score_normalization() {
    let config = SearchConfig::default();

    let chunk1 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let fts_results = vec![make_result(chunk1, doc, 1)];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    // Single result should have normalized score of 1.0
    assert_eq!(results.len(), 1);
    assert!((results[0].score - 1.0).abs() < 0.001);
}

#[test]
fn test_rrf_min_score_filter() {
    let config = SearchConfig::default().with_limit(10).with_min_score(0.5);

    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let chunk3 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    // chunk1 has rank 1, chunk3 has rank 100 (low score)
    let fts_results = vec![
        make_result(chunk1, doc, 1),
        make_result(chunk2, doc, 50),
        make_result(chunk3, doc, 100),
    ];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    // Low-scoring results should be filtered out
    // All results should have score >= 0.5
    for result in &results {
        assert!(result.score >= 0.5);
    }
}

#[test]
fn test_rrf_limit() {
    let config = SearchConfig::default().with_limit(2);

    let doc = Uuid::new_v4();
    let fts_results: Vec<_> = (1..=5)
        .map(|i| make_result(Uuid::new_v4(), doc, i))
        .collect();

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    assert_eq!(results.len(), 2);
}

#[test]
fn test_rrf_k_parameter() {
    // Higher k values make ranking differences less pronounced
    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let fts_results = vec![make_result(chunk1, doc, 1), make_result(chunk2, doc, 2)];

    // Low k: rank 1 score = 1/(10+1) = 0.091, rank 2 = 1/(10+2) = 0.083
    let config_low_k = SearchConfig::default().with_rrf_k(10);
    let results_low = reciprocal_rank_fusion(fts_results.clone(), Vec::new(), &config_low_k);

    // High k: rank 1 score = 1/(100+1) = 0.0099, rank 2 = 1/(100+2) = 0.0098
    let config_high_k = SearchConfig::default().with_rrf_k(100);
    let results_high = reciprocal_rank_fusion(fts_results, Vec::new(), &config_high_k);

    // With low k, the score difference is larger (relatively)
    let diff_low = results_low[0].score - results_low[1].score;
    let diff_high = results_high[0].score - results_high[1].score;

    // Low k should have larger relative difference
    assert!(diff_low > diff_high);
}

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
        },
    );

    let fts_only = SearchConfig::default().fts_only();
    assert!(fts_only.use_fts);
    assert!(!fts_only.use_vector);

    let vector_only = SearchConfig::default().vector_only();
    assert!(!vector_only.use_fts);
    assert!(vector_only.use_vector);
}

// --- Edge case tests ---

#[test]
fn test_rrf_both_empty() {
    let config = SearchConfig::default();
    let results = reciprocal_rank_fusion(Vec::new(), Vec::new(), &config);
    assert!(results.is_empty());
}

#[test]
fn test_rrf_fts_only_no_vector() {
    let config = SearchConfig::default().with_limit(10);

    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let chunk3 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let fts_results = vec![
        make_result(chunk1, doc, 1),
        make_result(chunk2, doc, 2),
        make_result(chunk3, doc, 3),
    ];

    let results = reciprocal_rank_fusion(fts_results, Vec::new(), &config);

    assert_eq!(results.len(), 3);
    assert_all_fts_only(&results);
    assert_scores_descending(&results);
}

#[test]
fn test_rrf_vector_only_no_fts() {
    let config = SearchConfig::default().with_limit(10);

    let chunk1 = Uuid::new_v4();
    let chunk2 = Uuid::new_v4();
    let chunk3 = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let vector_results = vec![
        make_result(chunk1, doc, 1),
        make_result(chunk2, doc, 2),
        make_result(chunk3, doc, 3),
    ];

    let results = reciprocal_rank_fusion(Vec::new(), vector_results, &config);

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
fn test_rrf_min_score_one_filters_all() {
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
    let config = SearchConfig::default().fts_only();

    assert_config(
        &config,
        &ExpectedSearchConfig {
            limit: 10,
            rrf_k: 60,
            min_score: 0.0,
            use_fts: true,
            use_vector: false,
        },
    );
}

#[test]
fn test_search_config_vector_only() {
    let config = SearchConfig::default().vector_only();

    assert_config(
        &config,
        &ExpectedSearchConfig {
            limit: 10,
            rrf_k: 60,
            min_score: 0.0,
            use_fts: false,
            use_vector: true,
        },
    );
}
