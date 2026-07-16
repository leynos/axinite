//! Format-specific text extraction routines.

mod office;

use office::{extract_docx, extract_pptx, extract_xlsx};

/// Extract text from document bytes based on MIME type and optional filename.
pub fn extract_text(data: &[u8], mime: &str, filename: Option<&str>) -> Result<String, String> {
    let base_mime = mime.split(';').next().unwrap_or(mime).trim();

    match base_mime {
        // PDF
        "application/pdf" => extract_pdf(data),

        // Office XML formats
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            extract_docx(data)
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            extract_pptx(data)
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => extract_xlsx(data),

        // Legacy Office (best-effort: treat as binary, try text extraction)
        "application/msword" | "application/vnd.ms-powerpoint" | "application/vnd.ms-excel" => {
            // Legacy binary formats — try to extract any text strings
            extract_binary_strings(data)
        }

        // Plain text family
        "text/plain"
        | "text/csv"
        | "text/tab-separated-values"
        | "text/markdown"
        | "text/html"
        | "text/xml"
        | "text/x-python"
        | "text/x-java"
        | "text/x-c"
        | "text/x-c++"
        | "text/x-rust"
        | "text/x-go"
        | "text/x-ruby"
        | "text/x-shellscript"
        | "text/javascript"
        | "text/css"
        | "text/x-toml"
        | "text/x-yaml"
        | "text/x-log" => extract_utf8(data),

        // JSON / XML / YAML application types
        "application/json" | "application/xml" | "application/x-yaml" | "application/yaml"
        | "application/toml" | "application/x-sh" => extract_utf8(data),

        // RTF
        "application/rtf" | "text/rtf" => extract_rtf(data),

        // Fallback: try to infer from filename extension
        _ => {
            if let Some(text) = try_extract_by_extension(data, filename) {
                Ok(text)
            } else {
                Err(format!("unsupported document type: {base_mime}"))
            }
        }
    }
}

fn extract_pdf(data: &[u8]) -> Result<String, String> {
    pdf_extract::extract_text_from_mem(data)
        .map(|t| t.trim().to_string())
        .map_err(|e| format!("PDF extraction failed: {e}"))
}

fn extract_utf8(data: &[u8]) -> Result<String, String> {
    // Try UTF-8 first, fall back to lossy decoding
    match std::str::from_utf8(data) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Ok(String::from_utf8_lossy(data).to_string()),
    }
}

fn extract_rtf(data: &[u8]) -> Result<String, String> {
    // Basic RTF text extraction: strip control words and groups
    let text = String::from_utf8_lossy(data);
    let mut result = String::new();
    let mut depth = 0i32;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            '\\' => {
                // Skip control word
                let mut word = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // Skip optional numeric parameter
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() || next == '-' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Consume trailing space
                if let Some(&' ') = chars.peek() {
                    chars.next();
                }
                // Convert common control words to text
                match word.as_str() {
                    "par" | "line" => result.push('\n'),
                    "tab" => result.push('\t'),
                    _ => {}
                }
            }
            _ => {
                if depth <= 1 {
                    result.push(ch);
                }
            }
        }
    }

    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        return Err("no text found in RTF".to_string());
    }
    Ok(trimmed)
}

fn extract_binary_strings(data: &[u8]) -> Result<String, String> {
    // Extract printable ASCII/UTF-8 runs from binary data (last resort)
    let mut strings = Vec::new();
    let mut current = String::new();

    for &byte in data {
        if (0x20..0x7F).contains(&byte) {
            current.push(byte as char);
        } else {
            if current.len() >= 4 {
                strings.push(std::mem::take(&mut current));
            }
            current.clear();
        }
    }
    if current.len() >= 4 {
        strings.push(current);
    }

    if strings.is_empty() {
        return Err("no readable text in binary document".to_string());
    }
    Ok(strings.join(" "))
}

/// Try to extract text based on filename extension when MIME type is generic.
fn try_extract_by_extension(data: &[u8], filename: Option<&str>) -> Option<String> {
    let ext = filename?.rsplit('.').next()?.to_lowercase();

    match ext.as_str() {
        "pdf" => extract_pdf(data).ok(),
        "docx" => extract_docx(data).ok(),
        "pptx" => extract_pptx(data).ok(),
        "xlsx" => extract_xlsx(data).ok(),
        "doc" | "ppt" | "xls" => extract_binary_strings(data).ok(),
        "rtf" => extract_rtf(data).ok(),
        "txt" | "csv" | "tsv" | "json" | "xml" | "yaml" | "yml" | "toml" | "md" | "markdown"
        | "py" | "js" | "ts" | "rs" | "go" | "java" | "c" | "cpp" | "h" | "hpp" | "rb" | "sh"
        | "bash" | "zsh" | "fish" | "css" | "html" | "htm" | "sql" | "log" | "ini" | "cfg"
        | "conf" | "env" | "gitignore" | "dockerfile" => extract_utf8(data).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
