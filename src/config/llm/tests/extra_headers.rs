//! Unit tests for `LLM_EXTRA_HEADERS` parsing.

use super::super::*;

#[test]
fn test_extra_headers_parsed() {
    let result = parse_extra_headers("HTTP-Referer:https://myapp.com,X-Title:MyApp").unwrap();
    assert_eq!(
        result,
        vec![
            ("HTTP-Referer".to_string(), "https://myapp.com".to_string()),
            ("X-Title".to_string(), "MyApp".to_string()),
        ]
    );
}

#[test]
fn test_extra_headers_empty_string() {
    let result = parse_extra_headers("").unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_extra_headers_whitespace_only() {
    let result = parse_extra_headers("  ").unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_extra_headers_malformed() {
    let result = parse_extra_headers("NoColonHere");
    assert!(result.is_err());
}

#[test]
fn test_extra_headers_empty_key() {
    let result = parse_extra_headers(":value");
    assert!(result.is_err());
}

#[test]
fn test_extra_headers_value_with_colons() {
    let result = parse_extra_headers("Authorization:Bearer abc:def").unwrap();
    assert_eq!(
        result,
        vec![("Authorization".to_string(), "Bearer abc:def".to_string())]
    );
}

#[test]
fn test_extra_headers_trailing_comma() {
    let result = parse_extra_headers("X-Title:MyApp,").unwrap();
    assert_eq!(result, vec![("X-Title".to_string(), "MyApp".to_string())]);
}

#[test]
fn test_extra_headers_with_spaces() {
    let result =
        parse_extra_headers(" HTTP-Referer : https://myapp.com , X-Title : MyApp ").unwrap();
    assert_eq!(
        result,
        vec![
            ("HTTP-Referer".to_string(), "https://myapp.com".to_string()),
            ("X-Title".to_string(), "MyApp".to_string()),
        ]
    );
}
