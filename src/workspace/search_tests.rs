//! Unit tests for `reciprocal_rank_fusion`, `SearchConfig`, and related search
//! regression coverage in the workspace module.

use super::*;

#[path = "search_tests/common.rs"]
mod common;
#[path = "search_tests/edge_cases.rs"]
mod edge_cases;

use common::*;

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
