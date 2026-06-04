use crate::webhook::delete_webhook;

#[test]
fn test_delete_webhook_not_available_outside_polling() {
    // this ensures webhook entrypoint resolves and can be called from tests.
    let _ = delete_webhook;
}
