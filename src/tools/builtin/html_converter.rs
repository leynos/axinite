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

        let already_present = normalized_markdown.contains(caption.as_str())
            || inserted_captions.contains(caption.as_str());
        if caption.is_empty() || already_present {
            continue;
        }

        let next_heading = blocks[index + 1..].iter().find_map(|block| match block {
            DocumentBlock::Heading(heading) => Some(heading),
            DocumentBlock::FigureCaption(_) => None,
        });

        if insert_caption(&mut restored, caption, next_heading) {
            inserted_captions.insert(caption.as_str());
        }
    }

    restored
}

/// Insert `caption` before `next_heading` in `restored`, or append it when no
/// heading follows (so trailing captions are kept rather than dropped).
///
/// Returns whether the caption was inserted.
#[cfg(feature = "html-to-markdown")]
fn insert_caption(restored: &mut String, caption: &str, next_heading: Option<&String>) -> bool {
    let Some(next_heading) = next_heading else {
        while restored.ends_with('\n') {
            restored.pop();
        }
        restored.push_str("\n\n");
        restored.push_str(caption);
        restored.push('\n');
        return true;
    };

    match find_markdown_heading(restored, next_heading) {
        Some(position) => {
            restored.insert_str(position, &format!("{caption}\n"));
            true
        }
        None => false,
    }
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
mod tests;
