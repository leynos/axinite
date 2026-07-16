//! Unit tests for the memory tool against a workspace store.

use super::*;

fn make_test_workspace() -> Arc<Workspace> {
    Arc::new(Workspace::new(
        "test_user",
        deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(
            tokio_postgres::Config::new(),
            tokio_postgres::NoTls,
        ))
        .build()
        .unwrap(),
    ))
}

#[test]
fn test_memory_search_schema() {
    let workspace = make_test_workspace();
    let tool = MemorySearchTool::new(workspace);

    assert_eq!(tool.name(), "memory_search");
    assert!(!tool.requires_sanitization());

    let schema = tool.parameters_schema();
    assert!(schema["properties"]["query"].is_object());
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&"query".into())
    );
}

#[test]
fn test_memory_write_schema() {
    let workspace = make_test_workspace();
    let tool = MemoryWriteTool::new(workspace);

    assert_eq!(tool.name(), "memory_write");

    let schema = tool.parameters_schema();
    assert!(schema["properties"]["content"].is_object());
    assert!(schema["properties"]["target"].is_object());
    assert!(schema["properties"]["append"].is_object());
}

#[test]
fn test_memory_read_schema() {
    let workspace = make_test_workspace();
    let tool = MemoryReadTool::new(workspace);

    assert_eq!(tool.name(), "memory_read");

    let schema = tool.parameters_schema();
    assert!(schema["properties"]["path"].is_object());
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&"path".into())
    );
}

#[test]
fn test_memory_tree_schema() {
    let workspace = make_test_workspace();
    let tool = MemoryTreeTool::new(workspace);

    assert_eq!(tool.name(), "memory_tree");

    let schema = tool.parameters_schema();
    assert!(schema["properties"]["path"].is_object());
    assert!(schema["properties"]["depth"].is_object());
    assert_eq!(schema["properties"]["depth"]["default"], 1);
}
