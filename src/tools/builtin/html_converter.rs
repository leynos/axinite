//! HTML to Markdown conversion for HTTP responses.
//!
//! Two-stage pipeline: readability (extract article) -> html-to-markdown-rs (convert to md).
//! When the `html-to-markdown` feature is disabled, passthrough only.

use crate::tools::tool::ToolError;

#[cfg(feature = "html-to-markdown")]
use html_to_markdown_rs::convert;
#[cfg(feature = "html-to-markdown")]
use kuchiki::traits::*;
#[cfg(feature = "html-to-markdown")]
use readability_js::Readability;

#[cfg(not(feature = "html-to-markdown"))]
pub fn convert_html_to_markdown(html: &str, _url: &str) -> Result<String, ToolError> {
    Ok(html.to_string())
}

#[cfg(feature = "html-to-markdown")]
pub fn convert_html_to_markdown(html: &str, url: &str) -> Result<String, ToolError> {
    let readability = Readability::new()
        .map_err(|e| ToolError::ExecutionFailed(format!("readability parser: {e}")))?;

    let article = readability.parse_with_url(html, url).map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to extract article content: {e}"))
    })?;

    let clean_html = remove_embedded_media(&article.content);

    let markdown = convert(&clean_html, None)
        .map_err(|e| ToolError::ExecutionFailed(format!("HTML to markdown: {}", e)))?;

    // Parse the original document once and share it between the two restore
    // passes below, rather than re-parsing the same raw html twice.
    let document = kuchiki::parse_html().one(html);
    let markdown = restore_intro_heading(&document, &article.title, &markdown);
    let markdown = restore_missing_figure_captions(&document, &markdown);

    Ok(markdown.replace("*[*", "* [*"))
}

#[cfg(feature = "html-to-markdown")]
fn remove_embedded_media(html: &str) -> String {
    let document = kuchiki::parse_html().one(html);
    for selector in [
        "figure.canvas-image",
        "figure[data-type=\"image\"]",
        "audio",
        "iframe",
        "img",
        "picture",
        "source",
        "video",
    ] {
        if let Ok(matches) = document.select(selector) {
            let nodes: Vec<_> = matches.map(|node| node.as_node().clone()).collect();
            for node in nodes {
                node.detach();
            }
        }
    }
    remove_exact_text_elements(&document, &["Powered by SmartAsset.com"]);
    document.to_string()
}

#[cfg(feature = "html-to-markdown")]
fn remove_exact_text_elements(document: &kuchiki::NodeRef, text_values: &[&str]) {
    let nodes: Vec<_> = document
        .descendants()
        .filter(|node| {
            let Some(element) = node.as_element() else {
                return false;
            };

            if !matches!(element.name.local.as_ref(), "div" | "span" | "p") {
                return false;
            }

            let text = normalize_block_text(&node.text_contents());
            text_values.contains(&text.as_str())
        })
        .collect();

    for node in nodes {
        node.detach();
    }
}

#[cfg(feature = "html-to-markdown")]
fn restore_intro_heading(
    document: &kuchiki::NodeRef,
    article_title: &str,
    markdown: &str,
) -> String {
    if let Some((level, heading)) =
        intro_heading_removed_as_title(document, article_title, markdown)
    {
        let hashes = "#".repeat(level);
        format!("{hashes} {heading}\n\n{markdown}")
    } else {
        markdown.to_string()
    }
}

#[cfg(feature = "html-to-markdown")]
fn intro_heading_removed_as_title(
    document: &kuchiki::NodeRef,
    article_title: &str,
    markdown: &str,
) -> Option<(usize, String)> {
    let article_title = normalize_heading_text(article_title);

    for selector in ["h1", "h2"] {
        let matches = document.select(selector).ok()?;
        for node in matches {
            let heading = node.text_contents().trim().to_string();
            let normalized_heading = normalize_heading_text(&heading);
            if is_redundant_heading(&normalized_heading, &article_title, &heading, markdown) {
                continue;
            }

            if !article_title.starts_with(&normalized_heading) {
                continue;
            }

            return Some((heading_level_for_selector(selector), heading));
        }
    }

    None
}

