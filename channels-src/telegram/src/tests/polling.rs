use crate::polling::get_updates_url;

#[test]
fn test_get_updates_url_includes_offset_and_timeout() {
    let url = get_updates_url(444_809_884, 30);
    assert!(url.contains("offset=444809884"));
    assert!(url.contains("timeout=30"));
    assert!(url.contains("allowed_updates=[\"message\",\"edited_message\"]"));
}
