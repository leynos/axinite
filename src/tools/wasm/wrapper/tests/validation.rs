//! Tests for private-IP rejection and schema-based parameter coercion.

use super::super::coerce_params_to_schema;
use super::super::http::{is_private_ip, reject_private_ip};

#[test]
fn test_is_private_ip_v4() {
    use std::net::IpAddr;
    // Private ranges
    assert!(is_private_ip("127.0.0.1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("10.0.0.1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("172.16.0.1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("192.168.1.1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("169.254.1.1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("0.0.0.0".parse::<IpAddr>().unwrap()));
    // CGNAT
    assert!(is_private_ip("100.64.0.1".parse::<IpAddr>().unwrap()));

    // Public IPs
    assert!(!is_private_ip("8.8.8.8".parse::<IpAddr>().unwrap()));
    assert!(!is_private_ip("1.1.1.1".parse::<IpAddr>().unwrap()));
    assert!(!is_private_ip("93.184.216.34".parse::<IpAddr>().unwrap()));
}

#[test]
fn test_is_private_ip_v6() {
    use std::net::IpAddr;
    assert!(is_private_ip("::1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("::".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("fc00::1".parse::<IpAddr>().unwrap()));
    assert!(is_private_ip("fe80::1".parse::<IpAddr>().unwrap()));

    // Public
    assert!(!is_private_ip("2606:4700::1111".parse::<IpAddr>().unwrap()));
}

#[test]
fn test_reject_private_ip_loopback() {
    let result = reject_private_ip("https://127.0.0.1:8080/api");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("private/internal IP"));
}

#[test]
fn test_reject_private_ip_internal() {
    let result = reject_private_ip("https://192.168.1.1/admin");
    assert!(result.is_err());
}

#[test]
fn test_reject_private_ip_public_ok() {
    // 8.8.8.8 (Google DNS) is public
    let result = reject_private_ip("https://8.8.8.8/dns-query");
    assert!(result.is_ok());
}

#[test]
fn test_coerce_params_string_to_number() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "count": { "type": "number" },
            "name": { "type": "string" }
        }
    });
    let params = serde_json::json!({"count": "5", "name": "test"});
    let result = coerce_params_to_schema(params, &schema);
    assert_eq!(result["count"], serde_json::json!(5.0));
    assert_eq!(result["name"], serde_json::json!("test"));
}

#[test]
fn test_coerce_params_string_to_integer() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "limit": { "type": "integer" }
        }
    });
    let params = serde_json::json!({"limit": "10"});
    let result = coerce_params_to_schema(params, &schema);
    assert_eq!(result["limit"], serde_json::json!(10));
}

#[test]
fn test_coerce_params_string_to_boolean() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "a": { "type": "boolean" },
            "b": { "type": "boolean" },
            "c": { "type": "boolean" },
            "d": { "type": "boolean" }
        }
    });
    let params = serde_json::json!({
        "a": "true",
        "b": "false",
        "c": "True",
        "d": "FALSE"
    });
    let result = coerce_params_to_schema(params, &schema);
    assert_eq!(result["a"], serde_json::json!(true));
    assert_eq!(result["b"], serde_json::json!(false));
    assert_eq!(result["c"], serde_json::json!(true));
    assert_eq!(result["d"], serde_json::json!(false));
}

#[test]
fn test_coerce_params_already_correct_type() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "count": { "type": "number" }
        }
    });
    let params = serde_json::json!({"count": 5});
    let result = coerce_params_to_schema(params, &schema);
    assert_eq!(result["count"], serde_json::json!(5));
}

#[test]
fn test_coerce_params_invalid_string_not_coerced() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "count": { "type": "number" }
        }
    });
    let params = serde_json::json!({"count": "not-a-number"});
    let result = coerce_params_to_schema(params, &schema);
    // Should remain as string since it can't be parsed
    assert_eq!(result["count"], serde_json::json!("not-a-number"));
}
