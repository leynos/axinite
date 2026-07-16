//! Tests for JSON-RPC send-parameter construction, including attachments,
//! and for `OutgoingResponse` attachment plumbing.

use super::*;

// ── build_rpc_params tests ──────────────────────────────────────

#[test]
fn build_rpc_params_direct_with_message() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Direct("+5555555555".to_string());
    let params = ch.build_rpc_params(&target, Some("Hello!"), None);
    assert_eq!(params["recipient"], serde_json::json!(["+5555555555"]));
    assert_eq!(params["account"], "+1234567890");
    assert_eq!(params["message"], "Hello!");
    // Direct targets must NOT include groupId.
    assert!(params.get("groupId").is_none());
    Ok(())
}

#[test]
fn build_rpc_params_direct_without_message() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Direct("+5555555555".to_string());
    let params = ch.build_rpc_params(&target, None, None);
    assert_eq!(params["recipient"], serde_json::json!(["+5555555555"]));
    assert_eq!(params["account"], "+1234567890");
    // No message key should be present for typing indicators.
    assert!(params.get("message").is_none());
    Ok(())
}

#[test]
fn build_rpc_params_group_with_message() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Group("abc123".to_string());
    let params = ch.build_rpc_params(&target, Some("Group msg"), None);
    assert_eq!(params["groupId"], "abc123");
    assert_eq!(params["account"], "+1234567890");
    assert_eq!(params["message"], "Group msg");
    // Group targets must NOT include recipient.
    assert!(params.get("recipient").is_none());
    Ok(())
}

#[test]
fn build_rpc_params_group_without_message() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Group("abc123".to_string());
    let params = ch.build_rpc_params(&target, None, None);
    assert_eq!(params["groupId"], "abc123");
    assert_eq!(params["account"], "+1234567890");
    assert!(params.get("message").is_none());
    Ok(())
}

#[test]
fn build_rpc_params_uuid_direct_target() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let target = RecipientTarget::Direct(uuid.to_string());
    let params = ch.build_rpc_params(&target, Some("hi"), None);
    assert_eq!(params["recipient"], serde_json::json!([uuid]));
    Ok(())
}

// ── build_rpc_params with attachments tests ─────────────────────────

#[test]
fn build_rpc_params_with_attachments() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Direct("+5555555555".to_string());
    let attachments = vec!["/path/to/image.png".to_string()];
    let params = ch.build_rpc_params(&target, Some("Check this!"), Some(&attachments));
    assert_eq!(params["recipient"], serde_json::json!(["+5555555555"]));
    assert_eq!(params["message"], "Check this!");
    assert_eq!(
        params["attachments"],
        serde_json::json!(["/path/to/image.png"])
    );
    Ok(())
}

#[test]
fn build_rpc_params_with_multiple_attachments() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Direct("+5555555555".to_string());
    let attachments = vec![
        "/path/to/image.png".to_string(),
        "/path/to/document.pdf".to_string(),
    ];
    let params = ch.build_rpc_params(&target, Some("Files attached"), Some(&attachments));
    assert_eq!(
        params["attachments"],
        serde_json::json!(["/path/to/image.png", "/path/to/document.pdf"])
    );
    Ok(())
}

#[test]
fn build_rpc_params_with_attachments_no_message() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Direct("+5555555555".to_string());
    let attachments = vec!["/path/to/image.png".to_string()];
    let params = ch.build_rpc_params(&target, None, Some(&attachments));
    assert!(params.get("message").is_none());
    assert_eq!(
        params["attachments"],
        serde_json::json!(["/path/to/image.png"])
    );
    Ok(())
}

#[test]
fn build_rpc_params_group_with_attachments() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let target = RecipientTarget::Group("abc123".to_string());
    let attachments = vec!["/path/to/photo.jpg".to_string()];
    let params = ch.build_rpc_params(&target, Some("Group photo"), Some(&attachments));
    assert_eq!(params["groupId"], "abc123");
    assert_eq!(params["message"], "Group photo");
    assert_eq!(
        params["attachments"],
        serde_json::json!(["/path/to/photo.jpg"])
    );
    Ok(())
}

// ── OutgoingResponse attachment tests ─────────────────────────────

#[test]
fn outgoing_response_with_attachments() {
    let response = OutgoingResponse::text("Hello with file")
        .with_attachments(vec!["/path/to/file.png".to_string()]);
    assert_eq!(response.content, "Hello with file");
    assert!(
        response
            .attachments
            .contains(&"/path/to/file.png".to_string())
    );
}

#[test]
fn outgoing_response_text_empty_attachments() {
    let response = OutgoingResponse::text("Hello");
    assert_eq!(response.content, "Hello");
    assert!(response.attachments.is_empty());
}
