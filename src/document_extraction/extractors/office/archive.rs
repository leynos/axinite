//! Shared ZIP entry selection and text reading for Office extraction.

use std::io::{Read, Seek};

/// An archive entry paired with its decoded name for sorting and its stable
/// archive index for reading.
pub(super) struct ArchiveEntry {
    name: String,
    index: usize,
}

/// Collect archive entries matching `prefix`/`suffix`, sorted by decoded name
/// so slide and sheet ordering is deterministic. Unreadable entries and names
/// are skipped.
pub(super) fn collect_entries<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    prefix: &str,
    suffix: &str,
) -> Vec<ArchiveEntry> {
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let Ok(file) = archive.by_index(i) else {
            continue;
        };
        let Ok(name) = file.name() else {
            // Retain usable Office text when another entry cannot be decoded.
            continue;
        };
        if name.starts_with(prefix) && name.ends_with(suffix) {
            entries.push(ArchiveEntry {
                name: name.into_owned(),
                index: i,
            });
        }
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

/// Read each entry to a string, apply `f`, and collect the non-empty results.
/// Unreadable entries are skipped.
pub(super) fn read_entry_texts<R, F>(
    archive: &mut zip::ZipArchive<R>,
    entries: &[ArchiveEntry],
    mut f: F,
) -> Vec<String>
where
    R: Read + Seek,
    F: FnMut(&str) -> String,
{
    let mut out = Vec::new();
    for entry in entries {
        let Ok(mut file) = archive.by_index(entry.index) else {
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
