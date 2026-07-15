//! Code-aware stripping of reasoning/final tags and string-based stripping
//! of XML and pipe-delimited tool tags from LLM response text.

use super::{
    CodeRegion, FINAL_TAG_RE, PIPE_REASONING_TAG_RE, THINKING_TAG_RE, find_code_regions,
    is_inside_code,
};

/// Result of scanning a tag-pair regex over text: the surviving text, the
/// index where the unscanned tail begins, and whether the scan ended inside
/// an unclosed opening tag.
struct TagScan {
    result: String,
    tail_start: usize,
    in_tag: bool,
}

impl TagScan {
    /// Start a scan over `text` with capacity reserved for the survivors.
    fn new(text: &str) -> Self {
        Self {
            result: String::with_capacity(text.len()),
            tail_start: 0,
            in_tag: false,
        }
    }

    /// Advance the scan over one non-code tag match: outside a tag pair,
    /// keep the preceding text and enter on an opening tag; inside, leave
    /// on a closing tag. Text between the pair is dropped.
    fn step(&mut self, text: &str, re: &regex::Regex, m: &regex::Match<'_>) {
        let is_close = tag_is_close(re, &text[m.start()..]);
        if !self.in_tag {
            self.result.push_str(&text[self.tail_start..m.start()]);
            self.in_tag = !is_close;
        } else if is_close {
            self.in_tag = false;
        }
        self.tail_start = m.end();
    }
}

/// Whether the regex match at the start of `at` is a closing tag (its first
/// capture group is "/").
fn tag_is_close(re: &regex::Regex, at: &str) -> bool {
    re.captures(at)
        .and_then(|c| c.get(1))
        .is_some_and(|g| g.as_str() == "/")
}

/// Drop the regions between open/close tag pairs (outside code regions),
/// returning the surviving text and the state at end of scan.
fn strip_tag_pairs(text: &str, re: &regex::Regex, code_regions: &[CodeRegion]) -> TagScan {
    let mut scan = TagScan::new(text);
    for m in re.find_iter(text) {
        if is_inside_code(m.start(), code_regions) {
            continue;
        }
        scan.step(text, re, &m);
    }
    scan
}

/// Strip thinking/reasoning tags using regex, respecting code regions.
///
/// Strict mode: an unclosed opening tag discards all trailing text after it.
pub(super) fn strip_thinking_tags_regex(text: &str, code_regions: &[CodeRegion]) -> String {
    // Fallback: with no usable regex, leave the text unstripped.
    let Some(thinking_tag_re) = THINKING_TAG_RE.as_ref() else {
        return text.to_string();
    };
    let mut scan = strip_tag_pairs(text, thinking_tag_re, code_regions);

    // Strict mode: if still inside an unclosed thinking tag, discard trailing text
    // BUT preserve any <final> block embedded in the discarded region
    let trailing = &text[scan.tail_start..];
    if !scan.in_tag {
        scan.result.push_str(trailing);
    } else {
        let trailing_regions = find_code_regions(trailing);
        if let Some(final_content) = extract_final_content(trailing, &trailing_regions) {
            scan.result.push_str(&final_content);
        }
    }

    scan.result
}

/// Extract content inside `<final>` tags. Returns `None` if no non-code `<final>` tags found.
///
/// When `<final>` tags are present, ONLY content inside them reaches the user.
/// This discards any untagged reasoning that leaked outside `<think>` tags.
pub(super) fn extract_final_content(text: &str, code_regions: &[CodeRegion]) -> Option<String> {
    // Fallback: with no usable regex, report no <final> tags.
    let final_tag_re = FINAL_TAG_RE.as_ref()?;
    let mut scan = FinalScan::default();

    for m in final_tag_re.find_iter(text) {
        if is_inside_code(m.start(), code_regions) {
            continue;
        }
        scan.step(text, final_tag_re, &m);
    }

    scan.finish(text)
}

/// Scanner state while collecting the content of `<final>...</final>` pairs.
#[derive(Default)]
struct FinalScan<'a> {
    parts: Vec<&'a str>,
    in_final: bool,
    last_index: usize,
    found_any: bool,
}

impl<'a> FinalScan<'a> {
    /// Advance the scan over one non-code tag match: an opening `<final>`
    /// starts collecting; a matching `</final>` captures the enclosed text.
    /// Mismatched tags (a close with no open, or a nested open) are ignored.
    fn step(&mut self, text: &'a str, re: &regex::Regex, m: &regex::Match<'_>) {
        let is_close = tag_is_close(re, &text[m.start()..]);
        if !self.in_final && !is_close {
            // Opening <final>
            self.in_final = true;
            self.found_any = true;
            self.last_index = m.end();
        } else if self.in_final && is_close {
            // Closing </final>
            self.parts.push(&text[self.last_index..m.start()]);
            self.in_final = false;
            self.last_index = m.end();
        }
    }

    /// Join the collected parts, treating an unclosed `<final>` as running
    /// to the end of `text`. Returns `None` if no `<final>` tag was seen.
    fn finish(mut self, text: &'a str) -> Option<String> {
        if !self.found_any {
            return None;
        }
        if self.in_final {
            self.parts.push(&text[self.last_index..]);
        }
        Some(self.parts.join(""))
    }
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
    let mut scan = strip_tag_pairs(text, pipe_tag_re, &code_regions);

    // An unclosed opening tag discards all trailing text after it.
    if !scan.in_tag {
        scan.result.push_str(&text[scan.tail_start..]);
    }

    scan.result
}

/// Strip `<tag>...</tag>` and `<tag ...>...</tag>` blocks from text.
/// Used for tool tags only (no code-awareness needed).
// LLM response text and a tag name are free-form strings with no invariant a newtype could enforce.
// @codescene(disable:"String Heavy Function Arguments")
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
