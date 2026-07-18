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
