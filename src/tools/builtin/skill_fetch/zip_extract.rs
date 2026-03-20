//! ZIP parsing and extraction helpers for bundled skill downloads.

use std::io::{Read, Take};

use flate2::read::DeflateDecoder;

use crate::tools::tool::ToolError;

const MAX_DECOMPRESSED: usize = 1024 * 1024;

/// Parsed fields from a ZIP local-file header (signature `PK\x03\x04`).
struct ZipLocalHeader {
    flags: u16,
    compression: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    name_start: usize,
    name_end: usize,
    extra_len: usize,
}

/// Parse a ZIP local-file header at `offset` into a [`ZipLocalHeader`].
///
/// Callers must enforce the precondition `offset + 30 <= data.len()` before
/// calling this function. If the four-byte signature does not match
/// `0x50 0x4B 0x03 0x04`, this returns `None`. Violating the length
/// precondition causes out-of-bounds panics rather than a safe error, so
/// callers must validate buffer bounds first. Callers must also validate that
/// `data.len() >= name_end + extra_len` before treating the parsed filename and
/// extra-field ranges as safe to read.
fn parse_zip_local_header(data: &[u8], offset: usize) -> Option<ZipLocalHeader> {
    if data[offset..offset + 4] != [0x50, 0x4B, 0x03, 0x04] {
        return None;
    }
    let flags = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
    let compression = u16::from_le_bytes([data[offset + 8], data[offset + 9]]);
    let compressed_size = u32::from_le_bytes([
        data[offset + 18],
        data[offset + 19],
        data[offset + 20],
        data[offset + 21],
    ]) as usize;
    let uncompressed_size = u32::from_le_bytes([
        data[offset + 22],
        data[offset + 23],
        data[offset + 24],
        data[offset + 25],
    ]) as usize;
    let name_len = u16::from_le_bytes([data[offset + 26], data[offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[offset + 28], data[offset + 29]]) as usize;
    let name_start = offset + 30;
    let name_end = name_start + name_len;
    Some(ZipLocalHeader {
        flags,
        compression,
        compressed_size,
        uncompressed_size,
        name_start,
        name_end,
        extra_len,
    })
}

/// Decompress `raw` bytes using ZIP `compression` method 0 (stored) or
/// 8 (deflate). Returns an error for any other method.
fn decompress_zip_entry(
    raw: &[u8],
    compression: u16,
    uncompressed_size: usize,
) -> Result<Vec<u8>, ToolError> {
    if raw.len() > MAX_DECOMPRESSED {
        return Err(ToolError::ExecutionFailed(
            "ZIP entry too large to decompress safely".to_string(),
        ));
    }

    match compression {
        0 => {
            if raw.len() != uncompressed_size {
                return Err(ToolError::ExecutionFailed(
                    "ZIP archive truncated".to_string(),
                ));
            }
            Ok(raw.to_vec())
        }
        8 => {
            let mut decoder: Take<DeflateDecoder<&[u8]>> =
                DeflateDecoder::new(raw).take((MAX_DECOMPRESSED as u64) + 1);
            let mut buf = Vec::with_capacity(uncompressed_size.min(MAX_DECOMPRESSED));
            decoder.read_to_end(&mut buf).map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to decompress SKILL.md: {}", e))
            })?;
            if buf.len() > MAX_DECOMPRESSED {
                return Err(ToolError::ExecutionFailed(
                    "ZIP entry too large to decompress safely".to_string(),
                ));
            }
            if buf.len() == MAX_DECOMPRESSED && uncompressed_size > MAX_DECOMPRESSED {
                return Err(ToolError::ExecutionFailed(
                    "ZIP entry too large to decompress safely".to_string(),
                ));
            }
            if buf.len() != uncompressed_size {
                return Err(ToolError::ExecutionFailed(
                    "ZIP archive truncated".to_string(),
                ));
            }
            Ok(buf)
        }
        other => Err(ToolError::ExecutionFailed(format!(
            "Unsupported ZIP compression method: {}",
            other
        ))),
    }
}

