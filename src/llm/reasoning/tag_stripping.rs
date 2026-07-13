//! Code-aware stripping of reasoning/final tags and string-based stripping
//! of XML and pipe-delimited tool tags from LLM response text.

use super::{
    CodeRegion, FINAL_TAG_RE, PIPE_REASONING_TAG_RE, THINKING_TAG_RE, find_code_regions,
    is_inside_code,
};

/// Strip thinking/reasoning tags using regex, respecting code regions.
///
/// Strict mode: an unclosed opening tag discards all trailing text after it.
pub(super) fn strip_thinking_tags_regex(text: &str, code_regions: &[CodeRegion]) -> String {
    // Fallback: with no usable regex, leave the text unstripped.
    let Some(thinking_tag_re) = THINKING_TAG_RE.as_ref() else {
        return text.to_string();
    };
    let mut result = String::with_capacity(text.len());
    let mut last_index = 0;
    let mut in_thinking = false;

    for m in thinking_tag_re.find_iter(text) {
        let idx = m.start();

        if is_inside_code(idx, code_regions) {
            continue;
        }

        // Check if this is a close tag by looking at capture group
        let caps = thinking_tag_re.captures(&text[idx..]);
        let is_close = caps
            .and_then(|c| c.get(1))
            .is_some_and(|g| g.as_str() == "/");

        if !in_thinking {
            // Append text before this tag
            result.push_str(&text[last_index..idx]);
            if !is_close {
                in_thinking = true;
            }
        } else if is_close {
            in_thinking = false;
        }

        last_index = m.end();
    }

    // Strict mode: if still inside an unclosed thinking tag, discard trailing text
    // BUT preserve any <final> block embedded in the discarded region
    if !in_thinking {
        result.push_str(&text[last_index..]);
    } else {
        let trailing = &text[last_index..];
        let trailing_regions = find_code_regions(trailing);
        if let Some(final_content) = extract_final_content(trailing, &trailing_regions) {
            result.push_str(&final_content);
        }
    }

    result
}

/// Extract content inside `<final>` tags. Returns `None` if no non-code `<final>` tags found.
///
/// When `<final>` tags are present, ONLY content inside them reaches the user.
/// This discards any untagged reasoning that leaked outside `<think>` tags.
pub(super) fn extract_final_content(text: &str, code_regions: &[CodeRegion]) -> Option<String> {
    // Fallback: with no usable regex, report no <final> tags.
    let final_tag_re = FINAL_TAG_RE.as_ref()?;
    let mut parts: Vec<&str> = Vec::new();
    let mut in_final = false;
    let mut last_index = 0;
    let mut found_any = false;

    for m in final_tag_re.find_iter(text) {
        let idx = m.start();

        if is_inside_code(idx, code_regions) {
            continue;
        }

        let caps = final_tag_re.captures(&text[idx..]);
        let is_close = caps
            .and_then(|c| c.get(1))
            .is_some_and(|g| g.as_str() == "/");

        if !in_final && !is_close {
            // Opening <final>
            in_final = true;
            found_any = true;
            last_index = m.end();
        } else if in_final && is_close {
            // Closing </final>
            parts.push(&text[last_index..idx]);
            in_final = false;
            last_index = m.end();
        }
    }

    if !found_any {
        return None;
    }

    // Unclosed <final> — include trailing content
    if in_final {
        parts.push(&text[last_index..]);
    }

    Some(parts.join(""))
}

/// Strip pipe-delimited reasoning tags, respecting code regions.
pub(super) fn strip_pipe_reasoning_tags(text: &str) -> String {
    // Fallback: with no usable regex, leave the text unstripped.
    let Some(pipe_tag_re) = PIPE_REASONING_TAG_RE.as_ref() else {
        return text.to_string();
    };
    if !pipe_tag_re.is_match(text) {
        return text.to_string();
    }

    let code_regions = find_code_regions(text);
    let mut result = String::with_capacity(text.len());
    let mut last_index = 0;
    let mut in_tag = false;

    for m in pipe_tag_re.find_iter(text) {
        let idx = m.start();

        if is_inside_code(idx, &code_regions) {
            continue;
        }

        let caps = pipe_tag_re.captures(&text[idx..]);
        let is_close = caps
            .and_then(|c| c.get(1))
            .is_some_and(|g| g.as_str() == "/");

        if !in_tag {
            result.push_str(&text[last_index..idx]);
            if !is_close {
                in_tag = true;
            }
        } else if is_close {
            in_tag = false;
        }

        last_index = m.end();
    }

    if !in_tag {
        result.push_str(&text[last_index..]);
    }

    result
}

/// Strip `<tag>...</tag>` and `<tag ...>...</tag>` blocks from text.
/// Used for tool tags only (no code-awareness needed).
pub(super) fn strip_xml_tag(text: &str, tag: &str) -> String {
    let open_exact = format!("<{}>", tag);
    let open_prefix = format!("<{} ", tag); // for <tag attr="...">
    let close = format!("</{}>", tag);

    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    loop {
        // Find the next opening tag (exact or with attributes)
        let exact_pos = remaining.find(&open_exact);
        let prefix_pos = remaining.find(&open_prefix);
        let start = match (exact_pos, prefix_pos) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };

        // Add everything before the tag
        result.push_str(&remaining[..start]);

        // Find the end of the opening tag (the closing >)
        let after_open = &remaining[start..];
        let open_end = match after_open.find('>') {
            Some(pos) => start + pos + 1,
            None => break, // malformed, stop
        };

        // Find the closing tag
        if let Some(close_offset) = remaining[open_end..].find(&close) {
            let end = open_end + close_offset + close.len();
            remaining = &remaining[end..];
        } else {
            // No closing tag, discard from here (malformed)
            remaining = "";
            break;
        }
    }

    result.push_str(remaining);
    result
}

/// Strip `<|tag|>...<|/tag|>` pipe-delimited blocks from text.
/// Used for tool tags only (no code-awareness needed).
pub(super) fn strip_pipe_tag(text: &str, tag: &str) -> String {
    let open = format!("<|{}|>", tag);
    let close = format!("<|/{}|>", tag);

    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(start) = remaining.find(&open) {
        result.push_str(&remaining[..start]);

        if let Some(close_offset) = remaining[start..].find(&close) {
            let end = start + close_offset + close.len();
            remaining = &remaining[end..];
        } else {
            remaining = "";
            break;
        }
    }

    result.push_str(remaining);
    result
}
