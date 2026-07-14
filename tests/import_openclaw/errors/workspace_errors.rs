//! Workspace file handling tests for the OpenClaw importer.

use tempfile::TempDir;

use ironclaw::import::openclaw::reader::OpenClawReader;

// ────────────────────────────────────────────────────────────────────
// Workspace File Errors
// ────────────────────────────────────────────────────────────────────

#[test]
fn test_error_workspace_not_directory() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create "workspace" as a file, not a directory
    std::fs::write(openclaw_path.join("workspace"), "not a directory").expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    // Should handle gracefully (no files found)
    let count = reader
        .list_workspace_files()
        .expect("list workspace files failed");
    assert_eq!(count, 0);
}

#[test]
fn test_edge_case_many_markdown_files() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let workspace_dir = openclaw_path.join("workspace");
    std::fs::create_dir_all(&workspace_dir).expect("mkdir failed");

    // Create 100 markdown files
    for i in 0..100 {
        std::fs::write(workspace_dir.join(format!("doc_{}.md", i)), "content")
            .expect("write failed");
    }

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let count = reader
        .list_workspace_files()
        .expect("list workspace files failed");
    assert_eq!(count, 100);
}
