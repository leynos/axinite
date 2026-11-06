//! Memory browsing and search DTOs for the web gateway API.

use serde::{Deserialize, Serialize};

// --- Memory ---

/// Response for the memory tree endpoint, listing every workspace entry.
#[derive(Debug, Serialize)]
pub struct MemoryTreeResponse {
    /// Directory and file entries, sorted by path.
    pub entries: Vec<TreeEntry>,
}

/// One node in the memory tree.
#[derive(Debug, Serialize)]
pub struct TreeEntry {
    /// Workspace-relative path, with `/` as the separator.
    pub path: String,
    /// `true` for a directory, `false` for a document.
    pub is_dir: bool,
}

/// Response for listing the immediate children of one workspace directory.
#[derive(Debug, Serialize)]
pub struct MemoryListResponse {
    /// Directory that was listed, echoed from the request. Empty for the workspace root.
    pub path: String,
    /// Immediate children of `path`.
    pub entries: Vec<ListEntry>,
}

/// One immediate child of a listed workspace directory.
#[derive(Debug, Serialize)]
pub struct ListEntry {
    /// Last path segment of the entry.
    pub name: String,
    /// Full workspace-relative path of the entry.
    pub path: String,
    /// `true` when the entry is a directory.
    pub is_dir: bool,
    /// Last update time as an RFC 3339 string. `None` when the workspace records none.
    pub updated_at: Option<String>,
}

/// Response for reading one memory document.
#[derive(Debug, Serialize)]
pub struct MemoryReadResponse {
    /// Document path, echoed from the request.
    pub path: String,
    /// Full text content of the document.
    pub content: String,
    /// RFC 3339 timestamp of the last write. The read handler always sets it.
    pub updated_at: Option<String>,
}

/// Request body to create or overwrite a memory document.
#[derive(Debug, Deserialize)]
pub struct MemoryWriteRequest {
    /// Workspace-relative path of the document to write.
    pub path: String,
    /// Full replacement text for the document.
    pub content: String,
}

/// Acknowledgement returned after a successful write.
#[derive(Debug, Serialize)]
pub struct MemoryWriteResponse {
    /// Path that was written, echoed from the request.
    pub path: String,
    /// Outcome label. Currently always `written`.
    pub status: &'static str,
}

/// Request body for searching memory documents.
#[derive(Debug, Deserialize)]
pub struct MemorySearchRequest {
    /// Query text passed to the workspace search.
    pub query: String,
    /// Maximum number of hits to return. Defaults to 10 when omitted.
    pub limit: Option<usize>,
}

/// Response for a memory search.
#[derive(Debug, Serialize)]
pub struct MemorySearchResponse {
    /// Matching hits, as returned by the workspace search.
    pub results: Vec<SearchHit>,
}

/// One document matched by a memory search.
#[derive(Debug, Serialize)]
pub struct SearchHit {
    /// Workspace-relative path of the matched document.
    pub path: String,
    /// Matched content returned by the search.
    pub content: String,
    /// Combined reciprocal-rank-fusion relevance score. Hits are sorted by it, highest first.
    pub score: f64,
}
