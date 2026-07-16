//! Unit tests for parsing and displaying cache retention settings.

use super::super::*;

#[test]
fn cache_retention_from_str_primary_values() {
    assert_eq!(
        "none".parse::<CacheRetention>().unwrap(),
        CacheRetention::None
    );
    assert_eq!(
        "short".parse::<CacheRetention>().unwrap(),
        CacheRetention::Short
    );
    assert_eq!(
        "long".parse::<CacheRetention>().unwrap(),
        CacheRetention::Long
    );
}

#[test]
fn cache_retention_from_str_aliases() {
    assert_eq!(
        "off".parse::<CacheRetention>().unwrap(),
        CacheRetention::None
    );
    assert_eq!(
        "disabled".parse::<CacheRetention>().unwrap(),
        CacheRetention::None
    );
    assert_eq!(
        "5m".parse::<CacheRetention>().unwrap(),
        CacheRetention::Short
    );
    assert_eq!(
        "ephemeral".parse::<CacheRetention>().unwrap(),
        CacheRetention::Short
    );
    assert_eq!(
        "1h".parse::<CacheRetention>().unwrap(),
        CacheRetention::Long
    );
}

#[test]
fn cache_retention_from_str_case_insensitive() {
    assert_eq!(
        "NONE".parse::<CacheRetention>().unwrap(),
        CacheRetention::None
    );
    assert_eq!(
        "Short".parse::<CacheRetention>().unwrap(),
        CacheRetention::Short
    );
    assert_eq!(
        "LONG".parse::<CacheRetention>().unwrap(),
        CacheRetention::Long
    );
    assert_eq!(
        "Ephemeral".parse::<CacheRetention>().unwrap(),
        CacheRetention::Short
    );
}

#[test]
fn cache_retention_from_str_invalid() {
    let err = "bogus".parse::<CacheRetention>().unwrap_err();
    assert!(
        err.contains("bogus"),
        "error should mention the invalid value"
    );
}

#[test]
fn cache_retention_display_round_trip() {
    for variant in [
        CacheRetention::None,
        CacheRetention::Short,
        CacheRetention::Long,
    ] {
        let s = variant.to_string();
        let parsed: CacheRetention = s.parse().unwrap();
        assert_eq!(parsed, variant, "round-trip failed for {s}");
    }
}