/// Parameters for extracting a single `SKILL.md` archive entry.
struct SkillEntryParams<'a> {
    data: &'a [u8],
    data_start: usize,
    data_end: usize,
    compression: u16,
    uncompressed_size: usize,
}

/// Validate bounds and size, decompress, and decode `SKILL.md` bytes to UTF-8.
fn extract_skill_entry(args: SkillEntryParams<'_>) -> Result<String, ToolError> {
    if args.data_end > args.data.len() {
        return Err(ToolError::ExecutionFailed(
            "ZIP archive truncated".to_string(),
        ));
    }
    if args.uncompressed_size > MAX_DECOMPRESSED {
        return Err(ToolError::ExecutionFailed(
            "ZIP entry too large to decompress safely".to_string(),
        ));
    }
    let decompressed = decompress_zip_entry(
        &args.data[args.data_start..args.data_end],
        args.compression,
        args.uncompressed_size,
    )?;
    String::from_utf8(decompressed).map_err(|e| {
        ToolError::ExecutionFailed(format!("SKILL.md in archive is not valid UTF-8: {}", e))
    })
}

/// Extract the root `SKILL.md` payload from a complete ZIP archive.
///
/// This function expects a complete ZIP archive as untrusted `&[u8]` input and
/// performs manual local-header parsing rather than relying on a high-level ZIP
/// library. It scans entries in local-header order, requires a root filename of
/// exactly `SKILL.md`, and validates header bounds, `extra_len`,
/// `compressed_size`, and `uncompressed_size` before passing matching entries
/// to [`extract_skill_entry`] for decompression and UTF-8 validation.
///
/// The parser enforces the configured size constraints for compressed input,
/// decompressed entry data, filename-derived offsets, and checked
/// offset-and-length arithmetic. Callers must treat the provided bytes as
/// untrusted input, and this function will reject malformed, truncated, or
/// oversized archives before attempting to return the skill payload.
///
/// Returns `Err` when offset arithmetic overflows (`ZIP header offset
/// overflow`, `ZIP header size overflow`), when entry data points out of
/// bounds, when `SKILL.md` is missing, or when [`extract_skill_entry`]
/// reports truncation, unsupported compression, invalid UTF-8 payload bytes, or
/// other [`ToolError::ExecutionFailed`] validation failures. Entry names with
/// invalid UTF-8 are treated as non-matching, which eventually yields the
/// missing-`SKILL.md` error.
pub(super) fn extract_skill_from_zip(data: &[u8]) -> Result<String, ToolError> {
    let mut offset = 0usize;

    while offset + 30 <= data.len() {
        let header = match parse_zip_local_header(data, offset) {
            Some(h) => h,
            None => break,
        };
        if header.flags & 0x0008 != 0 {
            return Err(ToolError::ExecutionFailed(
                "ZIP entries using data descriptors are not supported".to_string(),
            ));
        }

        if header.name_end > data.len() {
            break;
        }
        let file_name =
            std::str::from_utf8(&data[header.name_start..header.name_end]).unwrap_or("");

        let data_start = header
            .name_end
            .checked_add(header.extra_len)
            .ok_or_else(|| ToolError::ExecutionFailed("ZIP header offset overflow".to_string()))?;
        let data_end = data_start
            .checked_add(header.compressed_size)
            .ok_or_else(|| ToolError::ExecutionFailed("ZIP header size overflow".to_string()))?;

        if file_name == "SKILL.md" {
            return extract_skill_entry(SkillEntryParams {
                data,
                data_start,
                data_end,
                compression: header.compression,
                uncompressed_size: header.uncompressed_size,
            });
        }

        offset = data_end;
    }

    Err(ToolError::ExecutionFailed(
        "ZIP archive does not contain SKILL.md".to_string(),
    ))
}
