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

            let warnings_json: serde_json::Value = row.get("sanitization_warnings");
            let warnings = match warnings_json {
                serde_json::Value::Null => Vec::new(),
                value => serde_json::from_value(value).map_err(|e| {
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
mod tests {
    use std::time::Duration;

    use chrono::Utc;
    use rstest::{fixture, rstest};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{ActionRecord, DatabaseError, Store};
    use crate::context::JobContext;
    use crate::testing::try_test_pg_db;

    #[fixture]
    async fn seeded_store() -> Option<(Store, Uuid)> {
        let backend = try_test_pg_db().await?;
        let store = Store::from_pool(backend.pool());
        let ctx = JobContext::with_user(
            format!("actions-test-{}", Uuid::new_v4()),
            "job action fixture",
            "job action fixture",
        );
        let job_id = ctx.job_id;
        store.save_job(&ctx).await.expect("save_job should succeed");
        Some((store, job_id))
    }

    async fn cleanup_job(store: &Store, job_id: Uuid) {
        let conn = store.conn().await.expect("conn should succeed");
        conn.execute("DELETE FROM job_actions WHERE job_id = $1", &[&job_id])
            .await
            .expect("delete job_actions should succeed");
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&job_id])
            .await
            .expect("delete agent_jobs should succeed");
    }

    fn sample_action() -> ActionRecord {
        ActionRecord {
            id: Uuid::new_v4(),
            sequence: 7,
            tool_name: "shell".to_string(),
            input: serde_json::json!({ "cmd": "echo hi" }),
            output_raw: Some("hi".to_string()),
            output_sanitized: Some(serde_json::json!({ "stdout": "hi" })),
            sanitization_warnings: vec!["trimmed".to_string()],
            cost: Some(Decimal::new(125, 2)),
            duration: Duration::from_millis(250),
            success: true,
            error: None,
            executed_at: Utc::now(),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn save_action_round_trips_via_get_job_actions(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let action = sample_action();

        store
            .save_action(job_id, &action)
            .await
            .expect("save_action should succeed");

        let actions = store
            .get_job_actions(job_id)
            .await
            .expect("get_job_actions should succeed");

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, action.id);
        assert_eq!(actions[0].sequence, action.sequence);
        assert_eq!(actions[0].tool_name, action.tool_name);
        assert_eq!(actions[0].input, action.input);
        assert_eq!(actions[0].output_raw, action.output_raw);
        assert_eq!(actions[0].output_sanitized, action.output_sanitized);
        assert_eq!(
            actions[0].sanitization_warnings,
            action.sanitization_warnings
        );
        assert_eq!(actions[0].cost, action.cost);
        assert_eq!(actions[0].duration, action.duration);
        assert_eq!(actions[0].success, action.success);
        assert_eq!(actions[0].error, action.error);

        cleanup_job(&store, job_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn save_action_rejects_duration_that_exceeds_i32_millis(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let mut action = sample_action();
        action.duration = Duration::from_millis(i32::MAX as u64 + 1);

        let result = store.save_action(job_id, &action).await;

        assert!(matches!(
            result,
            Err(DatabaseError::Serialization(message))
                if message.contains("duration exceeds i32")
        ));

        cleanup_job(&store, job_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn save_action_rejects_sequence_that_exceeds_i32(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let mut action = sample_action();
        action.sequence = i32::MAX as u32 + 1;

        let result = store.save_action(job_id, &action).await;

        assert!(matches!(
            result,
            Err(DatabaseError::Serialization(message))
                if message.contains("sequence exceeds i32")
        ));

        cleanup_job(&store, job_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn get_job_actions_rejects_negative_duration(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let conn = store.conn().await.expect("conn should succeed");
        let action_id = Uuid::new_v4();
        let executed_at = Utc::now();
        let warnings = serde_json::json!(["warning"]);
        let input = serde_json::json!({ "cmd": "echo hi" });

        conn.execute(
            r#"
            INSERT INTO job_actions (
                id, job_id, sequence_num, tool_name, input, output_raw, output_sanitized,
                sanitization_warnings, cost, duration_ms, success, error_message, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
            &[
                &action_id,
                &job_id,
                &0i32,
                &"shell",
                &input,
                &Some("hi".to_string()),
                &Some(serde_json::json!({ "stdout": "hi" })),
                &warnings,
                &Some(Decimal::new(50, 2)),
                &-1i32,
                &true,
                &Option::<String>::None,
                &executed_at,
            ],
        )
        .await
        .expect("insert job action should succeed");

        let result = store.get_job_actions(job_id).await;

        assert!(matches!(
            result,
            Err(DatabaseError::Serialization(message))
                if message.contains("duration_ms must be non-negative")
        ));

        cleanup_job(&store, job_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn get_job_actions_treats_null_warnings_as_empty_vec(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let conn = store.conn().await.expect("conn should succeed");
        let action_id = Uuid::new_v4();
        let executed_at = Utc::now();
        let input = serde_json::json!({ "cmd": "echo hi" });

        conn.execute(
            r#"
            INSERT INTO job_actions (
                id, job_id, sequence_num, tool_name, input, output_raw, output_sanitized,
                sanitization_warnings, cost, duration_ms, success, error_message, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
            &[
                &action_id,
                &job_id,
                &0i32,
                &"shell",
                &input,
                &Some("hi".to_string()),
                &Some(serde_json::json!({ "stdout": "hi" })),
                &Option::<serde_json::Value>::None,
                &Some(Decimal::new(50, 2)),
                &5i32,
                &true,
                &Option::<String>::None,
                &executed_at,
            ],
        )
        .await
        .expect("insert job action should succeed");

        let actions = store
            .get_job_actions(job_id)
            .await
            .expect("get_job_actions should succeed");

        assert_eq!(actions.len(), 1);
        assert!(actions[0].sanitization_warnings.is_empty());

        cleanup_job(&store, job_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn get_job_actions_rejects_invalid_warning_payload_shape(
        #[future] seeded_store: Option<(Store, Uuid)>,
    ) {
        let Some((store, job_id)) = seeded_store.await else {
            return;
        };
        let conn = store.conn().await.expect("conn should succeed");
        let action_id = Uuid::new_v4();
        let executed_at = Utc::now();
        let input = serde_json::json!({ "cmd": "echo hi" });
        let warnings = serde_json::json!({ "unexpected": true });

        conn.execute(
            r#"
            INSERT INTO job_actions (
                id, job_id, sequence_num, tool_name, input, output_raw, output_sanitized,
                sanitization_warnings, cost, duration_ms, success, error_message, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
            &[
                &action_id,
                &job_id,
                &0i32,
                &"shell",
                &input,
                &Some("hi".to_string()),
                &Some(serde_json::json!({ "stdout": "hi" })),
                &warnings,
                &Some(Decimal::new(50, 2)),
                &5i32,
                &true,
                &Option::<String>::None,
                &executed_at,
            ],
        )
        .await
        .expect("insert job action should succeed");

        let result = store.get_job_actions(job_id).await;

        assert!(matches!(
            result,
            Err(DatabaseError::Serialization(message))
                if message.contains("invalid sanitization_warnings payload")
        ));

        cleanup_job(&store, job_id).await;
    }
}
