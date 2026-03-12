use super::*;

#[test]
fn test_build_rig_request_injects_cache_control_short() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::Short,
    )
    .unwrap();

    let params = req
        .additional_params
        .expect("should have additional_params for Short retention");
    assert_eq!(params["cache_control"]["type"], "ephemeral");
    assert!(
        params["cache_control"].get("ttl").is_none(),
        "Short retention should not include ttl"
    );
}

#[test]
fn test_build_rig_request_injects_cache_control_long() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::Long,
    )
    .unwrap();

    let params = req
        .additional_params
        .expect("should have additional_params for Long retention");
    assert_eq!(params["cache_control"]["type"], "ephemeral");
    assert_eq!(params["cache_control"]["ttl"], "1h");
}

#[test]
fn test_build_rig_request_no_cache_control_when_none() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::None,
    )
    .unwrap();

    assert!(
        req.additional_params.is_none(),
        "additional_params should be None when cache is disabled"
    );
}
