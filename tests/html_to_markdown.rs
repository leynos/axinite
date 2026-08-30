//! Integration tests for HTML-to-Markdown conversion.
//!
//! For each directory in tests/test-pages/, loads source.html, runs the converter,
//! and optionally verifies against expected.md and metadata.json (contains).
//! Run with: cargo test --test html_to_markdown -- --nocapture

use std::path::Path;

use proptest::prelude::*;
use rstest::rstest;

#[path = "support/markdown_normalization.rs"]
mod markdown_normalization;

use markdown_normalization::normalize;

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct PageMetadata {
    /// If false, skip golden-file comparison even when expected.md exists.
    check_expected: Option<bool>,
    /// Strings that must each appear in the converted markdown.
    contains: Option<Vec<String>>,
    /// Base URL for readability. If omitted, use default test-pages URL.
    url: Option<String>,
}

/// Normalize typographic/smart punctuation to ASCII so tests match converter output
/// regardless of apostrophe/quote variants (e.g. U+2019 ' → U+0027 ').
fn normalize_smart_punctuation(s: &str) -> String {
    s.replace(['\u{2019}', '\u{2018}'], "'")
        .replace(['\u{201C}', '\u{201D}'], "\"")
}

#[test]
fn normalize_ignores_soft_wrapping_without_merging_markdown_blocks() {
    let wrapped = concat!(
        "A paragraph split\nacross lines.\n\n",
        "A quote: \"\n[T]ext.\n\n",
        "## Heading\n\n- first\n- second",
    );
    let unwrapped = concat!(
        "A paragraph split across lines.\n\n",
        "A quote: \"[T]ext.\n\n",
        "## Heading\n\n- first\n- second",
    );

    assert_eq!(normalize(wrapped), normalize(unwrapped));
}

#[rstest]
#[case(
    "before\n\n```rust\nlet one = 1;\nlet two = 2;\n```\n\nafter",
    "before\n\n```rust\nlet one = 1;\nlet two = 2;\n```\n\nafter"
)]
#[case(
    concat!(
        "before\n\n",
        "  ```rust  \n",
        "\tlet one = 1;  \n",
        "    let two = 2;\n",
        "  ```  \n\n",
        "after",
    ),
    concat!(
        "before\n\n",
        "  ```rust  \n",
        "\tlet one = 1;  \n",
        "    let two = 2;\n",
        "  ```  \n\n",
        "after",
    )
)]
#[case(
    "  ```rust\n    let value = 1;\n  ```",
    "  ```rust\n    let value = 1;\n  ```"
)]
#[case(
    "- outer\n  - nested\n    - deeper\n- next",
    "- outer\n  - nested\n    - deeper\n- next"
)]
#[case(
    "- outer\n  - nested\ncontinuation",
    "- outer\n  - nested continuation"
)]
#[case("first line  \nsecond line", "first line  \nsecond line")]
#[case("final line  ", "final line  ")]
#[case("first\\\nsecond", "first\\\nsecond")]
#[case("```\n~~~\n```", "```\n~~~\n```")]
#[case("````\n```\n````", "````\n```\n````")]
#[case(
    "- first line\ncontinuation\n- second item",
    "- first line continuation\n- second item"
)]
#[case("text\n  \n\t", "text")]
fn normalize_matches_expected_markdown(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(normalize(input), expected);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn normalize_matches_generated_soft_wraps(words in prop::collection::vec("[a-z]{1,12}", 1..8)) {
        let input = words.join("\n");
        prop_assert_eq!(normalize(&input), words.join(" "));
    }

    #[test]
    fn normalize_preserves_generated_fenced_blocks(
        indentation in prop_oneof![Just(" "), Just("  "), Just("\t")],
        lines in prop::collection::vec("[a-zA-Z0-9 \\t]{0,24}", 0..8),
    ) {
        let input = format!(
            "before\n{indentation}```rust\n{}\n{indentation}```\nafter",
            lines.join("\n"),
        );
        prop_assert_eq!(normalize(&input), input);
    }

    #[test]
    fn normalize_joins_generated_list_continuations(
        item in "[a-z]{1,12}",
        continuation in "[a-z]{1,12}",
    ) {
        let input = format!("- {item}\n{continuation}");
        prop_assert_eq!(normalize(&input), format!("- {item} {continuation}"));
    }

    #[test]
    fn normalize_is_idempotent_for_generated_line_sequences(
        lines in prop::collection::vec(
            prop_oneof![
                Just(String::new()),
                Just(" \t".to_string()),
                Just("```".to_string()),
                Just("~~~".to_string()),
                Just("- item".to_string()),
                Just("  - nested".to_string()),
                "[a-z]{1,12}",
            ],
            0..16,
        ),
    ) {
        let normalized = normalize(&lines.join("\n"));
        prop_assert_eq!(normalize(&normalized), normalized);
    }
}

#[test]
fn convert_test_pages_to_markdown() {
    let test_pages = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test-pages");

    let entries =
        std::fs::read_dir(&test_pages).expect("test-pages directory not found or not readable");

    let mut converted = 0u32;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let source_html = path.join("source.html");
        if !source_html.is_file() {
            continue;
        }
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let default_url = format!("https://example.com/test-pages/{}/", dir_name);

        let metadata: PageMetadata = if path.join("metadata.json").is_file() {
            let raw =
                std::fs::read_to_string(path.join("metadata.json")).expect("read metadata.json");
            serde_json::from_str(&raw).expect("invalid metadata.json")
        } else {
            Default::default()
        };

        let url = metadata.url.as_deref().unwrap_or(&default_url).to_string();

        let html = std::fs::read_to_string(&source_html).expect("read source.html");
        let markdown = axinite::tools::builtin::convert_html_to_markdown(&html, &url)
            .expect("convert_html_to_markdown failed");

        let expected_md_path = path.join("expected.md");
        let should_check_expected =
            expected_md_path.is_file() && metadata.check_expected.unwrap_or(true);

        if should_check_expected {
            let expected = std::fs::read_to_string(&expected_md_path).expect("read expected.md");
            let norm_actual = normalize_smart_punctuation(&normalize(&markdown));
            let norm_expected = normalize_smart_punctuation(&normalize(&expected));
            assert_eq!(
                norm_actual, norm_expected,
                "markdown mismatch for {}:\n--- actual ---\n{}\n--- expected ---\n{}",
                dir_name, norm_actual, norm_expected
            );
        }

        if let Some(ref contains) = metadata.contains {
            let normalized_md = normalize_smart_punctuation(&markdown);
            for s in contains {
                assert!(
                    normalized_md.contains(&normalize_smart_punctuation(s)),
                    "{}: markdown missing expected content: {:?}",
                    dir_name,
                    s
                );
            }
        }

        if std::env::var("HTML_TO_MD_VERBOSE").is_ok() {
            println!("--- {} ---\n{}\n", dir_name, markdown);
        }
        converted += 1;
    }

    assert!(
        converted > 0,
        "No test pages found (no directories with source.html in tests/test-pages/)"
    );
}
