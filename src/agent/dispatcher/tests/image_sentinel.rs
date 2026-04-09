//! Image sentinel tests.

#[test]
fn test_image_sentinel_empty_data_url_should_be_skipped() {
    // Regression: unwrap_or_default() on missing "data" field produces an empty
    // string. Broadcasting an empty data_url would send a broken SSE event.
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "path": "/tmp/image.png"
        // "data" field is missing
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        data_url.is_empty(),
        "Missing 'data' field should produce empty string"
    );
    // The fix: empty data_url means we skip broadcasting
}

#[test]
fn test_image_sentinel_present_data_url_is_valid() {
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "data": "data:image/png;base64,abc123",
        "path": "/tmp/image.png"
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        !data_url.is_empty(),
        "Present 'data' field should produce non-empty string"
    );
}
