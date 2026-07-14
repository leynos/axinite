//! Extraction for Office Open XML archives (DOCX, PPTX, XLSX).

use std::io::Read;

pub(super) fn extract_docx(data: &[u8]) -> Result<String, String> {
    extract_office_xml(data, "word/document.xml")
}

pub(super) fn extract_pptx(data: &[u8]) -> Result<String, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("invalid PPTX archive: {e}"))?;

    // Collect slide filenames (ppt/slides/slide1.xml, slide2.xml, ...)
    let mut slide_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_string();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                slide_names.push(name);
            }
        }
    }
    slide_names.sort();

    let mut all_text = Vec::new();
    for name in &slide_names {
        if let Ok(mut file) = archive.by_name(name) {
            let mut xml = String::new();
            if file.read_to_string(&mut xml).is_ok() {
                let text = strip_xml_tags(&xml);
                if !text.is_empty() {
                    all_text.push(text);
                }
            }
        }
    }

    if all_text.is_empty() {
        return Err("no text found in PPTX slides".to_string());
    }
    Ok(all_text.join("\n\n---\n\n"))
}

pub(super) fn extract_xlsx(data: &[u8]) -> Result<String, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("invalid XLSX archive: {e}"))?;

    // Read shared strings (xl/sharedStrings.xml)
    let shared_strings = if let Ok(mut file) = archive.by_name("xl/sharedStrings.xml") {
        let mut xml = String::new();
        file.read_to_string(&mut xml)
            .map_err(|e| format!("failed to read shared strings: {e}"))?;
        parse_xlsx_shared_strings(&xml)
    } else {
        Vec::new()
    };

    // Read sheet data
    let mut sheet_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_string();
            if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
                sheet_names.push(name);
            }
        }
    }
    sheet_names.sort();

    let mut all_text = Vec::new();
    for name in &sheet_names {
        if let Ok(mut file) = archive.by_name(name) {
            let mut xml = String::new();
            if file.read_to_string(&mut xml).is_ok() {
                let text = parse_xlsx_sheet(&xml, &shared_strings);
                if !text.is_empty() {
                    all_text.push(text);
                }
            }
        }
    }

    if all_text.is_empty() && !shared_strings.is_empty() {
        // Fallback: just return shared strings
        return Ok(shared_strings.join("\n"));
    }

    if all_text.is_empty() {
        return Err("no text found in XLSX".to_string());
    }
    Ok(all_text.join("\n\n"))
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

/// Strip XML tags and return just the text content.
pub(super) fn strip_xml_tags(xml: &str) -> String {
    let mut result = String::with_capacity(xml.len() / 2);
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in xml.chars() {
        match ch {
            '<' => {
                in_tag = true;
            }
            '>' => {
                in_tag = false;
                // Add space between tag-delimited text runs
                if !last_was_space && !result.is_empty() {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if !in_tag => {
                if ch.is_whitespace() {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(ch);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Decode common XML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

/// Parse XLSX shared strings XML into a Vec of strings.
pub(super) fn parse_xlsx_shared_strings(xml: &str) -> Vec<String> {
    // Shared strings are in <si><t>text</t></si> elements
    let mut strings = Vec::new();
    let mut in_t = false;
    let mut current = String::new();
    let mut in_tag = false;
    let mut tag_name = String::new();

    for ch in xml.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_name.clear();
            }
            '>' => {
                in_tag = false;
                let tag = tag_name.trim().to_string();
                if tag == "t" || tag.starts_with("t ") {
                    in_t = true;
                    current.clear();
                } else if tag == "/t" {
                    in_t = false;
                    strings.push(std::mem::take(&mut current));
                } else if tag == "/si" {
                    in_t = false;
                }
            }
            _ if in_tag => {
                tag_name.push(ch);
            }
            _ if in_t => {
                current.push(ch);
            }
            _ => {}
        }
    }

    strings
}

/// Return `true` when an XLSX sheet tag opens a cell (`<c>` or `<c ...>`).
fn is_cell_open_tag(tag: &str) -> bool {
    tag.starts_with("c ") || tag == "c"
}

/// Extract the `t="..."` type attribute from a cell open tag
/// (`t="s"` means shared string).
fn cell_type_attribute(tag: &str) -> String {
    let Some(t_pos) = tag.find("t=\"") else {
        return String::new();
    };
    let rest = &tag[t_pos + 3..];
    match rest.find('"') {
        Some(end) => rest[..end].to_string(),
        None => String::new(),
    }
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

    /// React to one complete XML tag.
    fn handle_tag(&mut self, tag: &str) {
        if tag == "row" || tag.starts_with("row ") {
            self.in_row = true;
            self.current_row.clear();
        } else if tag == "/row" {
            self.end_row();
        } else if self.in_row && is_cell_open_tag(tag) {
            self.cell_type = cell_type_attribute(tag);
        } else if tag == "v" || tag.starts_with("v ") {
            self.in_v = true;
            self.current_val.clear();
        } else if tag == "/v" {
            self.end_value();
        } else if tag == "/c" {
            self.cell_type.clear();
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

/// Parse XLSX sheet XML into tab-separated rows.
fn parse_xlsx_sheet(xml: &str, shared_strings: &[String]) -> String {
    // Simple extraction: find <v> values in <c> cells, resolve shared string refs
    let mut parser = SheetParser::new(shared_strings);
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
                parser.handle_tag(tag_buf.trim());
            }
            _ if in_tag => {
                tag_buf.push(ch);
            }
            _ if parser.in_v => {
                parser.current_val.push(ch);
            }
            _ => {}
        }
    }

    parser.render()
}
