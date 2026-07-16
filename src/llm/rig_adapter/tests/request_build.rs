//! Tests for rig request construction and cache-control shaping.

use super::*;

#[rstest]
#[case(CacheRetention::Short, None)]
#[case(CacheRetention::Long, Some("1h"))]
#[case(CacheRetention::None, None)]
fn test_build_rig_request_cache_control(
    #[case] retention: CacheRetention,
    #[case] expected_ttl: Option<&str>,
) {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        retention,
    )
    // The rstest case name identifies the retention variant on failure.
    .expect("build_rig_request should succeed for this cache retention");

    match retention {
        CacheRetention::None => assert!(
            req.additional_params.is_none(),
            "additional_params should be None when cache is disabled"
        ),
        CacheRetention::Short | CacheRetention::Long => {
            let params = req
                .additional_params
                .expect("should have additional_params for this cache retention");
            assert_eq!(params["cache_control"]["type"], "ephemeral");

            if let Some(expected_ttl) = expected_ttl {
                assert_eq!(params["cache_control"]["ttl"], expected_ttl);
            } else {
                assert!(
                    params["cache_control"].get("ttl").is_none(),
                    "Short retention should not include ttl"
                );
            }
        }
    }
}
