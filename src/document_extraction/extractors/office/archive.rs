//! Shared ZIP entry selection and text reading for Office extraction.

use std::io::{Read, Seek};

/// Collect archive entry names matching `prefix`/`suffix`, sorted so slide and
/// sheet ordering is deterministic. Unreadable entries and names are skipped.
pub(super) fn collect_entry_names<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    prefix: &str,
    suffix: &str,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let Ok(file) = archive.by_index(i) else {
            continue;
        };
        let Ok(name) = file.name() else {
            continue;
        };
        if name.starts_with(prefix) && name.ends_with(suffix) {
            names.push(name.into_owned());
        }
    }
    names.sort();
    names
}

/// Read each named entry to a string, apply `f`, and collect the non-empty
/// results. Unreadable entries are skipped.
pub(super) fn read_entry_texts<R, F>(
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
