//! Null implementation of NativeToolFailureStore for NullDatabase.

use crate::agent::BrokenTool;
use crate::error::DatabaseError;

use super::NullDatabase;

impl crate::db::NativeToolFailureStore for NullDatabase {
    async fn record_tool_failure(
        &self,
        _tool_name: &str,
        _error: &str,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_broken_tools(&self, _threshold: i32) -> Result<Vec<BrokenTool>, DatabaseError> {
        Ok(vec![])
    }

    async fn mark_tool_repaired(&self, _tool_name: &str) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn increment_repair_attempts(&self, _tool_name: &str) -> Result<(), DatabaseError> {
        Ok(())
    }
}
