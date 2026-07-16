//! Extraction for Office Open XML archives (DOCX, PPTX, XLSX).

use std::io::{Read, Seek};

pub(super) fn extract_docx(data: &[u8]) -> Result<String, String> {
    extract_office_xml(data, "word/document.xml")
}

/// Collect archive entry names matching `prefix`/`suffix`, sorted so slide and
/// sheet ordering is deterministic.
fn collect_entry_names<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    prefix: &str,
    suffix: &str,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_string();
            if name.starts_with(prefix) && name.ends_with(suffix) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Read each named entry to a string, apply `f`, and collect the non-empty
/// results. Unreadable entries are skipped.
fn read_entry_texts<R, F>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    mut f: F,
) -> Vec<String>
where
    R: Read + Seek,
    F: FnMut(&str) -> String,
{
    let mut out = Vec::new();
    for name in names {
        let Ok(mut file) = archive.by_name(name) else {
            continue;
        };
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_err() {
            continue;
        }
        let text = f(&xml);
        if !text.is_empty() {
            out.push(text);
        }
    }
    out
}

pub(super) fn extract_pptx(data: &[u8]) -> Result<String, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("invalid PPTX archive: {e}"))?;

    let slide_names = collect_entry_names(&mut archive, "ppt/slides/slide", ".xml");
    let all_text = read_entry_texts(&mut archive, &slide_names, strip_xml_tags);

    if all_text.is_empty() {
        return Err("no text found in PPTX slides".to_string());
    }
    Ok(all_text.join("\n\n---\n\n"))
}

/// Read the workbook shared-strings table, returning an empty table when the
/// entry is absent.
fn read_xlsx_shared_strings<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Vec<String>, String> {
    let Ok(mut file) = archive.by_name("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .map_err(|e| format!("failed to read shared strings: {e}"))?;
    Ok(parse_xlsx_shared_strings(&xml))
}

/// Combine extracted sheet text with the shared-strings fallback, or report an
/// error when the workbook yields no text at all.
fn finalize_xlsx_text(
    all_text: Vec<String>,
    shared_strings: Vec<String>,
) -> Result<String, String> {
    if all_text.is_empty() && !shared_strings.is_empty() {
        // Fall back to the raw shared-strings table.
        return Ok(shared_strings.join("\n"));
    }
    if all_text.is_empty() {
        return Err("no text found in XLSX".to_string());
    }
    Ok(all_text.join("\n\n"))
}

pub(super) fn extract_xlsx(data: &[u8]) -> Result<String, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("invalid XLSX archive: {e}"))?;

    let shared_strings = read_xlsx_shared_strings(&mut archive)?;

    let sheet_names = collect_entry_names(&mut archive, "xl/worksheets/sheet", ".xml");
    let all_text = read_entry_texts(&mut archive, &sheet_names, |xml| {
        parse_xlsx_sheet(xml, &shared_strings)
    });

    finalize_xlsx_text(all_text, shared_strings)
}

fn extract_office_xml(data: &[u8], content_path: &str) -> Result<String, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("invalid Office XML archive: {e}"))?;

    let mut file = archive
        .by_name(content_path)
        .map_err(|e| format!("content file not found in archive: {e}"))?;

    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .map_err(|e| format!("failed to read content: {e}"))?;

    let text = strip_xml_tags(&xml);
    if text.is_empty() {
        return Err("no text content found".to_string());
    }
    Ok(text)
}

/// Accumulates the text content stripped from between XML tags, collapsing
/// runs of whitespace (and tag boundaries) into single separating spaces.
struct StrippedText {
    result: String,
    last_was_space: bool,
}

impl StrippedText {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            result: String::with_capacity(capacity),
            last_was_space: true,
        }
    }

    /// Emit a single separating space at a tag boundary, unless the previous
    /// character was already a space or nothing has been emitted yet.
    fn separate(&mut self) {
        if !self.last_was_space && !self.result.is_empty() {
            self.result.push(' ');
            self.last_was_space = true;
        }
    }
}

impl XmlTagScanner for StrippedText {
    /// A tag boundary separates adjacent text runs with a single space.
    fn handle_tag(&mut self, _tag: &str) {
        self.separate();
    }

    /// Append one character of text content, collapsing whitespace runs.
    fn push_char(&mut self, ch: char) {
        if !ch.is_whitespace() {
            self.result.push(ch);
            self.last_was_space = false;
        } else {
            self.separate();
        }
    }
}

/// Strip XML tags and return just the text content.
pub(super) fn strip_xml_tags(xml: &str) -> String {
    let mut text = StrippedText::with_capacity(xml.len() / 2);
    scan_xml_tags(xml, &mut text);
    // Decode common XML entities.
    text.result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

/// Scanner fed by [`scan_xml_tags`]: reacts to complete tags and to the text
/// characters between them.
trait XmlTagScanner {
    /// React to one complete XML tag, already trimmed and free of `<`/`>`.
    fn handle_tag(&mut self, tag: &str);
    /// Consume one character of inter-tag text content.
    fn push_char(&mut self, ch: char);
}

/// Drive a `<tag>text` scan over `xml`, dispatching each complete tag to
/// [`XmlTagScanner::handle_tag`] and every text character to
/// [`XmlTagScanner::push_char`].
fn scan_xml_tags(xml: &str, scanner: &mut impl XmlTagScanner) {
    let mut in_tag = false;
    let mut tag_buf = String::new();

    for ch in xml.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' => {
                in_tag = false;
                scanner.handle_tag(tag_buf.trim());
            }
            _ if in_tag => tag_buf.push(ch),
            _ => scanner.push_char(ch),
        }
    }
}

