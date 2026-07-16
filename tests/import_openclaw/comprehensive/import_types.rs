//! Import option, statistics, and error type tests.

use std::path::PathBuf;

use ironclaw::import::{ImportError, ImportOptions};

#[test]
fn test_import_options_construction() {
    let opts = ImportOptions {
        openclaw_path: PathBuf::from("/test/openclaw"),
        dry_run: true,
        re_embed: false,
        user_id: "test_user".to_string(),
    };

    assert_eq!(opts.user_id, "test_user");
    assert!(opts.dry_run);
    assert!(!opts.re_embed);
}

#[test]
fn test_import_stats_aggregation() {
    let stats = ironclaw::import::ImportStats {
        documents: 5,
        chunks: 10,
        conversations: 3,
        messages: 25,
        settings: 2,
        secrets: 1,
        skipped: 2,
        re_embed_queued: 1,
    };

    assert_eq!(stats.total_imported(), 46); // All except skipped
    assert!(!stats.is_empty());
}

#[test]
fn test_import_error_variants() {
    let err1 = ImportError::ConfigParse("test".to_string());
    assert_eq!(err1.to_string(), "JSON5 parse error: test");

    let err2 = ImportError::Database("db failed".to_string());
    assert_eq!(err2.to_string(), "Database error: db failed");

    let err3 = ImportError::Sqlite("sqlite error".to_string());
    assert_eq!(err3.to_string(), "SQLite error: sqlite error");

    let err4 = ImportError::Workspace("workspace error".to_string());
    assert_eq!(err4.to_string(), "Workspace error: workspace error");
}
