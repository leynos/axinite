//! Unit tests for document text extraction helpers.

use super::office::{parse_xlsx_shared_strings, strip_xml_tags};
use super::*;

#[test]
fn strip_xml_basic() {
    let xml = "<root><p>Hello</p><p>World</p></root>";
    assert_eq!(strip_xml_tags(xml), "Hello World");
}

#[test]
fn strip_xml_entities() {
    let xml = "<t>A &amp; B &lt; C</t>";
    assert_eq!(strip_xml_tags(xml), "A & B < C");
}

#[test]
fn extract_utf8_valid() {
    assert_eq!(extract_utf8(b"hello").unwrap(), "hello");
}

#[test]
fn extract_utf8_lossy() {
    let data = b"hello \xff world";
    let result = extract_utf8(data).unwrap();
    assert!(result.contains("hello"));
    assert!(result.contains("world"));
}

#[test]
fn extract_by_extension_txt() {
    let result = try_extract_by_extension(b"content", Some("notes.txt"));
    assert_eq!(result, Some("content".to_string()));
}

#[test]
fn extract_by_extension_unknown() {
    let result = try_extract_by_extension(b"data", Some("file.xyz"));
    assert!(result.is_none());
}

#[test]
fn extract_by_extension_no_filename() {
    let result = try_extract_by_extension(b"data", None);
    assert!(result.is_none());
}

#[test]
fn rtf_basic_extraction() {
    let rtf = br"{\rtf1\ansi Hello World\par Second line}";
    let result = extract_rtf(rtf).unwrap();
    assert!(result.contains("Hello World"));
    assert!(result.contains("Second line"));
}

#[test]
fn xlsx_shared_strings_parsing() {
    let xml = r#"<sst><si><t>Name</t></si><si><t>Age</t></si></sst>"#;
    let strings = parse_xlsx_shared_strings(xml);
    assert_eq!(strings, vec!["Name", "Age"]);
}