/// Streaming state for the shared-strings parser: tracks whether a `<t>` text
/// run is open and accumulates its content.
struct SharedStringsParser {
    strings: Vec<String>,
    in_t: bool,
    current: String,
}

impl XmlTagScanner for SharedStringsParser {
    fn handle_tag(&mut self, tag: &str) {
        if tag == "/t" {
            self.in_t = false;
            self.strings.push(std::mem::take(&mut self.current));
        } else if tag == "/si" {
            self.in_t = false;
        } else if is_element_open(tag, "t") {
            self.in_t = true;
            self.current.clear();
        }
    }

    fn push_char(&mut self, ch: char) {
        if self.in_t {
            self.current.push(ch);
        }
    }
}

/// Parse XLSX shared strings XML into a Vec of strings.
pub(super) fn parse_xlsx_shared_strings(xml: &str) -> Vec<String> {
    let mut parser = SharedStringsParser {
        strings: Vec::new(),
        in_t: false,
        current: String::new(),
    };
    scan_xml_tags(xml, &mut parser);
    parser.strings
}

/// Return `true` when `tag` opens the element `name` — either the bare name
/// (`<name>`) or the name followed by attributes (`<name ...>`).
fn is_element_open(tag: &str, name: &str) -> bool {
    tag.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

/// The structurally significant XLSX sheet tags the cell parser reacts to.
enum SheetTag {
    RowOpen,
    RowClose,
    CellOpen,
    ValueOpen,
    ValueClose,
    CellClose,
    Other,
}

/// Classify a complete XLSX sheet tag into the kind the parser acts on.
fn classify_sheet_tag(tag: &str) -> SheetTag {
    match tag {
        "/row" => SheetTag::RowClose,
        "/v" => SheetTag::ValueClose,
        "/c" => SheetTag::CellClose,
        _ if is_element_open(tag, "row") => SheetTag::RowOpen,
        _ if is_element_open(tag, "c") => SheetTag::CellOpen,
        _ if is_element_open(tag, "v") => SheetTag::ValueOpen,
        _ => SheetTag::Other,
    }
}

/// Extract the `t="..."` type attribute from a cell open tag
/// (`t="s"` means shared string).
fn cell_type_attribute(tag: &str) -> String {
    let Some((_, rest)) = tag.split_once("t=\"") else {
        return String::new();
    };
    rest.split_once('"')
        .map_or_else(String::new, |(value, _)| value.to_string())
}

/// Streaming state for the XLSX sheet cell parser: tracks the current row,
/// the value being read, and the current cell's type.
struct SheetParser<'a> {
    shared_strings: &'a [String],
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    in_v: bool,
    in_row: bool,
    current_val: String,
    cell_type: String,
}

impl<'a> SheetParser<'a> {
    fn new(shared_strings: &'a [String]) -> Self {
        Self {
            shared_strings,
            rows: Vec::new(),
            current_row: Vec::new(),
            in_v: false,
            in_row: false,
            current_val: String::new(),
            cell_type: String::new(),
        }
    }

    /// Close the current row, keeping it only when it has cells.
    fn end_row(&mut self) {
        self.in_row = false;
        if !self.current_row.is_empty() {
            self.rows.push(std::mem::take(&mut self.current_row));
        }
    }

    /// Close the current value, resolving shared-string references.
    fn end_value(&mut self) {
        self.in_v = false;
        let val = if self.cell_type == "s" {
            // Shared string reference
            self.current_val
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|idx| self.shared_strings.get(idx))
                .cloned()
                .unwrap_or_default()
        } else {
            self.current_val.clone()
        };
        self.current_row.push(val);
    }

    /// Render the collected rows as tab-separated lines.
    fn render(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl XmlTagScanner for SheetParser<'_> {
    fn handle_tag(&mut self, tag: &str) {
        match classify_sheet_tag(tag) {
            SheetTag::RowOpen => {
                self.in_row = true;
                self.current_row.clear();
            }
            SheetTag::RowClose => self.end_row(),
            SheetTag::CellOpen if self.in_row => {
                self.cell_type = cell_type_attribute(tag);
            }
            SheetTag::ValueOpen => {
                self.in_v = true;
                self.current_val.clear();
            }
            SheetTag::ValueClose => self.end_value(),
            SheetTag::CellClose => self.cell_type.clear(),
            _ => {}
        }
    }

    fn push_char(&mut self, ch: char) {
        if self.in_v {
            self.current_val.push(ch);
        }
    }
}

/// Parse XLSX sheet XML into tab-separated rows.
fn parse_xlsx_sheet(xml: &str, shared_strings: &[String]) -> String {
    // Resolve <v> values in <c> cells, dereferencing shared strings.
    let mut parser = SheetParser::new(shared_strings);
    scan_xml_tags(xml, &mut parser);
    parser.render()
}
