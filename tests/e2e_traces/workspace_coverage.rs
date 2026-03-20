//! E2E trace tests: workspace persistence (#574).
//!
//! Covers chunking, multi-document search, hybrid search, directory tree,
//! document lifecycle (write/read/overwrite), and identity in system prompt.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_llm::LlmTrace;

/// Asserts that at least one result for `tool_name` has a preview
/// containing any of `expected_substrings`.
fn assert_tool_result_contains(
    results: &[(String, String)],
    tool_name: &str,
    expected_substrings: &[&str],
) {
    let matched: Vec<_> = results
        .iter()
        .filter(|(name, _)| name == tool_name)
        .collect();
    assert!(
        !matched.is_empty(),
        "Expected at least one result for tool '{tool_name}'"
    );
    assert!(
        matched
            .iter()
            .any(|(_, preview)| expected_substrings.iter().any(|s| preview.contains(s))),
        "No result for '{tool_name}' contained any of {expected_substrings:?}: {matched:?}"
    );
}

// -----------------------------------------------------------------------
// Test 1: write_chunk_search
// -----------------------------------------------------------------------

#[tokio::test]
async fn write_chunk_search() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/write_chunk_search.json"
    ))
    .await
    .expect("failed to load write_chunk_search.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Write a long architecture document and search it")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify the document was persisted via workspace.
    let ws = rig.workspace().expect("workspace must be available");
    let doc = ws
        .read("context/architecture.md")
        .await
        .expect("architecture.md should exist");
    assert!(
        doc.content.contains("Payment Service"),
        "Document should contain 'Payment Service'"
    );
    assert!(
        doc.content.len() > 1000,
        "Document should be long (>1000 chars), got {}",
        doc.content.len()
    );

    // Verify memory_search was called and returned relevant results.
    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"memory_search".to_string()),
        "memory_search should be called: {started:?}"
    );
    let results = rig.tool_results();
    assert_tool_result_contains(
        &results,
        "memory_search",
        &["Payment Service", "payment", "architecture"],
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 2: multi_document_search
// -----------------------------------------------------------------------

#[tokio::test]
async fn multi_document_search() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/multi_doc_search.json"
    ))
    .await
    .expect("failed to load multi_doc_search.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Write three docs and search across them")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify all three documents were written.
    let ws = rig.workspace().expect("workspace must be available");
    let frontend = ws.read("context/frontend.md").await;
    let backend = ws.read("context/backend.md").await;
    let devops = ws.read("context/devops.md").await;
    assert!(frontend.is_ok(), "frontend.md should exist");
    assert!(backend.is_ok(), "backend.md should exist");
    assert!(devops.is_ok(), "devops.md should exist");

    // Verify cross-document memory_search was called.
    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"memory_search".to_string()),
        "memory_search should be called in multi_document_search: {started:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 3: hybrid_search_keyword_pipeline
// -----------------------------------------------------------------------

#[tokio::test]
async fn hybrid_search_keyword_pipeline() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/hybrid_search.json"
    ))
    .await
    .expect("failed to load hybrid_search.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Write and semantically search for ML content")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify both memory_write and memory_search were used.
    // This rig does not install a real embedding provider, so the test covers
    // the write-then-search pipeline through the keyword/FTS path.
    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"memory_write".to_string()),
        "memory_write should be called: {started:?}"
    );
    assert!(
        started.contains(&"memory_search".to_string()),
        "memory_search should be called: {started:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 4: directory_tree
// -----------------------------------------------------------------------

#[tokio::test]
async fn directory_tree() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/directory_tree.json"
    ))
    .await
    .expect("failed to load directory_tree.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Write files in a hierarchy and show the tree")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify tree tool was called.
    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"memory_tree".to_string()),
        "memory_tree should be called: {started:?}"
    );

    // Verify the tree result contains the expected directory hierarchy.
    let results = rig.tool_results();
    assert_tool_result_contains(&results, "memory_tree", &["alpha", "Alpha"]);
    assert_tool_result_contains(&results, "memory_tree", &["beta", "Beta"]);

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 5: document_lifecycle
// -----------------------------------------------------------------------

#[tokio::test]
async fn document_lifecycle() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/doc_lifecycle.json"
    ))
    .await
    .expect("failed to load doc_lifecycle.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Write, read, overwrite, and read a document")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify the document has the updated content.
    let ws = rig.workspace().expect("workspace must be available");
    let doc = ws
        .read("context/lifecycle.md")
        .await
        .expect("lifecycle.md should exist");
    assert!(
        doc.content.contains("Version 2"),
        "Document should contain 'Version 2', got: {:?}",
        doc.content
    );

    // memory_write and memory_read should each be called twice.
    let started = rig.tool_calls_started();
    let write_count = started
        .iter()
        .filter(|n| n.as_str() == "memory_write")
        .count();
    let read_count = started
        .iter()
        .filter(|n| n.as_str() == "memory_read")
        .count();
    assert_eq!(write_count, 2, "Expected 2 memory_write calls");
    assert_eq!(read_count, 2, "Expected 2 memory_read calls");

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 6: identity_in_system_prompt
// -----------------------------------------------------------------------

#[tokio::test]
async fn identity_in_system_prompt() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/workspace/identity_prompt.json"
    ))
    .await
    .expect("failed to load identity_prompt.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    // Seed an IDENTITY.md so the system prompt has real content to inject.
    let ws = rig.workspace().expect("workspace must be available");
    ws.write(
        "IDENTITY.md",
        "I am TestBot, a helpful testing assistant created for E2E verification.",
    )
    .await
    .expect("write IDENTITY.md");

    rig.send_message("Who are you?").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify the TraceLlm captured requests include a system message
    // with the seeded identity content.
    let trace_llm = rig.trace_llm().expect("trace_llm must be available");
    let captured = trace_llm
        .captured_requests()
        .expect("failed to inspect captured LLM requests");
    assert!(
        !captured.is_empty(),
        "Expected at least one captured request"
    );
    let first_request = &captured[0];
    let system_msg = first_request
        .iter()
        .find(|msg| matches!(msg.role, ironclaw::llm::Role::System));
    assert!(
        system_msg.is_some(),
        "Expected a system message in the first request"
    );
    let system_content = &system_msg.expect("system message verified above").content;
    let preview = system_content.chars().take(200).collect::<String>();
    assert!(
        system_content.contains("TestBot"),
        "System prompt should contain seeded identity 'TestBot', got: {:?}",
        preview
    );

    rig.shutdown();
}
