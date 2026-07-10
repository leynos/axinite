//! Unit tests for cloning WIT status updates across variants.

use rstest::rstest;

use super::super::{clone_wit_status_update, wit_channel};

#[test]
fn test_clone_wit_status_update() {
    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::Thinking,
        message: "hello".to_string(),
        metadata_json: "{\"a\":1}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(cloned.status, wit_channel::StatusType::Thinking));
    assert_eq!(cloned.message, "hello");
    assert_eq!(cloned.metadata_json, "{\"a\":1}");
}

#[test]
fn test_clone_wit_status_update_approval_needed() {
    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::ApprovalNeeded,
        message: "approval needed".to_string(),
        metadata_json: "{\"chat_id\":42}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(
        cloned.status,
        wit_channel::StatusType::ApprovalNeeded
    ));
    assert_eq!(cloned.message, "approval needed");
    assert_eq!(cloned.metadata_json, "{\"chat_id\":42}");
}

#[test]
fn test_clone_wit_status_update_auth_completed() {
    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::AuthCompleted,
        message: "auth complete".to_string(),
        metadata_json: "{}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(
        cloned.status,
        wit_channel::StatusType::AuthCompleted
    ));
    assert_eq!(cloned.message, "auth complete");
}

#[rstest]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::Thinking)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::Done)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::Interrupted)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::ToolStarted)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::ToolCompleted)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::ToolResult)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::ApprovalNeeded)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::Status)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::JobStarted)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::AuthRequired)]
#[case(crate::channels::wasm::wrapper::wit_channel::StatusType::AuthCompleted)]
fn test_clone_wit_status_update_all_variants(
    #[case] status: crate::channels::wasm::wrapper::wit_channel::StatusType,
) {
    let original = wit_channel::StatusUpdate {
        status,
        message: "sample".to_string(),
        metadata_json: "{}".to_string(),
    };
    let cloned = clone_wit_status_update(&original);

    assert_eq!(
        std::mem::discriminant(&cloned.status),
        std::mem::discriminant(&original.status)
    );
    assert_eq!(cloned.message, "sample");
    assert_eq!(cloned.metadata_json, "{}");
}
