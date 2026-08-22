//! Integration tests for HTML-to-Markdown conversion.
//!
//! For each directory in tests/test-pages/, loads source.html, runs the converter,
//! and optionally verifies against expected.md and metadata.json (contains).
//! Run with: cargo test --test html_to_markdown -- --nocapture

use std::path::Path;

use proptest::prelude::*;

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

#[derive(Clone, Copy)]
struct FenceDelimiter {
    character: char,
    length: usize,
}

impl FenceDelimiter {
    fn from_line(line: &str) -> Option<Self> {
        let character = line.chars().next()?;
        if !matches!(character, '`' | '~') {
            return None;
        }

        let length = line
            .chars()
            .take_while(|&marker| marker == character)
            .count();
        (length >= 3).then_some(Self { character, length })
    }

    fn closes(self, candidate: Self) -> bool {
        self.character == candidate.character && candidate.length >= self.length
    }
}

#[derive(Default)]
struct NormalizationState {
    lines: Vec<String>,
    paragraph: String,
    fence: Option<FenceDelimiter>,
}

impl NormalizationState {
    fn push_line(&mut self, line: &str) {
        let structural_line = line.trim_start();
        if self.push_fence_line(line, structural_line) {
            return;
        }
        if self.push_fenced_line(line) {
            return;
        }
        if structural_line.is_empty() {
            self.push_blank_line();
            return;
        }
        if is_markdown_block_line(structural_line) {
            self.push_markdown_block_line(line);
            return;
        }
        if self.extends_previous_list_item() {
            self.push_list_continuation(structural_line.trim());
            return;
        }
        self.push_paragraph_text(trim_paragraph_line(structural_line));
    }

    fn push_fence_line(&mut self, line: &str, structural_line: &str) -> bool {
        let Some(candidate) = FenceDelimiter::from_line(structural_line) else {
            return false;
        };
        if self.fence.is_some_and(|opener| !opener.closes(candidate)) {
            return false;
        }

        push_paragraph(&mut self.lines, &mut self.paragraph);
        self.lines.push(line.to_string());
        self.fence = self.fence.map_or(Some(candidate), |_| None);
        true
    }

    fn push_fenced_line(&mut self, line: &str) -> bool {
        if self.fence.is_some() {
            self.lines.push(line.to_string());
            true
        } else {
            false
        }
    }

    fn extends_previous_list_item(&self) -> bool {
        self.paragraph.is_empty()
            && self
                .lines
                .last()
                .is_some_and(|previous| is_markdown_list_item(previous))
    }

    fn push_blank_line(&mut self) {
        push_paragraph(&mut self.lines, &mut self.paragraph);
        if self
            .lines
            .last()
            .is_some_and(|previous| !previous.is_empty())
        {
            self.lines.push(String::new());
        }
    }

    fn push_markdown_block_line(&mut self, line: &str) {
        push_paragraph(&mut self.lines, &mut self.paragraph);
        self.lines.push(line.to_string());
    }

    fn push_list_continuation(&mut self, line: &str) {
        if let Some(previous) = self.lines.last_mut() {
            previous.push(' ');
            previous.push_str(line);
        }
    }

    fn push_paragraph_text(&mut self, line: &str) {
        if needs_soft_wrap_separator(&self.paragraph, line) {
            self.paragraph.push(' ');
        }
        self.paragraph.push_str(line);
        if is_markdown_hard_break(line) {
            push_paragraph(&mut self.lines, &mut self.paragraph);
        }
    }
}

fn normalize(s: &str) -> String {
    let s = s.replace("\r\n", "\n");
    let s = s.trim_matches('\n');

    let mut state = NormalizationState::default();
    for line in s.lines() {
        state.push_line(line);
    }
    push_paragraph(&mut state.lines, &mut state.paragraph);

    state.lines.join("\n")
}

fn needs_soft_wrap_separator(paragraph: &str, line: &str) -> bool {
    !paragraph.is_empty() && !(paragraph.ends_with('"') && line.starts_with('['))
}

fn trim_paragraph_line(line: &str) -> &str {
    if is_markdown_hard_break(line) {
        line
    } else {
        line.trim_end()
    }
}

fn is_markdown_hard_break(line: &str) -> bool {
    line.ends_with("  ")
        || line
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&character| character == b'\\')
            .count()
            % 2
            == 1
}

fn push_paragraph(lines: &mut Vec<String>, paragraph: &mut String) {
    if !paragraph.is_empty() {
        lines.push(std::mem::take(paragraph));
    }
}

fn is_markdown_block_line(line: &str) -> bool {
    matches!(line.chars().next(), Some('#' | '>' | '|'))
        || is_markdown_list_item(line)
        || ["---", "***"].iter().any(|prefix| line.starts_with(prefix))
}

fn is_markdown_list_item(line: &str) -> bool {
    ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        || line.split_once(". ").is_some_and(|(prefix, _)| {
            !prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit())
        })
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

#[test]
fn normalize_preserves_fenced_code_line_breaks() {
    let input = "before\n\n```rust\nlet one = 1;\nlet two = 2;\n```\n\nafter";
    let expected = "before\n\n```rust\nlet one = 1;\nlet two = 2;\n```\n\nafter";

    assert_eq!(normalize(input), expected);
}

#[test]
fn normalize_preserves_indented_fenced_code() {
    let input = concat!(
        "before\n\n",
        "  ```rust  \n",
        "\tlet one = 1;  \n",
        "    let two = 2;\n",
        "  ```  \n\n",
        "after",
    );

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_preserves_indented_fence_at_input_start() {
    let input = "  ```rust\n    let value = 1;\n  ```";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_preserves_nested_list_indentation() {
    let input = "- outer\n  - nested\n    - deeper\n- next";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_preserves_markdown_hard_breaks() {
    let input = "first line  \nsecond line";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_preserves_terminal_hard_break_spaces() {
    let input = "final line  ";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_preserves_unescaped_backslash_hard_breaks() {
    let input = "first\\\nsecond";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_keeps_mismatched_fence_markers_as_content() {
    let input = "```\n~~~\n```";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_keeps_shorter_fence_markers_as_content() {
    let input = "````\n```\n````";

    assert_eq!(normalize(input), input);
}

#[test]
fn normalize_joins_wrapped_list_item_continuations() {
    let input = "- first line\ncontinuation\n- second item";
    let expected = "- first line continuation\n- second item";

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
