//! Shared row-to-domain conversion helpers for the libSQL backend.

use crate::workspace::MemoryDocument;

use super::helpers::{get_json, get_opt_text, get_text, get_ts, parse_opt_uuid, parse_uuid_or_nil};

pub(crate) fn row_to_memory_document(row: &libsql::Row) -> MemoryDocument {
    let id_raw = get_text(row, 0);
    let agent_id_raw = get_opt_text(row, 2);

    MemoryDocument {
        id: parse_uuid_or_nil(&id_raw, 0, "memory_documents.id"),
        user_id: get_text(row, 1),
        agent_id: parse_opt_uuid(agent_id_raw, 2, "memory_documents.agent_id"),
        path: get_text(row, 3),
        content: get_text(row, 4),
        created_at: get_ts(row, 5),
        updated_at: get_ts(row, 6),
        metadata: get_json(row, 7),
    }
}
