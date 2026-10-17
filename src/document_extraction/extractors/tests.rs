//! Unit tests for document text extraction helpers.

use super::office::{parse_xlsx_shared_strings, strip_xml_tags};
use super::*;

#[rstest::rstest]
#[case::slides("ppt/slides/slideé.xml", office::extract_pptx)]
#[case::sheets("xl/worksheets/sheeté.xml", office::extract_xlsx)]
fn office_archives_preserve_unicode_entry_names(
    #[case] entry_name: &str,
    #[case] extract: fn(&[u8]) -> Result<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer.start_file(entry_name, zip::write::SimpleFileOptions::default())?;
    writer.write_all(b"<row><c><v>Unicode document</v></c></row>")?;
    let archive = writer.finish()?.into_inner();

    assert_eq!(extract(&archive)?, "Unicode document");
    Ok(())
}

/// Preserve decoded entry order and usable text beside an unreadable entry.
#[rstest::rstest]
#[case::pptx(
    "ppt/slides/slideZ.xml",
    "ppt/slides/slideä.xml",
    "\n\n---\n\n",
    office::extract_pptx
)]
#[case::xlsx(
    "xl/worksheets/sheetZ.xml",
    "xl/worksheets/sheetä.xml",
    "\n\n",
    office::extract_xlsx
)]
fn office_archives_preserve_decoded_order_and_readable_partial_text(
    #[case] seed_path: &str,
    #[case] utf8_path: &str,
    #[case] separator: &str,
    #[case] extract: fn(&[u8]) -> Result<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer.start_file(seed_path, zip::write::SimpleFileOptions::default())?;
    writer.write_all(b"<row><c><v>Legacy document</v></c></row>")?;
    writer.start_file(utf8_path, zip::write::SimpleFileOptions::default())?;
    writer.write_all(b"<row><c><v>UTF-8 document</v></c></row>")?;
    let unreadable_path = seed_path.replace('Z', "0");
    writer.start_file(
        unreadable_path.as_str(),
        zip::write::SimpleFileOptions::default(),
    )?;
    writer.write_all(&[0xff])?;
    let mut archive = writer.finish()?.into_inner();

    // ZipWriter accepts UTF-8 paths only. Equal-length edits produce CP437 names
    // without changing header offsets or the content CRCs.
    let seed_path_bytes = seed_path.as_bytes();
    let cp437_byte_offset = seed_path_bytes
        .iter()
        .position(|byte| *byte == b'Z')
        .expect("seed path should contain its Z marker");
    let path_offsets: Vec<usize> = archive
        .windows(seed_path_bytes.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == seed_path_bytes).then_some(offset))
        .collect();
    assert_eq!(
        path_offsets.len(),
        2,
        "seed path should appear in the local and central directory headers",
    );
    for offset in path_offsets {
        archive[offset + cp437_byte_offset] = 0x82;
    }

    let mut decoded_archive = zip::ZipArchive::new(std::io::Cursor::new(archive.as_slice()))?;
    let decoded_name = decoded_archive.by_index(0)?.name()?.into_owned();
    assert_eq!(decoded_name, seed_path.replace('Z', "é"));

    assert_eq!(
        extract(&archive)?,
        format!("UTF-8 document{separator}Legacy document"),
    );
    Ok(())
}

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
