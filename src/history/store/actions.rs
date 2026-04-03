//! Job action persistence helpers.

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
use crate::context::ActionRecord;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
    /// Save a job action.
    pub async fn save_action(
        &self,
        job_id: Uuid,
        action: &ActionRecord,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        let duration_ms = i32::try_from(action.duration.as_millis()).map_err(|_| {
            DatabaseError::Serialization(format!(
                "job action duration exceeds i32 milliseconds: {}",
                action.duration.as_millis()
            ))
        })?;
        let sequence_num = i32::try_from(action.sequence).map_err(|_| {
            DatabaseError::Serialization(format!(
                "job action sequence exceeds i32: {}",
                action.sequence
            ))
        })?;
        let warnings_json = serde_json::to_value(&action.sanitization_warnings)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        conn.execute(
            r#"
            INSERT INTO job_actions (
                id, job_id, sequence_num, tool_name, input, output_raw, output_sanitized,
                sanitization_warnings, cost, duration_ms, success, error_message, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
            &[
                &action.id,
                &job_id,
                &sequence_num,
                &action.tool_name,
                &action.input,
                &action.output_raw,
                &action.output_sanitized,
                &warnings_json,
                &action.cost,
                &duration_ms,
                &action.success,
                &action.error,
                &action.executed_at,
            ],
        )
        .await?;

        Ok(())
    }

    /// Get actions for a job.
    pub async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        let conn = self.conn().await?;

        let rows = conn
            .query(
                r#"
                SELECT id, sequence_num, tool_name, input, output_raw, output_sanitized,
                       sanitization_warnings, cost, duration_ms, success, error_message, created_at
                FROM job_actions WHERE job_id = $1 ORDER BY sequence_num
                "#,
                &[&job_id],
            )
            .await?;

        let mut actions = Vec::new();
        for row in rows {
            let duration_ms: i32 = row.get("duration_ms");
            if duration_ms < 0 {
                return Err(DatabaseError::Serialization(format!(
                    "job action duration_ms must be non-negative: {duration_ms}"
                )));
            }

            let sequence_num: i32 = row.get("sequence_num");
            if sequence_num < 0 {
                return Err(DatabaseError::Serialization(format!(
                    "job action sequence_num must be non-negative: {sequence_num}"
                )));
            }

            let warnings_json: Option<serde_json::Value> = row.get("sanitization_warnings");
            let warnings = match warnings_json {
                None | Some(serde_json::Value::Null) => Vec::new(),
                Some(value) => serde_json::from_value(value).map_err(|e| {
                    DatabaseError::Serialization(format!(
                        "invalid sanitization_warnings payload for job action: {e}"
                    ))
                })?,
            };

            actions.push(ActionRecord {
                id: row.get("id"),
                sequence: sequence_num as u32,
                tool_name: row.get("tool_name"),
                input: row.get("input"),
                output_raw: row.get("output_raw"),
                output_sanitized: row.get("output_sanitized"),
                sanitization_warnings: warnings,
                cost: row.get("cost"),
                duration: std::time::Duration::from_millis(duration_ms as u64),
                success: row.get("success"),
                error: row.get("error_message"),
                executed_at: row.get("created_at"),
            });
        }

        Ok(actions)
    }
}

#[cfg(all(test, feature = "postgres"))]
#[path = "actions/tests.rs"]
mod tests;
