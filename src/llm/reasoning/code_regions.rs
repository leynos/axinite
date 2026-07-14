//! Detection of code regions (fenced blocks and inline backtick spans) in
//! LLM response text so that tag stripping never touches code samples.

/// A byte range in the source text that is inside a code region (fenced or inline).
#[derive(Debug, Clone, Copy)]
pub(super) struct CodeRegion {
    pub(super) start: usize,
    pub(super) end: usize,
}

/// Detect fenced code blocks (``` and ~~~) and inline backtick spans.
/// Returns sorted `Vec<CodeRegion>` of byte ranges. Tags inside these ranges are
/// skipped during stripping so code examples mentioning `<thinking>` are preserved.
pub(super) fn find_code_regions(text: &str) -> Vec<CodeRegion> {
    let mut regions = find_fenced_regions(text);
    append_inline_regions(text, &mut regions);
    regions.sort_by_key(|r| r.start);
    regions
}

/// Return `true` for space or tab bytes (fence-line indentation).
fn is_space_or_tab(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Return `true` for the characters that can open a code fence.
fn is_fence_char(b: u8) -> bool {
    b == b'`' || b == b'~'
}

/// Advance past the next newline at or after `from`; `None` at end of input.
fn next_line_start(text: &str, from: usize) -> Option<usize> {
    text[from..].find('\n').map(|nl| from + nl + 1)
}

/// Parse an opening fence at a line start: optional indentation then a run
/// of 3+ identical backticks or tildes. Returns the fence character, the run
/// length, and the index just past the run.
fn parse_opening_fence(bytes: &[u8], line_start: usize) -> Option<(u8, usize, usize)> {
    // Skip optional leading whitespace
    let run_start = skip_spaces_and_tabs(bytes, line_start);
    if run_start >= bytes.len() || !is_fence_char(bytes[run_start]) {
        return None;
    }
    let fence_char = bytes[run_start];
    let run_end = fence_run_end(bytes, run_start, fence_char);
    let fence_len = run_end - run_start;
    if fence_len < 3 {
        return None;
    }
    Some((fence_char, fence_len, run_end))
}

/// Detect fenced code blocks: a line starting with 3+ backticks or tildes,
/// closed by a matching fence line (or extending to EOF when unclosed).
fn find_fenced_regions(text: &str) -> Vec<CodeRegion> {
    let mut regions = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // The scan is line-oriented: only look for a fence when `i` is at a
        // line start (i==0 or previous char is \n).
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        let opening = if at_line_start {
            parse_opening_fence(bytes, i)
        } else {
            None
        };

        let Some((fence_char, fence_len, run_end)) = opening else {
            // Not an opening fence line, skip to next line
            let Some(next) = next_line_start(text, i) else {
                break;
            };
            i = next;
            continue;
        };
        let line_start = i;

        // Skip rest of opening fence line (info string)
        let Some(next) = next_line_start(text, run_end) else {
            // Fence at EOF with no content — region extends to end
            regions.push(CodeRegion {
                start: line_start,
                end: bytes.len(),
            });
            break;
        };

        // Find closing fence (a line starting with >= fence_len of the same
        // char); an unclosed fence extends to EOF.
        let end = find_closing_fence(text, next, fence_char, fence_len).unwrap_or(bytes.len());
        regions.push(CodeRegion {
            start: line_start,
            end,
        });
        i = end;
    }
    regions
}

/// Scan forwards from `from`, line by line, for a closing fence line.
/// Returns the byte index just past the closing line, or `None` when the
/// fence is unclosed.
fn find_closing_fence(text: &str, from: usize, fence_char: u8, fence_len: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut line_start = from;
    while line_start < bytes.len() {
        if let Some(end) = closing_fence_line_end(bytes, line_start, fence_char, fence_len) {
            return Some(end);
        }
        // Not a closing fence, skip to next line
        line_start = next_line_start(text, line_start)?;
    }
    None
}

/// Check whether the line starting at `line_start` is a closing fence line:
/// optional indentation, at least `fence_len` repeats of `fence_char`, then
/// nothing but whitespace to the end of the line. Returns the byte index just
/// past the line's terminating newline (or EOF), or `None` when the line is
/// not a closing fence.
fn closing_fence_line_end(
    bytes: &[u8],
    line_start: usize,
    fence_char: u8,
    fence_len: usize,
) -> Option<usize> {
    // Skip optional leading whitespace
    let run_start = skip_spaces_and_tabs(bytes, line_start);
    let run_end = fence_run_end(bytes, run_start, fence_char);
    // Must be at least as long as the opening fence
    if run_end - run_start < fence_len {
        return None;
    }
    // Rest of line must be empty/whitespace
    line_end_if_blank(bytes, run_end)
}

/// Skip space/tab bytes starting at `i`; returns the first non-blank index.
fn skip_spaces_and_tabs(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    i
}

/// Return the index just past the run of `fence_char` bytes starting at `i`.
fn fence_run_end(bytes: &[u8], mut i: usize, fence_char: u8) -> usize {
    while i < bytes.len() && bytes[i] == fence_char {
        i += 1;
    }
    i
}

/// If the rest of the line from `i` holds only spaces/tabs, return the index
/// just past the terminating newline (or EOF); otherwise `None`.
fn line_end_if_blank(bytes: &[u8], mut i: usize) -> Option<usize> {
    while i < bytes.len() && bytes[i] != b'\n' {
        if !is_space_or_tab(bytes[i]) {
            return None;
        }
        i += 1;
    }
    if i < bytes.len() {
        i += 1; // skip the \n
    }
    Some(i)
}

/// Detect inline backtick spans outside the already-found fenced regions and
/// append them to `regions`.
fn append_inline_regions(text: &str, regions: &mut Vec<CodeRegion>) {
    let bytes = text.as_bytes();
    let mut j = 0;
    while j < bytes.len() {
        if bytes[j] != b'`' {
            j += 1;
            continue;
        }
        // Inside a fenced block? Skip
        if regions.iter().any(|r| j >= r.start && j < r.end) {
            j += 1;
            continue;
        }
        // Count opening backtick run
        let tick_start = j;
        while j < bytes.len() && bytes[j] == b'`' {
            j += 1;
        }
        let tick_len = j - tick_start;
        // Find matching closing run of exactly tick_len backticks
        if let Some(end) = find_closing_backticks(bytes, j, tick_len) {
            regions.push(CodeRegion {
                start: tick_start,
                end,
            });
            j = end;
        } else {
            j = tick_start + tick_len; // no match, move past
        }
    }
}

/// Scan forwards from `from` for a run of exactly `tick_len` backticks;
/// returns the index just past the run, or `None` when no run matches.
fn find_closing_backticks(bytes: &[u8], from: usize, tick_len: usize) -> Option<usize> {
    let mut k = from;
    while k < bytes.len() {
        if bytes[k] != b'`' {
            k += 1;
            continue;
        }
        let close_start = k;
        while k < bytes.len() && bytes[k] == b'`' {
            k += 1;
        }
        if k - close_start == tick_len {
            return Some(k);
        }
    }
    None
}

/// Check if a byte position falls inside any code region.
pub(super) fn is_inside_code(pos: usize, regions: &[CodeRegion]) -> bool {
    regions.iter().any(|r| pos >= r.start && pos < r.end)
}