#[cfg(feature = "html-to-markdown")]
fn is_redundant_heading(
    normalized_heading: &str,
    article_title: &str,
    heading: &str,
    markdown: &str,
) -> bool {
    normalized_heading.is_empty()
        || normalized_heading == article_title
        || markdown.contains(heading)
}

#[cfg(feature = "html-to-markdown")]
fn heading_level_for_selector(selector: &str) -> usize {
    if selector == "h1" { 1 } else { 2 }
}

#[cfg(feature = "html-to-markdown")]
fn normalize_heading_text(text: &str) -> String {
    text.trim()
        .trim_end_matches(':')
        .strip_prefix("The ")
        .unwrap_or(text.trim().trim_end_matches(':'))
        .to_lowercase()
}

#[cfg(feature = "html-to-markdown")]
fn restore_missing_figure_captions(document: &kuchiki::NodeRef, markdown: &str) -> String {
    let blocks = document_blocks(document);
    let mut restored = markdown.to_string();

    // Normalize the original markdown once, up front. `restored` only ever
    // grows by inserting caption text verbatim, so membership in the
    // original document plus a record of captions we've inserted ourselves
    // is equivalent to (and far cheaper than) re-normalizing the whole,
    // growing `restored` string on every iteration.
    let normalized_markdown = normalize_block_text(markdown);
    let mut inserted_captions: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (index, block) in blocks.iter().enumerate() {
        let DocumentBlock::FigureCaption(caption) = block else {
            continue;
        };

        if caption.is_empty()
            || normalized_markdown.contains(caption.as_str())
            || inserted_captions.contains(caption.as_str())
        {
            continue;
        }

        let next_heading = blocks[index + 1..].iter().find_map(|block| match block {
            DocumentBlock::Heading(heading) => Some(heading),
            DocumentBlock::FigureCaption(_) => None,
        });

        match next_heading {
            Some(next_heading) => {
                if let Some(position) = find_markdown_heading(&restored, next_heading) {
                    restored.insert_str(position, &format!("{caption}\n"));
                    inserted_captions.insert(caption.as_str());
                }
            }
            // No heading follows this caption, so there's nowhere to insert it
            // before; append it to the end instead of silently dropping it.
            None => {
                while restored.ends_with('\n') {
                    restored.pop();
                }
                restored.push_str("\n\n");
                restored.push_str(caption);
                restored.push('\n');
                inserted_captions.insert(caption.as_str());
            }
        }
    }

    restored
}

#[cfg(feature = "html-to-markdown")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentBlock {
    Heading(String),
    FigureCaption(String),
}

#[cfg(feature = "html-to-markdown")]
fn document_blocks(document: &kuchiki::NodeRef) -> Vec<DocumentBlock> {
    document
        .descendants()
        .filter_map(|node| {
            let element = node.as_element()?;
            match element.name.local.as_ref() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(DocumentBlock::Heading(
                    normalize_block_text(&node.text_contents()),
                )),
                "figcaption" if !is_removed_media_caption(&node) => Some(
                    DocumentBlock::FigureCaption(normalize_block_text(&node.text_contents())),
                ),
                _ => None,
            }
        })
        .collect()
}

#[cfg(feature = "html-to-markdown")]
fn is_removed_media_caption(node: &kuchiki::NodeRef) -> bool {
    node.ancestors().any(|ancestor| {
        let Some(element) = ancestor.as_element() else {
            return false;
        };

        if element.name.local.as_ref() != "figure" {
            return false;
        }

        let attributes = element.attributes.borrow();
        attributes.get("data-type") == Some("image")
            || attributes
                .get("class")
                .is_some_and(|class| class.split_whitespace().any(|name| name == "canvas-image"))
    })
}

#[cfg(feature = "html-to-markdown")]
fn normalize_block_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(feature = "html-to-markdown")]
fn find_markdown_heading(markdown: &str, heading: &str) -> Option<usize> {
    let mut offset = 0;
    for line in markdown.lines() {
        if line.trim_start_matches('#').trim() == heading {
            return Some(offset);
        }
        offset += line.len() + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    //! Unit tests for HTML-to-Markdown conversion and its passthrough.

    use super::*;

    #[cfg(not(feature = "html-to-markdown"))]
    #[test]
    fn passthrough_returns_input_unchanged_when_feature_disabled() {
        {
            let html = "<html><body>raw</body></html>";
            let out = convert_html_to_markdown(html, "https://example.com/").unwrap();
            assert_eq!(out, html);
        }
    }

    #[cfg(not(feature = "html-to-markdown"))]
    #[test]
    fn passthrough_ignores_url_when_feature_disabled() {
        {
            let html = "anything";
            let _ = convert_html_to_markdown(html, "").unwrap();
            let _ = convert_html_to_markdown(html, "https://example.com/page").unwrap();
        }
    }

    #[cfg(feature = "html-to-markdown")]
    #[test]
    fn simple_article_extracted_and_converted_to_markdown() {
        // Readability needs enough content (default char_threshold ~500) and clear main content.
        let html = r#"<!DOCTYPE html>
<html><head><title>Test</title></head><body>
<nav><a href="/">Home</a></nav>
<main>
  <article>
    <h1>Test Title</h1>
    <p>First paragraph with enough text so that readability's scoring finds this as the main content block. We need to exceed the default character threshold.</p>
    <p>Second paragraph. More body text here to make the article clearly the dominant content area versus the short nav and footer.</p>
    <p>Third paragraph for good measure. The extraction algorithm scores candidates by paragraph count and text length; this block should win.</p>
  </article>
</main>
<footer><p>Footer</p></footer>
</body></html>"#;
        let out = convert_html_to_markdown(html, "https://example.com/article").unwrap();
        assert!(
            out.contains("Test Title"),
            "expected title in output: {}",
            out
        );
        assert!(
            out.contains("First paragraph"),
            "expected content in output: {}",
            out
        );
        assert!(
            out.contains("Second paragraph"),
            "expected content in output: {}",
            out
        );
        assert!(
            !out.contains("<article>"),
            "expected markdown, not raw HTML"
        );
    }

    #[cfg(feature = "html-to-markdown")]
    #[test]
    fn trailing_figure_caption_with_no_following_heading_is_appended() {
        // The figure's <img> is stripped by remove_embedded_media before
        // conversion, so the caption text only survives via
        // restore_missing_figure_captions. With no heading after the
        // caption, it must be appended rather than dropped.
        let html = r#"<!DOCTYPE html>
<html><head><title>Test</title></head><body>
<main>
  <article>
    <h1>Test Title</h1>
    <p>First paragraph with enough text so that readability's scoring finds this as the main content block. We need to exceed the default character threshold for extraction.</p>
    <p>Second paragraph. More body text here to make the article clearly the dominant content area versus the short nav and footer elements on the page.</p>
    <figure>
      <img src="chart.png" alt="chart">
      <figcaption>Trailing caption describing the final figure in the article.</figcaption>
    </figure>
  </article>
</main>
</body></html>"#;
        let out = convert_html_to_markdown(html, "https://example.com/article").unwrap();
        assert!(
            out.trim_end()
                .ends_with("Trailing caption describing the final figure in the article."),
            "expected trailing caption at end of output: {}",
            out
        );
    }

    #[cfg(feature = "html-to-markdown")]
    #[test]
    fn returns_execution_error_on_empty_html() {
        let result = convert_html_to_markdown("", "https://example.com/");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Execution failed") || msg.contains("extract") || msg.contains("content"),
            "{}",
            msg
        );
    }

    #[cfg(feature = "html-to-markdown")]
    #[test]
    fn returns_execution_error_on_plain_text_not_html() {
        let result = convert_html_to_markdown("not html at all", "https://example.com/");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Execution failed")
                || msg.contains("extract")
                || msg.contains("content")
                || msg.contains("parser"),
            "{}",
            msg
        );
    }
}
